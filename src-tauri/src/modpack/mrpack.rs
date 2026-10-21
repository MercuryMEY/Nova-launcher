//! `.mrpack` 解析：本质就是一个 zip，里面放一份 `modrinth.index.json`。
//!
//! 只做客户端安装，所以 `env.client == "unsupported"` 的文件（服务端专用）直接跳过；
//! 格式版本也只认 1，万一以后出了不兼容的 v2，宁可报错也不去猜字段含义。
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::loader::meta::LoaderKind;

/// 索引文件在 zip 里的固定位置
pub const INDEX_NAME: &str = "modrinth.index.json";
/// 通用覆盖目录，服务端也会用
pub const OVERRIDES: &str = "overrides";
/// 客户端专用覆盖目录，优先级高于 `overrides`
pub const CLIENT_OVERRIDES: &str = "client-overrides";
pub const SUPPORTED_FORMAT: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version_id: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub files: Vec<PackFile>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

impl Index {
    pub fn game_version(&self) -> &str {
        self.dependencies
            .get("minecraft")
            .map(|s| s.as_str())
            .unwrap_or("")
    }

    /// 依赖表里的加载器。键名是死的，顺序按常见程度排
    pub fn loader(&self) -> Option<(LoaderKind, String)> {
        let table: [(&str, LoaderKind); 4] = [
            ("fabric-loader", LoaderKind::Fabric),
            ("quilt-loader", LoaderKind::Quilt),
            ("neoforge", LoaderKind::NeoForge),
            ("forge", LoaderKind::Forge),
        ];
        for (key, kind) in table {
            if let Some(v) = self.dependencies.get(key) {
                if !v.trim().is_empty() {
                    return Some((kind, v.trim().to_string()));
                }
            }
        }
        None
    }

