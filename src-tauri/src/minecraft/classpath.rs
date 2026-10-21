//! classpath 组装：libraries 目录 + 平台分类器 + client.jar 收尾。
//! 顺序很重要：原版 client.jar 必须放在最后，
//! 1.13 之后 Mixin 和 Forge 都依赖“原版类靠后”这个顺序。

use std::path::Path;

use serde::Serialize;

use crate::minecraft::rules::{allows, Features, Platform};
use crate::minecraft::version_json::{Artifact, Library, VersionJson};

#[derive(Debug, Clone, Serialize)]
pub struct ClasspathEntry {
    /// libraries 目录下的相对路径，统一用 / 分隔
    pub path: String,
    pub url: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// true 表示这是 natives 包，需要解压而不是进 classpath
    pub native: bool,
}

/// maven 坐标 -> 相对路径，例如
/// org.lwjgl:lwjgl:3.3.3 -> org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar
pub fn maven_to_path(name: &str) -> Option<String> {
    let (coord, ext) = match name.split_once('@') {
        Some((c, e)) => (c, e),
        None => (name, "jar"),
    };
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = parts.get(3).copied();

    let mut file = format!("{artifact}-{version}");
    if let Some(c) = classifier {
        file.push('-');
        file.push_str(c);
    }
    file.push('.');
    file.push_str(ext);

    Some(format!(
        "{}/{artifact}/{version}/{file}",
        group.replace('.', "/")
    ))
}

/// 把一个带 classifier 的坐标拼成路径，老版本 JSON 里 natives 只有名字没有 path
fn path_with_classifier(name: &str, classifier: &str) -> Option<String> {
    let base = name.split('@').next()?;
    let parts: Vec<&str> = base.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    maven_to_path(&format!(
        "{}:{}:{}:{}",
        parts[0], parts[1], parts[2], classifier
    ))
}

fn entry(
    lib: &Library,
    artifact: Option<&Artifact>,
    fallback: Option<String>,
    native: bool,
) -> Option<ClasspathEntry> {
    let path = artifact.and_then(|a| a.path.clone()).or(fallback)?;
    let sha1 = artifact
        .and_then(|a| a.sha1.clone())
        .or_else(|| lib.sha1.clone());
    let size = artifact.and_then(|a| a.size).or(lib.size);
    // Fabric / Quilt 只给仓库根（形如 https://maven.fabricmc.net/），
    // 文件名要自己按 maven 规则拼；官方 JSON 没有这个字段，行为不变
    let url = artifact.and_then(|a| a.url.clone()).or_else(|| {
        lib.url
            .as_ref()
            .filter(|b| b.starts_with("http"))
            .map(|base| format!("{}/{}", base.trim_end_matches('/'), path))
    });
    Some(ClasspathEntry {
        path,
        url,
        sha1,
        size,
        native,
    })
}

/// 过滤掉不适用于当前平台的库（rules 求值）
pub fn collect(version: &VersionJson, platform: &Platform, features: &Features) -> Vec<ClasspathEntry> {
    let mut out: Vec<ClasspathEntry> = Vec::new();

    for lib in &version.libraries {
        if !allows(&lib.rules, platform, features) {
            continue;
        }
        out.extend(entries_of(lib, platform));
    }

    out
}

/// 新版 JSON 不再写 natives 字段，而是把 classifier 写进坐标：
/// org.lwjgl:lwjgl:3.4.1:natives-windows
/// 这类包必须解压到 natives 目录，不能进 classpath。
fn name_is_native(name: &str) -> bool {
    match name.split(':').nth(3) {
        Some(c) => c.split('@').next().unwrap_or(c).starts_with("natives"),
        None => false,
    }
}

fn entries_of(lib: &Library, platform: &Platform) -> Vec<ClasspathEntry> {
    let mut out = Vec::new();
    let artifact = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref());
    let native_by_name = name_is_native(&lib.name);

    // 普通 jar；新版 natives 包也是走 artifact，这里靠坐标识别
    if let Some(e) = entry(lib, artifact, maven_to_path(&lib.name), native_by_name) {
        out.push(e);
    }

    // natives 包：classifier 名里的 ${arch} 要按当前架构换成 64 / arm64
    if let Some(natives) = &lib.natives {
        if let Some(tpl) = natives.get(&platform.os_name) {
            let arch = if platform.os_arch == "arm64" { "arm64" } else { "64" };
            let classifier = tpl.replace("${arch}", arch);
            let art = lib
                .downloads
                .as_ref()
                .and_then(|d| d.classifiers.get(&classifier));
            if let Some(e) = entry(lib, art, path_with_classifier(&lib.name, &classifier), true) {
                out.push(e);
            }
        }
    }

    out
}

/// 拼成 Java 的 -cp 字符串，平台分隔符不同，client.jar 放最后
pub fn classpath_string(entries: &[ClasspathEntry], libraries_dir: &Path, client_jar: &Path) -> String {
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut items: Vec<String> = entries
        .iter()
        .filter(|e| !e.native)
        .map(|e| libraries_dir.join(&e.path).to_string_lossy().into_owned())
        .collect();
    items.push(client_jar.to_string_lossy().into_owned());
    items.join(sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_basic() {
        assert_eq!(
            maven_to_path("org.lwjgl:lwjgl:3.3.3").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar"
        );
    }

    #[test]
    fn maven_with_classifier_and_ext() {
        assert_eq!(
            maven_to_path("org.lwjgl:lwjgl:3.3.3:natives-macos").unwrap(),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-macos.jar"
        );
        assert_eq!(
            maven_to_path("de.oceanlabs.mcp:mcp:1.0@zip").unwrap(),
            "de/oceanlabs/mcp/mcp/1.0/mcp-1.0.zip"
        );
    }

    #[test]
    fn classpath_puts_client_jar_last() {
        let entries = vec![ClasspathEntry {
            path: "a/b/1.0/b-1.0.jar".into(),
            url: None,
            sha1: None,
            size: None,
            native: false,
        }];
        let s = classpath_string(&entries, Path::new("/root/libraries"), Path::new("/root/versions/x/x.jar"));
        let sep = if cfg!(windows) { ';' } else { ':' };
        let parts: Vec<&str> = s.split(sep).collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[1].ends_with("x.jar"));
    }
}
