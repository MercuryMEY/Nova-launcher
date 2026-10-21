//! 加载器元数据：查有哪些版本、拿 Fabric/Quilt 的 profile、拼 Forge/NeoForge 的安装器地址。
//!
//! 这里只做“查”和“拼字符串”，不落盘、不开进程，纯函数部分都有单测。
//! 真正的安装动作在 `install` 子模块里。
use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const FABRIC_META: &str = "https://meta.fabricmc.net";
pub const QUILT_META: &str = "https://meta.quiltmc.org";
pub const FORGE_MAVEN: &str = "https://maven.minecraftforge.net";
pub const NEOFORGE_MAVEN: &str = "https://maven.neoforged.net/releases";
pub const BMCLAPI: &str = "https://bmclapi2.bangbang93.com";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoaderKind {
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

impl LoaderKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
            Self::Quilt => "quilt",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fabric => "Fabric",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
            Self::Quilt => "Quilt",
        }
    }

    /// Forge 与 NeoForge 只能靠官方安装器装（要跑一堆 processor），
    /// Fabric / Quilt 拿到 profile JSON 就够了
    pub fn needs_installer(self) -> bool {
        matches!(self, Self::Forge | Self::NeoForge)
    }

    pub fn parse(s: &str) -> AppResult<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fabric" => Ok(Self::Fabric),
            "forge" => Ok(Self::Forge),
            "neoforge" | "neo" => Ok(Self::NeoForge),
            "quilt" => Ok(Self::Quilt),
            other => Err(AppError::Message(format!("不认识的加载器：{other}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LoaderVersion {
    /// 加载器自己的版本号：Fabric `0.16.5` / Forge `47.2.0` / NeoForge `21.1.9`
    pub id: String,
    /// 它对应的 MC 版本，避免前端传错
    pub game: String,
    /// 官方标记的稳定版；NeoForge 用“版本号里有没有 beta”判断
    pub stable: bool,
    pub note: String,
}

fn get_text(
    client: &reqwest::blocking::Client,
    urls: &[String],
    timeout: Duration,
) -> AppResult<String> {
    let mut last: Option<AppError> = None;
    for url in urls {
        match client.get(url).timeout(timeout).send() {
            Ok(resp) if resp.status().is_success() => {
                return resp
                    .text()
                    .map_err(|e| AppError::Message(format!("读取 {url} 的响应失败：{e}")));
            }
            Ok(resp) => {
                last = Some(AppError::Message(format!(
                    "HTTP {}：{url}",
                    resp.status().as_u16()
                )))
            }
            Err(e) => last = Some(AppError::Message(format!("连不上 {url}：{e}"))),
        }
    }
    Err(last.unwrap_or_else(|| AppError::Message("没有可用的下载源".into())))
}

/// 官方优先、镜像兜底；开了“优先镜像”就把顺序反过来
fn ordered(official: String, mirror: Option<String>, prefer_mirror: bool) -> Vec<String> {
    match (mirror, prefer_mirror) {
        (Some(m), true) => vec![m, official],
        (Some(m), false) => vec![official, m],
        (None, _) => vec![official],
    }
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
struct MetaVersionRow {
    #[serde(default)]
    version: String,
    #[serde(default = "yes")]
    stable: bool,
}

/// Forge 的 maven-metadata.json 是 `{ "1.20.1": ["1.20.1-47.2.0", ...] }`。
/// 直接命中 game 键最好；拿不到就把所有值扫一遍按前缀筛，兼容不同时期的格式。
pub fn forge_entries_for(map: &HashMap<String, Vec<String>>, game: &str) -> Vec<String> {
    let prefix = format!("{game}-");
    let mut out: Vec<String> = Vec::new();
    let pool: Vec<&String> = match map.get(game) {
        Some(list) => list.iter().collect(),
        None => map.values().flatten().collect(),
    };
    for full in pool {
        if let Some(rest) = full.strip_prefix(&prefix) {
            if !rest.is_empty() {
                out.push(rest.to_string());
            }
        }
    }
    out.sort_by(|a, b| version_key(b).cmp(&version_key(a)).then_with(|| b.cmp(a)));
    out.dedup();
    out
}

/// NeoForge 的版本号是 `21.1.9` 这种，第一个数字是 MC 的次版本，第二个是修订号。
/// `21.0.x` 对应 1.21（没有第三个数字），`20.4.x` 对应 1.20.4。
pub fn neoforge_game(neo: &str) -> Option<String> {
    let mut it = neo.split('.');
    let major: u32 = it.next()?.parse().ok()?;
    let minor: u32 = it.next()?.parse().ok()?;
    if major < 20 {
        return None;
    }
    if minor == 0 {
        Some(format!("1.{major}"))
    } else {
        Some(format!("1.{major}.{minor}"))
    }
}

/// 从 maven-metadata.xml 里扒所有 `<version>` 再按 MC 版本筛。
/// 为这点小事引一个 XML 库不值得，官方这个文件的结构非常稳定。
pub fn neoforge_versions_for(xml: &str, game: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        let Some(end) = after.find("</version>") else {
            break;
        };
        let v = after[..end].trim();
        if !v.is_empty() && neoforge_game(v).as_deref() == Some(game) {
            out.push(v.to_string());
        }
        rest = &after[end..];
    }
    // 自然序降序：21.1.10 排在 21.1.9 前面。按字符串 sort 会把 10 排到 9 之后
    out.sort_by(|a, b| version_key(b).cmp(&version_key(a)).then_with(|| b.cmp(a)));
    out.dedup();
    out
}

/// 版本号拆成数字段：`"21.1.10"` → `[21, 1, 10]`，非数字字符当分隔符
///
/// Forge 的 `47.2.0`、NeoForge 的 `21.1.10`、Fabric 的 `0.15.11` 都要靠它比大小，
/// 字典序会把 10 排到 9 前面
fn version_key(v: &str) -> Vec<u32> {
    v.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// 装好的版本目录名。Forge / NeoForge 的官方安装器自己决定目录名，
/// 这个值只在“装完没找到新目录”时当兜底线索用。
pub fn loader_id(kind: LoaderKind, game: &str, loader: &str) -> String {
    match kind {
        LoaderKind::Fabric => format!("fabric-loader-{loader}-{game}"),
        LoaderKind::Quilt => format!("quilt-loader-{loader}-{game}"),
        LoaderKind::Forge => format!("{game}-forge-{loader}"),
        LoaderKind::NeoForge => format!("{game}-neoforge-{loader}"),
    }
}

/// 取字符串开头的数字（`"5-pre1"` → 5）。解析不到返回 None
///
/// 官方版本号里预发布后缀很多（1.20.5-pre1 / 1.21-rc1），
/// 直接 `parse::<u32>()` 会整段失败，掉进默认值把版本判错
fn leading_num(s: &str) -> Option<u32> {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// 安装器要用的 Java 大版本：老版本 Forge 在 17 上跑不起来，新版本在 8 上更没戏
pub fn installer_java_major(game: &str) -> u32 {
    let mut it = game.split('.');
    let first = it.next().unwrap_or("");
    // 快照（24w14a）和预发布首段都不是数字，一律当新版处理 ——
    // 老版本压根没人装加载器，宁可给高版本也别给 Java 8
    let Some(major) = leading_num(first) else {
        return 21;
    };
    let minor = it.next().and_then(leading_num).unwrap_or(0);
    let patch = it.next().and_then(leading_num).unwrap_or(0);
    if major != 1 {
        return 21;
    }
    if minor >= 21 {
        return 21;
    }
    if minor == 20 {
        return if patch < 5 { 17 } else { 21 };
    }
    if minor >= 17 {
        return 17;
    }
    8
}

pub fn list(
    client: &reqwest::blocking::Client,
    kind: LoaderKind,
    game: &str,
    prefer_mirror: bool,
    timeout: Duration,
) -> AppResult<Vec<LoaderVersion>> {
    let rows: Vec<LoaderVersion> = match kind {
        LoaderKind::Fabric => {
            let official = format!("{FABRIC_META}/v2/versions/loader/{game}");
            let mirror = Some(format!("{BMCLAPI}/fabric-meta/v2/versions/loader/{game}"));
            let body = get_text(client, &ordered(official, mirror, prefer_mirror), timeout)?;
            let list: Vec<MetaVersionRow> = serde_json::from_str(&body)?;
            list.into_iter()
                .filter(|r| !r.version.is_empty())
                .map(|r| LoaderVersion {
                    id: r.version,
                    game: game.to_string(),
                    stable: r.stable,
                    note: String::new(),
                })
                .collect()
        }
        LoaderKind::Quilt => {
            let official = format!("{QUILT_META}/v3/versions/loader/{game}");
            let mirror = Some(format!("{BMCLAPI}/quilt-meta/v3/versions/loader/{game}"));
            let body = get_text(client, &ordered(official, mirror, prefer_mirror), timeout)?;
            let list: Vec<MetaVersionRow> = serde_json::from_str(&body)?;
            list.into_iter()
                .filter(|r| !r.version.is_empty())
                .map(|r| LoaderVersion {
                    id: r.version,
                    game: game.to_string(),
                    stable: r.stable,
                    note: String::new(),
                })
                .collect()
        }
        LoaderKind::Forge => {
            let official = format!("{FORGE_MAVEN}/net/minecraftforge/forge/maven-metadata.json");
            let mirror = Some(format!("{BMCLAPI}/forge/maven-metadata.json"));
            let body = get_text(client, &ordered(official, mirror, prefer_mirror), timeout)?;
            let map: HashMap<String, Vec<String>> = serde_json::from_str(&body)?;
            forge_entries_for(&map, game)
                .into_iter()
                .map(|id| LoaderVersion {
                    id,
                    game: game.to_string(),
                    // Forge 的正式版没有 beta 标记，全部当稳定版
                    stable: true,
                    note: String::new(),
                })
                .collect()
        }
        LoaderKind::NeoForge => {
            let official = format!("{NEOFORGE_MAVEN}/net/neoforged/neoforge/maven-metadata.xml");
            let body = get_text(client, &ordered(official, None, prefer_mirror), timeout)?;
            neoforge_versions_for(&body, game)
                .into_iter()
                .map(|id| {
                    let beta = id.to_ascii_lowercase().contains("beta");
                    LoaderVersion {
                        id,
                        game: game.to_string(),
                        stable: !beta,
                        note: if beta { "测试版".into() } else { String::new() },
                    }
                })
                .collect()
        }
    };

    if rows.is_empty() {
        return Err(AppError::Message(format!(
            "{} 没有支持 {game} 的版本，换个 MC 版本试试",
            kind.label()
        )));
    }
    Ok(rows)
}

/// Fabric / Quilt：直接拿官方拼好的 profile（自带 inheritsFrom），返回 (版本目录名, 原始 JSON)
pub fn profile(
    client: &reqwest::blocking::Client,
    kind: LoaderKind,
    game: &str,
    loader: &str,
    prefer_mirror: bool,
    timeout: Duration,
) -> AppResult<(String, String)> {
    let (official, mirror) = match kind {
        LoaderKind::Fabric => (
            format!("{FABRIC_META}/v2/versions/loader/{game}/{loader}/profile/json"),
            Some(format!(
                "{BMCLAPI}/fabric-meta/v2/versions/loader/{game}/{loader}/profile/json"
            )),
        ),
        LoaderKind::Quilt => (
            format!("{QUILT_META}/v3/versions/loader/{game}/{loader}/profile/json"),
            Some(format!(
                "{BMCLAPI}/quilt-meta/v3/versions/loader/{game}/{loader}/profile/json"
            )),
        ),
        _ => {
            return Err(AppError::Message(format!(
                "{} 必须先跑官方安装器，没有现成的 profile",
                kind.label()
            )))
        }
    };
    let body = get_text(client, &ordered(official, mirror, prefer_mirror), timeout)?;
    let v: serde_json::Value = serde_json::from_str(&body)?;
    let id = v
        .get("id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::Message("加载器返回的 profile 里没有 id".into()))?;
    Ok((id, body))
}

/// 安装器 jar 的地址与落地文件名
pub fn installer_url(kind: LoaderKind, game: &str, loader: &str) -> AppResult<(String, String)> {
    match kind {
        LoaderKind::Forge => {
            let full = format!("{game}-{loader}");
            Ok((
                format!("{FORGE_MAVEN}/net/minecraftforge/forge/{full}/forge-{full}-installer.jar"),
                format!("forge-{full}-installer.jar"),
            ))
        }
        LoaderKind::NeoForge => Ok((
            format!(
                "{NEOFORGE_MAVEN}/net/neoforged/neoforge/{loader}/neoforge-{loader}-installer.jar"
            ),
            format!("neoforge-{loader}-installer.jar"),
        )),
        _ => Err(AppError::Message(format!("{} 不需要安装器", kind.label()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_filter_by_game() {
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        map.insert(
            "1.20.1".into(),
            vec![
                "1.20.1-47.1.0".into(),
                "1.20.1-47.2.0".into(),
                "1.20-46.0.14".into(),
            ],
        );
        let got = forge_entries_for(&map, "1.20.1");
        assert_eq!(got, vec!["47.2.0".to_string(), "47.1.0".to_string()]);
    }

    #[test]
    fn forge_prefix_does_not_swallow_neighbours() {
        // 1.20 的条目不能混进 1.20.1 的结果里
        let mut map: HashMap<String, Vec<String>> = HashMap::new();
        map.insert("other".into(), vec!["1.20-46.0.14".into()]);
        assert!(forge_entries_for(&map, "1.20.1").is_empty());
        assert_eq!(forge_entries_for(&map, "1.20"), vec!["46.0.14".to_string()]);
    }

    #[test]
    fn neoforge_game_mapping() {
        assert_eq!(neoforge_game("21.0.3").as_deref(), Some("1.21"));
        assert_eq!(neoforge_game("21.1.9").as_deref(), Some("1.21.1"));
        assert_eq!(neoforge_game("20.4.237").as_deref(), Some("1.20.4"));
        assert_eq!(neoforge_game("19.9.9"), None);
    }

    #[test]
    fn neoforge_xml_scan() {
        let xml = "<metadata><versioning><versions>\
            <version>21.1.9</version><version>21.1.10</version>\
            <version>21.0.1</version><version>20.4.237</version>\
            </versions></versioning></metadata>";
        // 新的在前
        assert_eq!(
            neoforge_versions_for(xml, "1.21.1"),
            vec!["21.1.10".to_string(), "21.1.9".to_string()]
        );
        assert_eq!(
            neoforge_versions_for(xml, "1.21"),
            vec!["21.0.1".to_string()]
        );
    }

    #[test]
    fn loader_ids() {
        assert_eq!(
            loader_id(LoaderKind::Fabric, "1.20.1", "0.15.0"),
            "fabric-loader-0.15.0-1.20.1"
        );
        assert_eq!(
            loader_id(LoaderKind::Forge, "1.20.1", "47.2.0"),
            "1.20.1-forge-47.2.0"
        );
        assert_eq!(
            loader_id(LoaderKind::NeoForge, "1.21.1", "21.1.9"),
            "1.21.1-neoforge-21.1.9"
        );
    }

    #[test]
    fn installer_java_picks_by_game() {
        assert_eq!(installer_java_major("1.16.5"), 8);
        assert_eq!(installer_java_major("1.17"), 17);
        assert_eq!(installer_java_major("1.20.1"), 17);
        assert_eq!(installer_java_major("1.20.4"), 17);
        assert_eq!(installer_java_major("1.20.6"), 21);
        assert_eq!(installer_java_major("1.21.1"), 21);
        assert_eq!(installer_java_major("24w14a"), 21);
    }

    #[test]
    fn installer_url_shape() {
        let (url, name) = installer_url(LoaderKind::Forge, "1.20.1", "47.2.0").unwrap();
        assert!(url.ends_with("/1.20.1-47.2.0/forge-1.20.1-47.2.0-installer.jar"));
        assert_eq!(name, "forge-1.20.1-47.2.0-installer.jar");
        let (url, name) = installer_url(LoaderKind::NeoForge, "1.21.1", "21.1.9").unwrap();
        assert!(url.ends_with("/21.1.9/neoforge-21.1.9-installer.jar"));
        assert_eq!(name, "neoforge-21.1.9-installer.jar");
    }

    #[test]
    fn kind_roundtrip() {
        for k in [
            LoaderKind::Fabric,
            LoaderKind::Forge,
            LoaderKind::NeoForge,
            LoaderKind::Quilt,
        ] {
            assert_eq!(LoaderKind::parse(k.key()).unwrap(), k);
        }
        assert!(LoaderKind::parse("liteloader").is_err());
    }
}