//! 本地 Mod 扫描：只认文件名不够用，得进 jar 里读一次元数据。
//!
//! 四种加载器各写各的描述文件：
//! - Fabric: `fabric.mod.json`
//! - Quilt:  `quilt.mod.json`（id 在 `quilt_loader` 里）
//! - Forge:  `META-INF/mods.toml`
//! - NeoForge: `META-INF/neoforge.mods.toml`
//!
//! 读取时一律“读到就用，读不到就留空”，因为玩家目录里永远躺着几个改坏的 jar，
//! 一个坏文件不该让整个列表报错。
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::minecraft::isolate;
use crate::state::IsolationMode;

/// 界面上的一条本地 Mod
#[derive(Debug, Clone, Serialize)]
pub struct LocalMod {
    /// 磁盘上的文件名（含 `.disabled` 后缀，删除与启停都靠它定位）
    pub file_name: String,
    pub path: String,
    /// `.jar.disabled` 视为已禁用（不删文件，只是启动时不加载）
    pub enabled: bool,
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    /// fabric / quilt / forge / neoforge，靠命中的描述文件判断
    pub loader: Option<String>,
    pub size: u64,
}

/// jar 里读出来的元数据
#[derive(Debug, Clone, Default)]
pub struct JarMeta {
    pub id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub loader: Option<String>,
}

/// Mod 装哪个目录跟着隔离模式走：non-fake 模式下 `versions/<id>/mods` 一存在，
/// 这个版本就会自动隔离，Mod 也就不会串到别的版本里去
pub fn mods_dir(root: &Path, id: &str, mode: IsolationMode) -> PathBuf {
    isolate::game_dir(root, id, mode).join("mods")
}

/// `.jar.disabled` → (`xxx.jar`, false)
pub fn split_disabled(file_name: &str) -> (String, bool) {
    match file_name.strip_suffix(".disabled") {
        Some(base) if base.ends_with(".jar") => (base.to_string(), true),
        _ => (file_name.to_string(), false),
    }
}

/// 从 MODS.TOML 里抠一个字段。不引 toml 库：只要三个值，
/// 而这个格式在所有版本的 Forge/NeoForge 里都长一个样（`key="value"`）
pub fn toml_field(body: &str, key: &str) -> Option<String> {
    for line in body.lines() {
        let line = line.trim();
        // 跳过注释行，否则 `# modId="x"` 会被当成真值
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim();
        let val = rest
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| rest.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(rest);
        if !val.is_empty() {
            return Some(val.to_string());
        }
    }
    None
}

#[derive(Deserialize)]
struct FabricJson {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: String,
}

#[derive(Deserialize)]
struct QuiltJson {
    #[serde(default)]
    quilt_loader: Option<QuiltLoader>,
}

#[derive(Deserialize)]
struct QuiltLoader {
    #[serde(default)]
    id: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    metadata: Option<QuiltMeta>,
}

#[derive(Deserialize)]
struct QuiltMeta {
    #[serde(default)]
    name: String,
}

