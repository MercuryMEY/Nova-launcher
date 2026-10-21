use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// 启动器自身的数据目录（配置、日志、缓存）。
/// 注意：与游戏实例目录严格区分，实例目录由 minecraft::isolate 计算。
pub fn data_dir() -> PathBuf {
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("NovaLauncher")
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

/// 默认 .minecraft 位置，按平台区分
pub fn default_minecraft_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = dirs::config_dir() {
            return appdata.join(".minecraft");
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            return home.join("Library/Application Support/minecraft");
        }
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".minecraft")
}

pub fn ensure_dir(path: &Path) -> AppResult<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

/// 安全替换写：写临时文件 -> 旧文件转备份 -> 就位 -> 删备份。
/// 任何一步失败都保留原配置，最坏情况只是多出 .tmp / .bak。
pub fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let data = serde_json::to_vec_pretty(value)?;
    let tmp = path.with_extension("json.tmp");
    let bak = path.with_extension("json.bak");

    // 1. 完整写入临时文件并落盘
    {
        let mut f = File::create(&tmp)?;
        f.write_all(&data)?;
        f.sync_all()?;
    }

    // 2. 旧文件改名成备份（不删除，失败可回滚）
    let had_old = path.exists();
    if had_old {
        let _ = fs::remove_file(&bak);
        fs::rename(path, &bak)?;
    }

    // 3. 临时文件就位，失败则回滚
    match fs::rename(&tmp, path) {
        Ok(()) => {
            if had_old {
                let _ = fs::remove_file(&bak);
            }
            Ok(())
        }
        Err(e) => {
            if had_old {
                let _ = fs::rename(&bak, path);
            }
            let _ = fs::remove_file(&tmp);
            Err(AppError::Io(e.to_string()))
        }
    }
}

pub fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> AppResult<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&text)?))
}

/// 流式计算 SHA1，不把文件整体读入内存
pub fn sha1_file(path: &Path) -> AppResult<String> {
    use sha1::{Digest, Sha1};

    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// 流式计算 SHA512：Modrinth 的整合包索引有时只给这个
///
/// 没有抽成泛型：sha1 与 sha2 各自 re-export 的 `Digest` 是否同一个 crate
/// 版本取决于依赖解析结果，复制十几个字比赌版本一致性划算
pub fn sha512_file(path: &Path) -> AppResult<String> {
    use sha2::{Digest, Sha512};

    let mut file = File::open(path)?;
    let mut hasher = Sha512::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}
