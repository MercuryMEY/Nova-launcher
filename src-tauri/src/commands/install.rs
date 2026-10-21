//! 安装相关命令：预览、开始安装、取消。
//!
//! 预览和安装走同一份计划，所以界面上“要下 3200 个文件”
//! 和实际下载数量一定一致。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::install::{executor, plan};
use crate::minecraft::isolate;
use crate::state::AppState;

/// 同时只允许一个安装：两个任务同时往同一个 sha1 文件写会互相踩
static INSTALLING: AtomicBool = AtomicBool::new(false);
/// 当前任务的取消标志，取消命令靠它通知下载线程
static CANCEL_SLOT: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

#[derive(Serialize)]
pub struct InstallPreview {
    pub id: String,
    pub has_client: bool,
    pub libraries: usize,
    pub assets: usize,
    pub has_logging: bool,
    pub total_files: usize,
    pub total_bytes: u64,
    /// missing / partial / complete：不再只看 client.jar
    pub status: String,
    pub missing_count: usize,
    /// 最多 20 个缺失文件名，供界面提示用
    pub missing_names: Vec<String>,
    pub stages: Vec<StageCount>,
}

#[derive(Serialize)]
pub struct StageCount {
    pub stage: String,
    pub files: usize,
}

#[derive(Serialize)]
pub struct InstallSummary {
    pub downloaded: usize,
    pub skipped: usize,
    pub bytes: u64,
    pub seconds: u64,
    pub total_files: usize,
}

/// 不管成功还是报错，都要把“正在安装”的标志放掉，
/// 否则一次失败之后整个启动器再也装不了东西
struct BusyGuard;

impl Drop for BusyGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
        if let Ok(mut slot) = CANCEL_SLOT.lock() {
            *slot = None;
        }
    }
}

#[tauri::command]
pub async fn preview_install(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<InstallPreview> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    let http = state.http.clone();

    tauri::async_runtime::spawn_blocking(move || {
        let (version, _body) = plan::fetch_version_json(&http, &id, &cfg)?;
        // 预览不落盘：asset index 只在真安装时写
        let p = plan::build(&http, &root, &id, &cfg, &version, false)?;
        let (c, l, a) = p.counts();
        let missing = p.missing_required();
        Ok::<_, AppError>(InstallPreview {
            status: missing.status().to_string(),
            missing_count: missing.count(),
            missing_names: missing.names.clone(),
            id: id.clone(),
            has_client: c > 0,
            libraries: l,
            assets: a,
            has_logging: p.logging.is_some(),
            total_files: p.file_count(),
            total_bytes: p.total_bytes(),
            stages: executor::stage_totals(&p)
                .into_iter()
                .map(|(stage, files)| StageCount {
                    stage: stage.to_string(),
                    files,
                })
                .collect(),
        })
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

#[tauri::command]
pub async fn start_install(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> AppResult<InstallSummary> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Message("已经有一个安装任务在跑了".into()));
    }
    let _guard = BusyGuard;

    let cfg = state.snapshot();
    let root = state.minecraft_root();
    let http = state.http.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    if let Ok(mut slot) = CANCEL_SLOT.lock() {
        *slot = Some(cancel.clone());
    }
    let started = Instant::now();
    // 闭包会拿走 app 和 id，这里各留一份给结束后发事件用
    let evt_app = app.clone();
    let evt_id = id.clone();

    let summary = tauri::async_runtime::spawn_blocking(move || {
        let _ = evt_app.emit("install:stage", serde_json::json!({ "stage": "正在获取版本信息" }));
        let (version, body) = plan::fetch_version_json(&http, &id, &cfg)?;
        // 先把 JSON 落盘：就算后面下载失败，版本也能在列表里看见并重试
        plan::save_version_json(&root, &id, &body)?;

        let p = plan::build(&http, &root, &id, &cfg, &version, true)?;
        let total_files = p.file_count();

        // 顺手清掉 7 天前留下的 .part：取消或崩溃攒下来的半成品很占地方
        let week = std::time::Duration::from_secs(7 * 86400);
        let _ = crate::download::file::cleanup_stale_parts(&isolate::libraries_dir(&root), week);
        let _ = crate::download::file::cleanup_stale_parts(&isolate::version_dir(&root, &id), week);

        let a = evt_app.clone();
        let rep = executor::run(&http, &cfg, p, cancel, move |stage, done, total| {
            let _ = a.emit(
                "install:progress",
                serde_json::json!({ "stage": stage, "done": done, "total": total }),
            );
        })?;

        Ok::<_, AppError>(InstallSummary {
            downloaded: rep.downloaded,
            skipped: rep.skipped,
            bytes: rep.bytes,
            seconds: started.elapsed().as_secs(),
            total_files,
        })
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??;

    let _ = app.emit(
        "install:done",
        serde_json::json!({
            "id": evt_id,
            "downloaded": summary.downloaded,
            "skipped": summary.skipped,
            "seconds": summary.seconds,
        }),
    );
    Ok(summary)
}

/// 只置标志，不阻塞：下载线程会在取下一个任务时自行退出
#[tauri::command]
pub fn cancel_install() -> bool {
    match CANCEL_SLOT.lock() {
        Ok(slot) => match slot.as_ref() {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        },
        Err(_) => false,
    }
}

#[tauri::command]
pub fn install_busy() -> bool {
    INSTALLING.load(Ordering::SeqCst)
}
