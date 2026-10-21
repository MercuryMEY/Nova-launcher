//! Modrinth API v2 的最小封装：搜索项目、拿某个项目在指定版本/加载器下的文件。
//!
//! 只做这两件事：Modrinth 的项目详情里大段 markdown 对我们没用，
//! 搜索结果的图标 + 标题 + 简介已经够摆了。
//!
//! 注意 Modrinth 明确要求带 User-Agent 标识应用，不带会被限流甚至拒绝。
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

const API: &str = "https://api.modrinth.com/v2";
const USER_AGENT: &str = "NovaLauncher/0.1.0 (Minecraft launcher; mod browsing)";

/// 搜索结果的单条
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub follows: u64,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    /// 这个项目支持哪些 MC 版本
    pub game_versions: Vec<String>,
}

#[derive(Serialize)]
pub struct SearchPage {
    pub hits: Vec<SearchHit>,
    pub total: u64,
    pub offset: u32,
}

/// 一个可下载的版本文件。Modrinth 每个版本可能挂多个 jar（如 sources），主文件靠 primary 标
#[derive(Debug, Clone, Serialize)]
pub struct ModFile {
    pub filename: String,
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    /// required / optional / incompatible / embedded
    pub dependency_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModVersion {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    /// release / beta / alpha
    pub version_type: String,
    pub downloads: u64,
    pub date_published: String,
    pub file: ModFile,
    pub dependencies: Vec<Dependency>,
}

// ---------- 原始响应 ----------

#[derive(Deserialize)]
struct RawSearch {
    #[serde(default)]
    hits: Vec<RawHit>,
    #[serde(default)]
    total_hits: u64,
    #[serde(default)]
    offset: u32,
}

#[derive(Deserialize)]
struct RawHit {
    project_id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    follows: u64,
    #[serde(default)]
    icon_url: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    /// 搜索结果里的 versions 是「支持的 MC 版本」，不是加载器版本
    #[serde(default)]
    versions: Vec<String>,
}

#[derive(Deserialize)]
struct RawVersion {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    version_number: String,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    version_type: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    date_published: String,
    #[serde(default)]
    files: Vec<RawFile>,
    #[serde(default)]
    dependencies: Vec<RawDep>,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    filename: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    hashes: RawHashes,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    primary: bool,
}

#[derive(Deserialize, Default)]
struct RawHashes {
    #[serde(default)]
    sha1: String,
}

#[derive(Deserialize)]
struct RawDep {
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    version_id: Option<String>,
    #[serde(default)]
    dependency_type: String,
}

// ---------- 纯函数（可测） ----------

/// 加载器 → Modrinth 的 categories facet 值。原版不给 facet（Mod 本来就要加载器）
pub fn loader_facet(loader: &str) -> Option<&'static str> {
    match loader.trim().to_ascii_lowercase().as_str() {
        "fabric" => Some("fabric"),
        "forge" => Some("forge"),
        "neoforge" => Some("neoforge"),
        "quilt" => Some("quilt"),
        _ => None,
    }
}

/// Modrinth 的 facets 是「数组的数组」，内层之间是 AND，外层之间是 OR。
/// 我们要的是「是 mod」且「支持这个 MC 版本」且「支持这个加载器」，所以三个内层数组。
pub fn facets(game: &str, loader: &str) -> String {
    let mut groups: Vec<String> = vec!["[\"project_type:mod\"]".to_string()];
    if !game.trim().is_empty() {
        groups.push(format!("[\"versions:{}\"]", game.trim()));
    }
    if let Some(l) = loader_facet(loader) {
        groups.push(format!("[\"categories:{l}\"]"));
    }
    format!("[{}]", groups.join(","))
}

/// 排序字段白名单。用户点错按钮不该让后端 400
pub fn sort_key(sort: &str) -> &'static str {
    match sort {
        "downloads" => "downloads",
        "follows" => "follows",
        "newest" => "newest",
        "updated" => "updated",
        _ => "relevance",
    }
}

/// Modrinth 的版本会同时挂 sources.jar 之类，必须挑 primary，没有就退回第一个
fn pick_file(files: &[RawFile]) -> Option<ModFile> {
    let chosen = files
        .iter()
        .find(|f| f.primary && !f.url.is_empty())
        .or_else(|| files.iter().find(|f| !f.url.is_empty()))?;
    Some(ModFile {
        filename: if chosen.filename.is_empty() {
            "download.jar".to_string()
        } else {
            chosen.filename.clone()
        },
        url: chosen.url.clone(),
        sha1: chosen.hashes.sha1.clone(),
        size: chosen.size,
    })
}

// ---------- 网络 ----------

fn get_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::blocking::Client,
    url: &str,
    params: &[(&str, String)],
    timeout: Duration,
) -> AppResult<T> {
    let mut req = client
        .get(url)
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .timeout(timeout);
    for (k, v) in params {
        req = req.query(&[(k, v)]);
    }
    let resp = req.send().map_err(|e| {
        AppError::Message(format!("连不上 Modrinth：{e}，可能需要代理"))
    })?;
    if !resp.status().is_success() {
        return Err(AppError::Message(format!(
            "Modrinth 返回 HTTP {}，稍后再试",
            resp.status().as_u16()
        )));
    }
    let body = resp
        .text()
        .map_err(|e| AppError::Message(format!("读取 Modrinth 响应失败：{e}")))?;
    serde_json::from_str(&body).map_err(|e| AppError::Message(format!("Modrinth 数据格式不对：{e}")))
}

