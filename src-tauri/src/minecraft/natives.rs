//! native 库解压。
//!
//! 26.3 的 JVM 参数是 `-Djava.library.path=${natives_directory}/java`，
//! 所以解压目标必须是 `isolate::native_extract_dir()`，不能直接用 natives 根。
//!
//! 只解压 .dll / .so / .dylib / .jnilib，跳过 META-INF 与签名文件；
//! 同名同大小则跳过，重复启动时这步几乎不花时间。

use std::fs::{self, File};
use std::io;
use std::path::Path;

use zip::ZipArchive;

use crate::error::{AppError, AppResult};
use crate::minecraft::{classpath, isolate, rules};
use crate::minecraft::version_json::VersionJson;

#[derive(Debug, Default, Clone)]
pub struct NativesReport {
    /// 本次真正写出去的库文件数
    pub extracted: usize,
    /// 已存在而跳过的
    pub skipped: usize,
    /// 对应 jar 不在本地
    pub missing: Vec<String>,
}

/// 只认动态库后缀，大小写不敏感（Windows 上也有 .DLL 的情况）
pub fn is_native_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".dll")
        || lower.ends_with(".so")
        || lower.ends_with(".dylib")
        || lower.ends_with(".jnilib")
}

/// 把单个 jar 里的动态库落到 dest_dir
fn extract_jar(jar: &Path, dest_dir: &Path, report: &mut NativesReport) -> AppResult<()> {
    let file = File::open(jar).map_err(|e| {
        AppError::Message(format!("打不开 native 包 {}：{e}", jar.display()))
    })?;
    let mut archive = ZipArchive::new(file).map_err(|e| {
        AppError::Message(format!("native 包已损坏 {}：{e}", jar.display()))
    })?;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };
        if entry.is_dir() {
            continue;
        }
        let raw_name = entry.name().to_string();
        let base = raw_name.rsplit('/').next().unwrap_or("").to_string();
        if base.is_empty() || !is_native_file(&base) {
            continue;
        }
        // zip 里塞 ../ 的条目直接丢弃，不拼路径
        if base.contains("..") || base.starts_with('/') || base.starts_with('\\') {
            log::warn!("跳过可疑的 native 条目：{raw_name}");
            continue;
        }

        let dest = dest_dir.join(&base);
        let size = entry.size();
        if let Ok(meta) = fs::metadata(&dest) {
            if meta.len() == size {
                report.skipped += 1;
                continue;
            }
        }

        crate::fsutil::ensure_dir(dest_dir)?;
        let mut out = File::create(&dest)?;
        io::copy(&mut entry, &mut out)?;
        report.extracted += 1;
    }
    Ok(())
}

/// 按版本 JSON 解压全部 native 包；缺 jar 不报错，返回在 missing 里
pub fn extract(root: &Path, id: &str, version: &VersionJson) -> AppResult<NativesReport> {
    let platform = rules::Platform::current();
    let features = rules::Features::default();
    let target = isolate::native_extract_dir(root, id);
    let libs = isolate::libraries_dir(root);

    let mut report = NativesReport::default();
    let entries = classpath::collect(version, &platform, &features);
    let natives: Vec<_> = entries.into_iter().filter(|e| e.native).collect();
    if natives.is_empty() {
        return Ok(report);
    }

    crate::fsutil::ensure_dir(&target)?;
    for entry in natives {
        let jar = libs.join(&entry.path);
        if !jar.is_file() {
            report.missing.push(entry.path.clone());
            continue;
        }
        extract_jar(&jar, &target, &mut report)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_platform_suffixes() {
        assert!(is_native_file("lwjgl.dll"));
        assert!(is_native_file("liblwjgl.so"));
        assert!(is_native_file("liblwjgl.dylib"));
        assert!(is_native_file("libjinput-osx.jnilib"));
        assert!(is_native_file("LIBGL.DLL"));
    }

    #[test]
    fn rejects_classes_and_meta() {
        assert!(!is_native_file("a.class"));
        assert!(!is_native_file("MANIFEST.MF"));
        assert!(!is_native_file("META-INF/foo.dsa"));
    }
}
