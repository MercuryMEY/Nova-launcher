//! 日志文件管理：列目录、尾部读取、删除。
//!
//! 文件名由前端回传，一律挡掉 `..` 与路径分隔符，
//! 否则一个 `read_log("../../config.json")` 就能读到配置。
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 只回看文件末尾 512KB：完整日志可能几十 MB，界面也不需要那么多
const TAIL_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct LogFileInfo {
    pub name: String,
    pub size: u64,
    pub modified: String,
}

fn safe_name(name: &str) -> AppResult<&str> {
    if name.is_empty()
        || name.contains("..")
        || name.contains('/')
        || name.contains('\\')
        || name.starts_with('.')
    {
        return Err(AppError::Message("日志文件名不合法".into()));
    }
    Ok(name)
}

pub fn list(dir: &Path) -> Vec<LogFileInfo> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".log") {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = meta
            .modified()
            .map(|t| {
                chrono::DateTime::<chrono::Local>::from(t)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_default();
        out.push(LogFileInfo {
            name,
            size: meta.len(),
            modified,
        });
    }
    // 新的在前
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out
}

/// 返回最后 `max_lines` 行。`truncated` 表示前面还有内容没读。
pub fn read_tail(dir: &Path, name: &str, max_lines: usize) -> AppResult<(Vec<String>, bool)> {
    let path = dir.join(safe_name(name)?);
    if !path.is_file() {
        return Err(AppError::NotFound(name.to_string()));
    }
    let mut file = std::fs::File::open(&path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    if start > 0 {
        file.seek(SeekFrom::Start(start))?;
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    // 截断点可能落在一个汉字中间，用 lossy 而不是 read_to_string
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let truncated = start > 0;
    if lines.len() > max_lines {
        let keep = lines.split_off(lines.len() - max_lines);
        return Ok((keep, true));
    }
    Ok((lines, truncated))
}

pub fn delete(dir: &Path, name: &str) -> AppResult<()> {
    let path = dir.join(safe_name(name)?);
    if !path.is_file() {
        return Err(AppError::NotFound(name.to_string()));
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_path_traversal() {
        assert!(safe_name("../config.json").is_err());
        assert!(safe_name("a/b.log").is_err());
        assert!(safe_name(".hidden").is_err());
        assert!(safe_name("").is_err());
    }

    #[test]
    fn accepts_plain_log_name() {
        assert!(safe_name("26.3-20261021-004512.log").is_ok());
    }
}