/// 搜索 Mod。`query` 为空时就是「按版本 + 加载器浏览热门」
pub fn search(
    client: &reqwest::blocking::Client,
    game: &str,
    loader: &str,
    query: &str,
    sort: &str,
    offset: u32,
    limit: u32,
    timeout: Duration,
) -> AppResult<SearchPage> {
    let limit = limit.clamp(1, 50);
    let params = vec![
        ("query", query.trim().to_string()),
        ("facets", facets(game, loader)),
        ("index", sort_key(sort).to_string()),
        ("offset", offset.to_string()),
        ("limit", limit.to_string()),
    ];
    let raw: RawSearch = get_json(client, &format!("{API}/search"), &params, timeout)?;
    let hits = raw
        .hits
        .into_iter()
        .map(|h| SearchHit {
            project_id: h.project_id,
            slug: h.slug,
            title: h.title,
            description: h.description,
            author: h.author,
            downloads: h.downloads,
            follows: h.follows,
            icon_url: h.icon_url.filter(|u| !u.is_empty()),
            categories: h.categories,
            game_versions: h.versions,
        })
        .collect();
    Ok(SearchPage {
        hits,
        total: raw.total_hits,
        offset: raw.offset,
    })
}

/// 某个项目在指定 MC 版本 + 加载器下有哪些文件可下
pub fn versions(
    client: &reqwest::blocking::Client,
    project_id: &str,
    game: &str,
    loader: &str,
    timeout: Duration,
) -> AppResult<Vec<ModVersion>> {
    let mut params: Vec<(&str, String)> = Vec::new();
    if !game.trim().is_empty() {
        params.push(("game_versions", format!("[\"{}\"]", game.trim())));
    }
    if let Some(l) = loader_facet(loader) {
        params.push(("loaders", format!("[\"{l}\"]")));
    }
    let raw: Vec<RawVersion> = get_json(
        client,
        &format!("{API}/project/{project_id}/version"),
        &params,
        timeout,
    )?;

    let mut out: Vec<ModVersion> = Vec::new();
    for v in raw {
        // 没有可下文件的版本直接丢掉，界面点进去也是空的
        let Some(file) = pick_file(&v.files) else {
            continue;
        };
        out.push(ModVersion {
            id: v.id,
            name: v.name,
            version_number: v.version_number,
            game_versions: v.game_versions,
            loaders: v.loaders,
            version_type: v.version_type,
            downloads: v.downloads,
            date_published: v.date_published,
            file,
            dependencies: v
                .dependencies
                .into_iter()
                .map(|d| Dependency {
                    project_id: d.project_id,
                    version_id: d.version_id,
                    dependency_type: d.dependency_type,
                })
                .collect(),
        });
    }
    // Modrinth 本身按发布时间倒序，这里再兜一层，保证界面第一项就是最新
    out.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    if out.is_empty() {
        return Err(AppError::Message(format!(
            "这个 Mod 没有支持 {game}{} 的版本",
            if loader.is_empty() {
                String::new()
            } else {
                format!(" + {loader}")
            }
        )));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facets_include_game_and_loader() {
        let f = facets("1.20.1", "fabric");
        assert_eq!(f, "[[\"project_type:mod\"],[\"versions:1.20.1\"],[\"categories:fabric\"]]");
    }

    #[test]
    fn facets_drop_unknown_loader() {
        // 不认识的加载器不该拼成一个必定搜不到的条件
        let f = facets("1.21", "vanilla");
        assert_eq!(f, "[[\"project_type:mod\"],[\"versions:1.21\"]]");
    }

    #[test]
    fn facets_without_game_only_type() {
        assert_eq!(facets("", ""), "[[\"project_type:mod\"]]");
    }

    #[test]
    fn sort_whitelist() {
        assert_eq!(sort_key("downloads"), "downloads");
        assert_eq!(sort_key("乱写的"), "relevance");
        assert_eq!(sort_key(""), "relevance");
    }

    #[test]
    fn pick_primary_file() {
        let files = vec![
            RawFile {
                filename: "mod-sources.jar".into(),
                url: "https://x/sources".into(),
                hashes: RawHashes::default(),
                size: 10,
                primary: false,
            },
            RawFile {
                filename: "mod-1.2.0.jar".into(),
                url: "https://x/mod".into(),
                hashes: RawHashes {
                    sha1: "abc".into(),
                },
                size: 20,
                primary: true,
            },
        ];
        let f = pick_file(&files).unwrap();
        assert_eq!(f.filename, "mod-1.2.0.jar");
        assert_eq!(f.sha1, "abc");
        assert_eq!(f.size, 20);
    }

    #[test]
    fn pick_file_without_primary_falls_back() {
        let files = vec![RawFile {
            filename: "only.jar".into(),
            url: "https://x/a".into(),
            hashes: RawHashes::default(),
            size: 1,
            primary: false,
        }];
        assert_eq!(pick_file(&files).unwrap().filename, "only.jar");
    }

    #[test]
    fn pick_file_skips_empty_url() {
        // 早期有些条目 url 是空串，宁可当没有
        let files = vec![RawFile {
            filename: "broken.jar".into(),
            url: String::new(),
            hashes: RawHashes::default(),
            size: 0,
            primary: true,
        }];
        assert!(pick_file(&files).is_none());
    }
}