fn non_empty(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

pub fn parse_fabric(body: &str) -> Option<JarMeta> {
    let j: FabricJson = serde_json::from_str(body).ok()?;
    let id = non_empty(j.id)?;
    Some(JarMeta {
        id: Some(id),
        name: non_empty(j.name),
        version: non_empty(j.version),
        loader: Some("fabric".into()),
    })
}

pub fn parse_quilt(body: &str) -> Option<JarMeta> {
    let j: QuiltJson = serde_json::from_str(body).ok()?;
    let q = j.quilt_loader?;
    let id = non_empty(q.id)?;
    Some(JarMeta {
        id: Some(id),
        name: q.metadata.and_then(|m| non_empty(m.name)),
        version: non_empty(q.version),
        loader: Some("quilt".into()),
    })
}

pub fn parse_toml(body: &str, loader: &str) -> Option<JarMeta> {
    let id = toml_field(body, "modId")?;
    Some(JarMeta {
        id: Some(id),
        name: toml_field(body, "displayName"),
        version: toml_field(body, "version"),
        loader: Some(loader.into()),
    })
}

fn read_entry(path: &Path, name: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;
    let mut entry = zip.by_name(name).ok()?;
    let mut body = String::new();
    entry.read_to_string(&mut body).ok()?;
    Some(body)
}

/// 依次尝试四种描述文件。顺序上先 Fabric 再 Quilt：
/// Quilt 兼容 Fabric 的 Mod，所以带 `fabric.mod.json` 的 jar 在 Quilt 环境里也算 Fabric 包
pub fn meta_from_jar(path: &Path) -> JarMeta {
    if let Some(body) = read_entry(path, "fabric.mod.json") {
        if let Some(m) = parse_fabric(&body) {
            return m;
        }
    }
    if let Some(body) = read_entry(path, "quilt.mod.json") {
        if let Some(m) = parse_quilt(&body) {
            return m;
        }
    }
    if let Some(body) = read_entry(path, "META-INF/neoforge.mods.toml") {
        if let Some(m) = parse_toml(&body, "neoforge") {
            return m;
        }
    }
    if let Some(body) = read_entry(path, "META-INF/mods.toml") {
        if let Some(m) = parse_toml(&body, "forge") {
            return m;
        }
    }
    JarMeta::default()
}

/// 扫一遍 mods 目录。目录不存在就是空列表，不是错误
pub fn scan(dir: &Path) -> Vec<LocalMod> {
    let mut out: Vec<LocalMod> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if !file_name.to_ascii_lowercase().contains(".jar") {
            continue;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let (_, enabled) = split_disabled(&file_name);
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let meta = meta_from_jar(&path);
        out.push(LocalMod {
            file_name,
            path: path.to_string_lossy().into_owned(),
            enabled,
            id: meta.id,
            name: meta.name,
            version: meta.version,
            loader: meta.loader,
            size,
        });
    }
    out.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_suffix_detected() {
        assert_eq!(
            split_disabled("sodium-0.5.8.jar.disabled"),
            ("sodium-0.5.8.jar".to_string(), true)
        );
        assert_eq!(
            split_disabled("sodium-0.5.8.jar"),
            ("sodium-0.5.8.jar".to_string(), false)
        );
        // 只有 .jar 后面才是禁用，别把普通文件当 Mod
        assert_eq!(
            split_disabled("readme.disabled"),
            ("readme.disabled".to_string(), false)
        );
    }

    #[test]
    fn toml_field_reads_quoted_value() {
        let body = "modLoader=\"javafml\"\nmodId=\"jei\"\nversion=\"15.2.0.27\"\n";
        assert_eq!(toml_field(body, "modId").as_deref(), Some("jei"));
        assert_eq!(toml_field(body, "version").as_deref(), Some("15.2.0.27"));
        assert_eq!(toml_field(body, "作者"), None);
    }

    #[test]
    fn toml_field_ignores_comment_and_prefix() {
        // modIdExtra 不能被当成 modId；注释掉的行也不算
        let body = "# modId=\"ghost\"\nmodIdExtra=\"x\"\nmodId = \"real\"\n";
        assert_eq!(toml_field(body, "modId").as_deref(), Some("real"));
    }

    #[test]
    fn toml_field_handles_single_quote_and_bare() {
        assert_eq!(toml_field("modId='lit'\n", "modId").as_deref(), Some("lit"));
        assert_eq!(toml_field("modId=bare\n", "modId").as_deref(), Some("bare"));
    }

    #[test]
    fn fabric_json_parsed() {
        let body = r#"{"id":"sodium","name":"Sodium","version":"0.5.8","depends":{}}"#;
        let m = parse_fabric(body).unwrap();
        assert_eq!(m.id.as_deref(), Some("sodium"));
        assert_eq!(m.name.as_deref(), Some("Sodium"));
        assert_eq!(m.version.as_deref(), Some("0.5.8"));
        assert_eq!(m.loader.as_deref(), Some("fabric"));
    }

    #[test]
    fn fabric_json_without_id_rejected() {
        assert!(parse_fabric(r#"{"name":"x"}"#).is_none());
        assert!(parse_fabric("not json").is_none());
    }

    #[test]
    fn quilt_json_nested() {
        let body = r#"{"quilt_loader":{"id":"qsl","version":"1.0.0","metadata":{"name":"QSL"}}}"#;
        let m = parse_quilt(body).unwrap();
        assert_eq!(m.id.as_deref(), Some("qsl"));
        assert_eq!(m.name.as_deref(), Some("QSL"));
        assert_eq!(m.loader.as_deref(), Some("quilt"));
    }

    #[test]
    fn toml_meta_picks_loader() {
        let m = parse_toml("modId=\"neoforge_mod\"\ndisplayName=\"NF\"\n", "neoforge").unwrap();
        assert_eq!(m.id.as_deref(), Some("neoforge_mod"));
        assert_eq!(m.name.as_deref(), Some("NF"));
        assert_eq!(m.loader.as_deref(), Some("neoforge"));
    }
}