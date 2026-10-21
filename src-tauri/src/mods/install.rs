//! Mod 文件操作：下载安装、启用/禁用、删除。
//!
//! 全部落在 `game_dir/mods/` 里，文件名完全交给 Modrinth 给的名字，
//! 但必须先过一遍白名单校验 —— 这个字符串来自网络，直接拼路径就是路径穿越。
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::download::file::{self, FileTask};
use crate::error::{AppError, AppResult};
use crate::fsutil;

use super::modrinth::ModFile;

/// 文件名只允许“普通文件名”：无路径分隔符、无 `..`、必须以 .jar 结尾
pub fn safe_file_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Message("Mod 文件名为空".into()));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(AppError::Message(format!("Mod 文件名不合法：{name}")));
    }
    if !name.to_ascii_lowercase().ends_with(".jar") {
        return Err(AppError::Message(format!(
            "只装 .jar 文件，收到的是 {name}"
        )));
    }
    Ok(name.to_string())
}

/// 安装结果：`skipped=true` 表示同名校验通过，没真下
pub struct Installed {
    pub path: PathBuf,
    pub skipped: bool,
}

/// 下一个 Mod 文件到 mods 目录
pub fn install(
    client: &reqwest::blocking::Client,
    dir: &Path,
    file: &ModFile,
    timeout: Duration,
) -> AppResult<Installed> {
    let name = safe_file_name(&file.filename)?;
    fsutil::ensure_dir(dir)?;
    let dest = dir.join(&name);
    let task = FileTask::new(file.url.clone(), dest.clone(), file.sha1.clone(), file.size);
    let skipped = file::already_ok(&task);
    // download_file 内部也会判已就位，这里先算一次只为把结果告诉界面
    file::download_file(client, &task, timeout, &mut |_, _| {})?;
    Ok(Installed { path: dest, skipped })
}

/// 目标文件名：启用就是裸 `xxx.jar`，禁用就补 `.disabled`
fn target_name(base: &str, enable: bool) -> String {
    if enable {
        base.to_string()
    } else {
        format!("{base}.disabled")
    }
}

/// 当前状态就是目标状态：已经启用还要启用、已经禁用还要禁用。
/// 这两种情况必须原地返回，否则会把目标当成「已存在的同名文件」直接报错
fn is_noop(currently_disabled: bool, enable: bool) -> bool {
    currently_disabled != enable
}

/// 禁用 = 加 `.disabled` 后缀；重新启用就去掉。返回新的文件名
pub fn toggle(dir: &Path, file_name: &str, enable: bool) -> AppResult<String> {
    let (base, currently_disabled) = super::local::split_disabled(file_name);
    let base = safe_file_name(&base)?;
    let src = dir.join(file_name);
    if !src.is_file() {
        return Err(AppError::Message(format!("找不到 {file_name}")));
    }
    let target = target_name(&base, enable);
    // 已经是目标状态就直接返回，重复点不该报错
    if is_noop(currently_disabled, enable) {
        return Ok(target);
    }
    let dst = dir.join(&target);
    if dst.exists() {
        return Err(AppError::Message(format!(
            "{target} 已存在，先处理掉那个文件"
        )));
    }
    std::fs::rename(&src, &dst)
        .map_err(|e| AppError::Message(format!("改名失败：{e}")))?;
    Ok(target)
}

/// 删除一个 Mod 文件（只删 jar / jar.disabled，不碰别的）
pub fn remove(dir: &Path, file_name: &str) -> AppResult<()> {
    let (base, _) = super::local::split_disabled(file_name);
    let base = safe_file_name(&base)?;
    // 用校验过的 base 反推真实路径，避免拿外部传入的 file_name 直接拼
    let actual = if file_name.ends_with(".disabled") {
        format!("{base}.disabled")
    } else {
        base
    };
    let path = dir.join(&actual);
    if !path.is_file() {
        return Err(AppError::Message(format!("找不到 {actual}")));
    }
    std::fs::remove_file(&path).map_err(|e| AppError::Message(format!("删除失败：{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_name_accepts_plain_jar() {
        assert_eq!(
            safe_file_name("sodium-fabric-0.5.8.jar").unwrap(),
            "sodium-fabric-0.5.8.jar"
        );
    }

    #[test]
    fn safe_name_rejects_traversal() {
        assert!(safe_file_name("../evil.jar").is_err());
        assert!(safe_file_name("sub/dir/mod.jar").is_err());
        assert!(safe_file_name("sub\\dir\\mod.jar").is_err());
        assert!(safe_file_name("..").is_err());
    }

    #[test]
    fn safe_name_rejects_non_jar() {
        assert!(safe_file_name("mod.zip").is_err());
        assert!(safe_file_name("mod.jar.exe").is_err());
        assert!(safe_file_name("").is_err());
    }

    #[test]
    fn toggle_noop_only_when_state_matches() {
        // 已启用 + 要启用、已禁用 + 要禁用：原地返回，绝不 rename
        assert!(is_noop(false, true));
        assert!(is_noop(true, false));
        // 这两个是真要改名的情况，不能被当成 noop
        assert!(!is_noop(false, false));
        assert!(!is_noop(true, true));
    }

    #[test]
    fn toggle_target_name() {
        // enable=true 时目标名不带 disabled
        let (base, disabled) = super::super::local::split_disabled("a.jar.disabled");
        assert!(disabled);
        assert_eq!(base, "a.jar");
        assert_eq!(target_name(&base, true), "a.jar");
        assert_eq!(target_name(&base, false), "a.jar.disabled");
    }
}