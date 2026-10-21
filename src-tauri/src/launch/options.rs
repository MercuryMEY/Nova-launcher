//! 启动前对 `options.txt` 的定点修改。
//!
//! 原版客户端没有「全屏」启动参数（version JSON 里只有 --width/--height），
//! 界面上那个全屏开关只能落到 options.txt。所以只改 `fullscreen:` 这一行，
//! 其余行原样保留 —— 用户在游戏里调的音量、按键、视角一个都不能丢。

use std::fs;
use std::path::Path;

use crate::error::AppResult;
use crate::state::FullscreenMode;

/// 按策略改写 `<game_dir>/options.txt` 的 fullscreen 行
///
/// `Ignore` 时一个字节都不碰；文件不存在时就只写这一行，
/// 游戏启动后自己会把其余默认值补齐
pub fn apply_fullscreen(game_dir: &Path, mode: FullscreenMode) -> AppResult<()> {
    let want = match mode {
        FullscreenMode::Ignore => return Ok(()),
        FullscreenMode::On => true,
        FullscreenMode::Off => false,
    };
    let path = game_dir.join("options.txt");
    crate::fsutil::ensure_dir(game_dir)?;

    let original = fs::read_to_string(&path).unwrap_or_default();
    let mut out: Vec<String> = Vec::with_capacity(original.lines().count() + 1);
    let mut replaced = false;
    for line in original.lines() {
        if let Some((key, _)) = line.split_once(':') {
            if key.trim() == "fullscreen" {
                out.push(format!("fullscreen:{}", if want { "true" } else { "false" }));
                replaced = true;
                continue;
            }
        }
        out.push(line.to_string());
    }
    if !replaced {
        out.push(format!("fullscreen:{}", if want { "true" } else { "false" }));
    }
    let mut text = out.join("\n");
    text.push('\n');
    fs::write(&path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nova-opts-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn read(dir: &Path) -> String {
        fs::read_to_string(dir.join("options.txt")).unwrap()
    }

    #[test]
    fn ignore_does_not_touch_file() {
        let dir = temp_dir();
        fs::write(dir.join("options.txt"), "fullscreen:true\nfov:0.5\n").unwrap();
        apply_fullscreen(&dir, FullscreenMode::Ignore).unwrap();
        assert_eq!(read(&dir), "fullscreen:true\nfov:0.5\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replaces_only_the_fullscreen_line() {
        let dir = temp_dir();
        fs::write(dir.join("options.txt"), "fov:0.5\nfullscreen:false\nlang:zh_cn\n").unwrap();
        apply_fullscreen(&dir, FullscreenMode::On).unwrap();
        let text = read(&dir);
        assert!(text.contains("fullscreen:true"), "{text}");
        // 其他键必须一字不动
        assert!(text.contains("fov:0.5"), "{text}");
        assert!(text.contains("lang:zh_cn"), "{text}");
        assert!(!text.contains("fullscreen:false"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn appends_when_file_missing() {
        let dir = temp_dir();
        apply_fullscreen(&dir, FullscreenMode::Off).unwrap();
        assert_eq!(read(&dir), "fullscreen:false\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn appends_when_key_absent() {
        let dir = temp_dir();
        fs::write(dir.join("options.txt"), "fov:0.5\n").unwrap();
        apply_fullscreen(&dir, FullscreenMode::On).unwrap();
        let text = read(&dir);
        assert!(text.contains("fov:0.5"), "{text}");
        assert!(text.contains("fullscreen:true"), "{text}");
        let _ = fs::remove_dir_all(&dir);
    }
}