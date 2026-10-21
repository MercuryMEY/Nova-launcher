//! Mod 命令：搜索 Modrinth、列出本地 Mod、安装 / 启停 / 删除。
//!
//! 搜索与下载都走 spawn_blocking：这两个都是网络活，
//! 放在 Tauri 的命令线程上会把窗口卡住。
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::mods::{install as files, local, modrinth};
use crate::state::{AppConfig, AppState};

/// 前端挑好某个版本的文件后回传的最小信息。
/// 不传整个 ModVersion：多传的字段后端一个都用不上，还得防着被改。
#[derive(Deserialize)]
pub struct FileRef {
    pub filename: String,
    pub url: String,
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Serialize)]
pub struct InstallResult {
    pub path: String,
    /// true 表示同名校验通过，没有真下载
    pub skipped: bool,
    /// 界面顺便提示一句“装到哪了”
    pub dir: String,
}

/// 版本号会直接拼进路径，和文件名一样要过白板
fn safe_version_id(id: &str) -> AppResult<String> {
    let id = id.trim();
    if id.is_empty() {
        return Err(AppError::Message("先选一个游戏版本".into()));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(AppError::Message(format!("版本名不合法：{id}")));
    }
    Ok(id.to_string())
}

/// 整合包实例名同样要过白板：它也会拼进路径
fn safe_instance_id(id: &str) -> AppResult<String> {
    let id = id.trim();
    if id.is_empty() {
        return Err(AppError::Message("先选一个整合包实例".into()));
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(AppError::Message(format!("实例名不合法：{id}")));
    }
    Ok(id.to_string())
}

/// 当前要管的 Mod 目录
///
/// 不给实例时跟着游戏版本 + 隔离模式走（`versions/<id>/mods` 或
/// `game/mods`）；给了实例就固定落在 `instances/<id>/mods`，
/// 和整合包启动时用的 gameDir 是同一个目录
fn mods_path(state: &AppState, game: &str, instance: Option<&str>) -> AppResult<PathBuf> {
    let root = state.minecraft_root();
    if let Some(ins) = instance {
        let ins = safe_instance_id(ins)?;
        return Ok(crate::modpack::install::instance_dir(&root, &ins).join("mods"));
    }
    let id = safe_version_id(game)?;
    let cfg = state.snapshot();
    Ok(local::mods_dir(&root, &id, cfg.settings.isolation))
}

/// 元数据接口：纯文本，慢就是网络不通
fn meta_timeout(cfg: &AppConfig) -> Duration {
    Duration::from_secs(cfg.settings.mirror_timeout_secs.max(3).saturating_mul(3).max(30))
}

#[tauri::command]
pub async fn mods_search(
    state: State<'_, AppState>,
    game: String,
    loader: String,
    query: String,
    sort: String,
    offset: u32,
    limit: u32,
) -> AppResult<modrinth::SearchPage> {
    let cfg = state.snapshot();
    let http = state.http.clone();
    let timeout = meta_timeout(&cfg);
    tauri::async_runtime::spawn_blocking(move || {
        modrinth::search(&http, &game, &loader, &query, &sort, offset, limit, timeout)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

#[tauri::command]
pub async fn mods_versions(
    state: State<'_, AppState>,
    project_id: String,
    game: String,
    loader: String,
) -> AppResult<Vec<modrinth::ModVersion>> {
    let cfg = state.snapshot();
    let http = state.http.clone();
    let timeout = meta_timeout(&cfg);
    tauri::async_runtime::spawn_blocking(move || {
        modrinth::versions(&http, &project_id, &game, &loader, timeout)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

/// 下载一个 Mod 到当前版本的 mods 目录
#[tauri::command]
pub async fn mods_install(
    state: State<'_, AppState>,
    game: String,
    instance: Option<String>,
    file: FileRef,
) -> AppResult<InstallResult> {
    let dir = mods_path(&state, &game, instance.as_deref())?;
    let cfg = state.snapshot();
    let http = state.http.clone();
    // Mod 文件可以很大（光影、资源类模组），用安装流程那套放宽的超时
    let timeout = crate::install::plan::timeout_of(&cfg);
    let dir_for_task = dir.clone();
    let target = crate::mods::modrinth::ModFile {
        filename: file.filename,
        url: file.url,
        sha1: file.sha1,
        size: file.size,
    };
    let joined = tauri::async_runtime::spawn_blocking(move || {
        files::install(&http, &dir_for_task, &target, timeout)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?;
    // join 出来的还是 Result：一层是线程 panic，一层是下载失败，这里再拆一次
    let done = joined?;
    Ok(InstallResult {
        path: done.path.to_string_lossy().into_owned(),
        skipped: done.skipped,
        dir: dir.to_string_lossy().into_owned(),
    })
}

/// 本地已装的 Mod 列表（读 jar 里的元数据，读不到就给文件名）
#[tauri::command]
pub fn mods_list(
    state: State<'_, AppState>,
    game: String,
    instance: Option<String>,
) -> AppResult<Vec<local::LocalMod>> {
    let dir = mods_path(&state, &game, instance.as_deref())?;
    Ok(local::scan(&dir))
}

/// mods 目录路径，界面上要显示“装到这里”，也方便用户自己往里扔 jar
#[tauri::command]
pub fn mods_dir_of(
    state: State<'_, AppState>,
    game: String,
    instance: Option<String>,
) -> AppResult<String> {
    Ok(mods_path(&state, &game, instance.as_deref())?
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub fn mods_toggle(
    state: State<'_, AppState>,
    game: String,
    instance: Option<String>,
    file_name: String,
    enable: bool,
) -> AppResult<String> {
    let dir = mods_path(&state, &game, instance.as_deref())?;
    files::toggle(&dir, &file_name, enable)
}

#[tauri::command]
pub fn mods_remove(
    state: State<'_, AppState>,
    game: String,
    instance: Option<String>,
    file_name: String,
) -> AppResult<()> {
    let dir = mods_path(&state, &game, instance.as_deref())?;
    files::remove(&dir, &file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_id_guard() {
        assert!(safe_version_id("1.20.1").is_ok());
        assert!(safe_version_id("fabric-loader-0.16.5-1.21.4").is_ok());
        assert!(safe_version_id("").is_err());
        assert!(safe_version_id("../etc").is_err());
        assert!(safe_version_id("a/b").is_err());
        assert!(safe_version_id("a\\b").is_err());
    }

    #[test]
    fn instance_id_guard() {
        assert!(safe_instance_id("prominence").is_ok());
        assert!(safe_instance_id("  空格名  ").is_ok());
        assert!(safe_instance_id("").is_err());
        assert!(safe_instance_id("..").is_err());
        assert!(safe_instance_id("a/../b").is_err());
        assert!(safe_instance_id("a\\b").is_err());
    }
}