    /// 客户端要下的文件：客户端不可用的、没有下载地址的都滤掉
    pub fn client_files(&self) -> Vec<PackFile> {
        self.files
            .iter()
            .filter(|f| f.client_usable() && f.download_url().is_some())
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackFile {
    /// 相对实例目录的路径，例如 `mods/sodium.jar`
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub downloads: Vec<String>,
    #[serde(default)]
    pub hashes: Hashes,
    /// 官方字段名是 `fileSize`，少了 rename_all 恒为 0，
    /// 下载时「字节数等于声明值」这条校验等于没做
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub env: Env,
}

impl PackFile {
    /// `env.client` 缺省表示两端都要，只有显式 unsupported 才是纯服务端文件
    pub fn client_usable(&self) -> bool {
        !self.env.client.eq_ignore_ascii_case("unsupported")
    }

    /// 只认 http(s)：整合包里偶尔混进空串或本地路径，那类当没有下载源
    pub fn download_url(&self) -> Option<&str> {
        self.downloads
            .iter()
            .find(|u| u.starts_with("http://") || u.starts_with("https://"))
            .map(|s| s.as_str())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Hashes {
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub sha512: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Env {
    #[serde(default)]
    pub client: String,
    #[serde(default)]
    pub server: String,
}

/// 从字节解析索引。纯函数，单测直接喂 JSON 字符串
pub fn parse_index(bytes: &[u8]) -> AppResult<Index> {
    let index: Index = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Message(format!("整合包索引格式不对：{e}")))?;
    if index.format_version > SUPPORTED_FORMAT {
        return Err(AppError::Message(format!(
            "这个整合包是格式 v{}，当前只支持 v{SUPPORTED_FORMAT}",
            index.format_version
        )));
    }
    if index.game_version().trim().is_empty() {
        return Err(AppError::Message(
            "整合包里没写 Minecraft 版本，没法判断该装哪个版本".into(),
        ));
    }
    Ok(index)
}

/// 打开 zip 读索引。只读这一个条目，不解压整包
pub fn read_index(pack: &Path) -> AppResult<Index> {
    let file = std::fs::File::open(pack)
        .map_err(|e| AppError::Message(format!("打不开 {}：{e}", pack.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| AppError::Message(format!("这个文件不是 zip：{e}")))?;
    let mut entry = zip.by_name(INDEX_NAME).map_err(|_| {
        AppError::Message(format!(
            "不是 Modrinth 整合包：压缩包里没有 {INDEX_NAME}"
        ))
    })?;
    let mut buf = Vec::new();
    entry
        .read_to_end(&mut buf)
        .map_err(|e| AppError::Message(format!("读取 {INDEX_NAME} 失败：{e}")))?;
    parse_index(&buf)
}

/// 整合包内部的路径同样来自网络，拼磁盘前必须过一遍白名单
pub fn safe_rel_path(raw: &str) -> AppResult<String> {
    let path = raw.trim().replace('\\', "/");
    if path.is_empty() {
        return Err(AppError::Message("整合包里有空路径".into()));
    }
    if path.starts_with('/') {
        return Err(AppError::Message(format!("整合包里有绝对路径：{raw}")));
    }
    // Windows 盘符与 NTFS 数据流都靠冒号，一并拒掉
    if path.contains(':') {
        return Err(AppError::Message(format!("整合包里有非法路径：{raw}")));
    }
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.iter().any(|p| *p == "..") {
        return Err(AppError::Message(format!("整合包里有越界路径：{raw}")));
    }
    if parts.is_empty() {
        return Err(AppError::Message(format!("整合包里有空路径：{raw}")));
    }
    Ok(parts.join("/"))
}

/// 只留能当目录名的字符：字母数字（含汉字）、`-_.`，其余换成 `-`
fn sanitize(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    out.trim_matches(|c| c == '-' || c == '.').to_string()
}

/// 实例目录名。名字是作者随手取的，可能带空格、斜杠、emoji，全部要洗
pub fn instance_id(name: &str, fallback: &str) -> String {
    let first = sanitize(name);
    let id = if first.is_empty() { sanitize(fallback) } else { first };
    let id = if id.is_empty() { "pack".to_string() } else { id };
    // Windows 单层目录名上限 255 字节，留 60 个字符足够且不会踩线
    id.chars().take(60).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(extra: &str) -> String {
        format!(
            r#"{{
  "formatVersion": 1,
  "name": "My Pack",
  "versionId": "1.0.0",
  "summary": "test",
  "files": [
    {{
      "path": "mods/a.jar",
      "hashes": {{ "sha1": "abc", "sha512": "def" }},
      "env": {{ "client": "required", "server": "unsupported" }},
      "downloads": ["https://cdn.modrinth.com/a.jar"],
      "fileSize": 1234
    }},
    {{
      "path": "mods/serveronly.jar",
      "env": {{ "client": "unsupported" }},
      "downloads": ["https://cdn.modrinth.com/s.jar"]
    }}
  ],
  "dependencies": {{ "minecraft": "1.20.1", "fabric-loader": "0.15.11" }}{extra}
}}"#
        )
    }

    #[test]
    fn parse_minimal_index() {
        let idx = parse_index(sample("").as_bytes()).unwrap();
        assert_eq!(idx.name, "My Pack");
        assert_eq!(idx.game_version(), "1.20.1");
        let (kind, ver) = idx.loader().unwrap();
        assert_eq!(kind, LoaderKind::Fabric);
        assert_eq!(ver, "0.15.11");
    }

    #[test]
    fn client_files_skip_server_only() {
        let idx = parse_index(sample("").as_bytes()).unwrap();
        let files = idx.client_files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "mods/a.jar");
        assert_eq!(files[0].hashes.sha1, "abc");
        assert_eq!(files[0].file_size, 1234);
    }

    #[test]
    fn missing_minecraft_is_rejected() {
        let json = r#"{"formatVersion":1,"name":"x","dependencies":{}}"#;
        assert!(parse_index(json.as_bytes()).is_err());
    }

    #[test]
    fn newer_format_is_rejected() {
        let json = r#"{"formatVersion":2,"name":"x","dependencies":{"minecraft":"1.21"}}"#;
        assert!(parse_index(json.as_bytes()).is_err());
    }

    #[test]
    fn loader_keys_cover_three_families() {
        let json = r#"{"formatVersion":1,"name":"x","dependencies":{"minecraft":"1.21","neoforge":"21.1.9"}}"#;
        let idx = parse_index(json.as_bytes()).unwrap();
        assert_eq!(idx.loader().unwrap().0, LoaderKind::NeoForge);

        let json = r#"{"formatVersion":1,"name":"x","dependencies":{"minecraft":"1.12.2","forge":"14.23.5.2860"}}"#;
        let idx = parse_index(json.as_bytes()).unwrap();
        let (kind, ver) = idx.loader().unwrap();
        assert_eq!(kind, LoaderKind::Forge);
        assert_eq!(ver, "14.23.5.2860");

        // 纯原版整合包（比如只带资源包）没有加载器依赖
        let json = r#"{"formatVersion":1,"name":"x","dependencies":{"minecraft":"1.21"}}"#;
        assert!(parse_index(json.as_bytes()).unwrap().loader().is_none());
    }

    #[test]
    fn download_url_prefers_http() {
        let file = PackFile {
            path: "mods/a.jar".into(),
            downloads: vec!["".into(), "ftp://x/a.jar".into(), "https://cdn/a.jar".into()],
            hashes: Hashes::default(),
            file_size: 1,
            env: Env::default(),
        };
        assert_eq!(file.download_url(), Some("https://cdn/a.jar"));
    }

    #[test]
    fn rel_path_rejects_escape() {
        assert!(safe_rel_path("mods/a.jar").is_ok());
        assert!(safe_rel_path("config/x/y.toml").is_ok());
        assert!(safe_rel_path("../evil.jar").is_err());
        assert!(safe_rel_path("mods/../../evil.jar").is_err());
        assert!(safe_rel_path("/etc/passwd").is_err());
        assert!(safe_rel_path("C:/Windows/x.dll").is_err());
        assert!(safe_rel_path("").is_err());
    }

    #[test]
    fn instance_id_is_washable() {
        assert_eq!(instance_id("My Pack", "x"), "My-Pack");
        // 汉字保留，斜杠跟空格都会被换成 -
        assert_eq!(instance_id("整合包 / v2", "x"), "整合包---v2");
        // 名字全是符号时退回文件名
        assert_eq!(instance_id("///", "fallback"), "fallback");
        assert_eq!(instance_id("", ""), "pack");
    }
}