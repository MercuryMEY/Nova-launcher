//! Mojang 官方 Java 运行时的清单与安装。
//!
//! 清单结构：`{ platform: { component: { manifest: {url}, version: {name} } } }`，
//! 组件清单里再列每个文件的 url / sha1 / target。
//!
//! 注意：BMCLAPI 不代理 java-runtime，这里只能直连官方，失败时要给清楚提示。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::download::file::{download_file, FileTask};
use crate::download::mirror::SourceChoice;
use crate::download::plan;
use crate::error::{AppError, AppResult};
use crate::fsutil;

/// 官方启动器使用的固定清单哈希，与启动器版本无关
pub const RUNTIME_INDEX: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// 当前平台在清单里的键
pub fn platform_key() -> &'static str {
    if cfg!(windows) {
        if cfg!(target_arch = "aarch64") {
            "windows-arm64"
        } else {
            "windows-x64"
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "mac-os-arm64"
        } else {
            "mac-os"
        }
    } else if cfg!(target_arch = "aarch64") {
        "linux-arm64"
    } else {
        "linux"
    }
}

#[derive(Debug, Deserialize)]
pub struct RuntimeEntry {
    pub manifest: Option<ManifestRef>,
    pub version: Option<RuntimeVersion>,
    #[serde(default)]
    pub availability: Option<Availability>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestRef {
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeVersion {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Availability {
    #[serde(default)]
    pub group: Option<i32>,
    #[serde(default)]
    pub progress: Option<i32>,
}

pub type RuntimeIndex = BTreeMap<String, BTreeMap<String, RuntimeEntry>>;

#[derive(Debug, Deserialize)]
pub struct ComponentManifest {
    pub files: BTreeMap<String, RuntimeFile>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeFile {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub downloads: Option<RuntimeDownloads>,
    #[serde(default)]
    pub executable: bool,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeDownloads {
    #[serde(default)]
    pub raw: Option<RawDownload>,
}

#[derive(Debug, Deserialize)]
pub struct RawDownload {
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

fn parse_major_loose(v: &str) -> Option<u32> {
    v.split(|c: char| c == '.' || c == '-' || c == '_' || c == '+')
        .next()?
        .parse()
        .ok()
}

/// 选一个组件：先认版本 JSON 指定的 component，
/// 失效时按清单里的 `version.name` 找主版本最接近（且不低于目标）的那个。
pub fn choose_component(
    index: &RuntimeIndex,
    platform: &str,
    preferred: Option<&str>,
    target_major: u32,
) -> Option<String> {
    let table = index.get(platform)?;
    if let Some(name) = preferred {
        if table.contains_key(name) {
            return Some(name.to_string());
        }
    }
    let mut best: Option<(u32, String)> = None;
    for (name, entry) in table {
        let major = entry
            .version
            .as_ref()
            .and_then(|v| parse_major_loose(&v.name))
            .unwrap_or(0);
        if major == 0 || major < target_major {
            continue;
        }
        match &best {
            Some((m, _)) if *m <= major => {}
            _ => best = Some((major, name.clone())),
        }
    }
    best.map(|(_, name)| name)
}

/// `<mc>/runtime/<component>/<platform>`
pub fn runtime_root(mc_root: &Path, component: &str, platform: &str) -> PathBuf {
    mc_root.join("runtime").join(component).join(platform)
}

/// 可执行文件位置：`<root>/<component>/bin/java[.exe]`
pub fn java_binary(root: &Path, component: &str) -> PathBuf {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    root.join(component).join("bin").join(exe)
}

pub fn fetch_index(
    client: &reqwest::blocking::Client,
    timeout: Duration,
) -> AppResult<RuntimeIndex> {
    let (index, _url) = plan::fetch_json::<RuntimeIndex>(
        client,
        RUNTIME_INDEX,
        SourceChoice::OfficialFirst,
        timeout,
    )?;
    Ok(index)
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perm = meta.permissions();
        perm.set_mode(perm.mode() | 0o755);
        let _ = std::fs::set_permissions(path, perm);
    }
}

#[cfg(windows)]
fn make_executable(_path: &Path) {}

/// 安装并返回 java 可执行文件路径。`progress(stage, done, total)`。
pub fn install(
    client: &reqwest::blocking::Client,
    mc_root: &Path,
    component: &str,
    platform: &str,
    entry: &RuntimeEntry,
    timeout: Duration,
    progress: &mut dyn FnMut(&str, u64, u64),
) -> AppResult<PathBuf> {
    let manifest_ref = entry.manifest.as_ref().ok_or_else(|| {
        AppError::Message("官方清单里没有这个 Java 运行时的下载信息".to_string())
    })?;

    progress("正在获取 Java 运行时清单", 0, 0);
    let (manifest, _url) = plan::fetch_json::<ComponentManifest>(
        client,
        &manifest_ref.url,
        SourceChoice::OfficialFirst,
        timeout,
    )?;

    let root = runtime_root(mc_root, component, platform);
    fsutil::ensure_dir(&root)?;

    let total_files = manifest
        .files
        .values()
        .filter(|f| f.kind == "file")
        .count()
        .max(1) as u64;
    let mut done_files: u64 = 0;

    for (rel, entry) in manifest.files.iter() {
        let target = entry.target.as_deref().unwrap_or(rel.as_str());
        // 清单被篡改的兜底：绝对路径与 .. 一律不落盘
        if target.contains("..") || Path::new(target).is_absolute() {
            log::warn!("跳过可疑的运行时条目：{target}");
            continue;
        }
        let dest = root.join(target);

        match entry.kind.as_str() {
            "directory" => {
                fsutil::ensure_dir(&dest)?;
            }
            "link" => {
                // Windows 上官方也用“复制文件 + executable 标记”的方式，这里不建符号链接
            }
            "file" => {
                if let Some(dl) = entry.downloads.as_ref().and_then(|d| d.raw.as_ref()) {
                    let task = FileTask::new(
                        dl.url.clone(),
                        dest.clone(),
                        dl.sha1.clone(),
                        dl.size,
                    );
                    let label = target.to_string();
                    download_file(client, &task, timeout, &mut |done, total| {
                        progress(&format!("正在下载 {label}"), done, total);
                    })?;
                    if entry.executable {
                        make_executable(&dest);
                    }
                }
                done_files += 1;
                progress(
                    &format!("Java 运行时已就位 {done_files}/{total_files}"),
                    done_files,
                    total_files,
                );
            }
            _ => {}
        }
    }

    let java = java_binary(&root, component);
    if !java.is_file() {
        return Err(AppError::Message(format!(
            "Java 运行时装完了，但没找到可执行文件：{}",
            java.display()
        )));
    }
    Ok(java)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_with(items: &[(&str, &str)]) -> RuntimeIndex {
        let mut table: BTreeMap<String, RuntimeEntry> = BTreeMap::new();
        for (name, version) in items {
            table.insert(
                name.to_string(),
                RuntimeEntry {
                    manifest: None,
                    version: Some(RuntimeVersion {
                        name: version.to_string(),
                    }),
                    availability: None,
                },
            );
        }
        let mut idx = RuntimeIndex::new();
        idx.insert("windows-x64".to_string(), table);
        idx
    }

    #[test]
    fn preferred_component_wins_when_present() {
        let idx = index_with(&[("java-runtime-gamma", "17.0.8"), ("java-runtime-delta", "21.0.5")]);
        assert_eq!(
            choose_component(&idx, "windows-x64", Some("java-runtime-delta"), 17).as_deref(),
            Some("java-runtime-delta")
        );
    }

    #[test]
    fn fallback_picks_lowest_sufficient_major() {
        let idx = index_with(&[("java-runtime-gamma", "17.0.8"), ("java-runtime-delta", "21.0.5")]);
        assert_eq!(
            choose_component(&idx, "windows-x64", Some("gone"), 18).as_deref(),
            Some("java-runtime-delta")
        );
        assert_eq!(
            choose_component(&idx, "windows-x64", None, 17).as_deref(),
            Some("java-runtime-gamma")
        );
        assert_eq!(choose_component(&idx, "windows-x64", None, 25), None);
    }

    #[test]
    fn unknown_platform_returns_none() {
        let idx = index_with(&[("java-runtime-gamma", "17.0.8")]);
        assert_eq!(choose_component(&idx, "plan9", None, 17), None);
    }

    #[test]
    fn paths_follow_official_layout() {
        let root = Path::new("/mc");
        let r = runtime_root(root, "java-runtime-gamma", "windows-x64");
        assert_eq!(
            r,
            Path::new("/mc/runtime/java-runtime-gamma/windows-x64")
        );
        let exe = java_binary(&r, "java-runtime-gamma");
        // java_binary 返回的是可执行文件本身（<component>/bin/java），
        // 所以只能断言到文件名这一层，不能断言 ends_with("bin")
        let name = if cfg!(windows) { "java.exe" } else { "java" };
        assert_eq!(exe.file_name().and_then(|s| s.to_str()), Some(name));
        assert_eq!(
            exe.parent().and_then(|p| p.file_name()).and_then(|s| s.to_str()),
            Some("bin")
        );
    }

    #[test]
    fn platform_key_is_known() {
        let key = platform_key();
        assert!(key.starts_with("windows") || key.starts_with("mac-os") || key.starts_with("linux"));
    }
}
