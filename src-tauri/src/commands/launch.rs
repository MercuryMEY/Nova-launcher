//! 启动、停止、状态、日志四个方向的命令。
//!
//! 所有阻塞动作（拼参数、解压 natives、探测 Java）都塞进 spawn_blocking，
//! 否则一次冷启动会把窗口卡住一两秒。
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::account::microsoft;
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::java::probe;
use crate::launch::{command, logs, options, proc};
use crate::minecraft::natives::{self, NativesReport};
use crate::state::{AccountKind, AppConfig, AppState};

#[derive(Serialize)]
pub struct LaunchSummary {
    pub version_id: String,
    pub pid: u32,
    pub java: String,
    pub log_path: String,
    pub natives_extracted: usize,
    pub natives_skipped: usize,
    pub natives_missing: usize,
    pub missing_files: usize,
}

#[derive(Serialize)]
pub struct GameStatus {
    pub running: bool,
    pub pid: u32,
    pub version_id: String,
    pub java: String,
    pub started_at: String,
    pub log_path: String,
    pub exit: Option<proc::ExitInfo>,
}

#[derive(Serialize)]
pub struct LogTail {
    pub lines: Vec<String>,
    pub truncated: bool,
}

struct Prep {
    java: PathBuf,
    natives: NativesReport,
}

/// 只做能失败的事：拼参数已在外面完成，这里只解压与选 Java
fn prepare(
    plan: &command::LaunchPlan,
    cfg: &AppConfig,
    root: &std::path::Path,
    app: &AppHandle,
) -> AppResult<Prep> {
    // 顺序很关键：先查 client.jar 与依赖库（这两样不会因为准备动作而变化），
    // 再解压 natives，最后才检查 native 目录。
    // 反过来先查 native 目录，首次启动时它天然是空的，自己就把自己拦住了
    plan.precheck_core()?;

    let _ = app.emit("launch:stage", serde_json::json!({ "stage": "正在准备 native 库" }));
    let version = command::load_version(root, &plan.id)?;
    let report = natives::extract(root, &plan.id, &version)?;
    if !report.missing.is_empty() {
        // 缺包不阻断：先让用户看到游戏输出，再回安装流程补
        log::warn!("缺少 {} 个 native 包", report.missing.len());
    }
    plan.precheck_natives()?;

    // 全屏只能落 options.txt：原版没有这个启动参数
    options::apply_fullscreen(&plan.game_dir, cfg.settings.fullscreen_mode)?;

    let target = plan.java_major;
    let _ = app.emit(
        "launch:stage",
        serde_json::json!({ "stage": format!("正在查找 Java {target}") }),
    );
    let list = probe::detect(cfg.settings.java_path.as_deref(), false);
    let chosen = probe::pick(&list, target).ok_or_else(|| {
        AppError::LaunchFailed(format!(
            "需要 Java {target}，但本机没找到；请到设置 → Java 页下载或手动指定"
        ))
    })?;

    Ok(Prep {
        java: chosen.path,
        natives: report,
    })
}

#[tauri::command]
pub async fn launch_game(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
    // 整合包实例 id：传了就把 gameDir 指到 instances/<id>，
    // 版本（JSON/加载器/库）还是照 id 找
    instance: Option<String>,
) -> AppResult<LaunchSummary> {
    // 单实例：已经跑着就别再拉一个，否则日志与内存都会乱。
    // 这里必须「一进门就抢锁」——真正的进程要等到账户验证、拼参数、解压 native、
    // 探测 Java 之后才创建，中间那几个 await 足够第二次点击再挤进来
    let _guard = crate::launch::LaunchGuard::acquire(&state.launching)?;
    let busy = state
        .game
        .lock()
        .map(|g| g.as_ref().map(|r| r.is_running()).unwrap_or(false))
        .unwrap_or(false);
    if busy {
        return Err(AppError::LaunchFailed(
            "已经有一个游戏在运行了，先停掉它再启动".into(),
        ));
    }

    let cfg = state.snapshot();
    let root = state.minecraft_root();

    // 微软账户的 access token 只有几小时寿命，每次启动都先确认一次；
    // 离线账户不进网络，走占位身份
    let _ = app.emit(
        "launch:stage",
        serde_json::json!({ "stage": "正在验证账户" }),
    );
    let auth = {
        let (cfg, http) = (cfg.clone(), state.http.clone());
        tauri::async_runtime::spawn_blocking(move || -> AppResult<command::AuthTokens> {
            match cfg.current_account() {
                Some(a) if a.kind == AccountKind::Microsoft => {
                    let s = microsoft::ensure_session(&http, &a.id)?;
                    Ok(command::AuthTokens {
                        name: s.name,
                        uuid: s.uuid,
                        access_token: s.access_token,
                        user_type: "msa".into(),
                    })
                }
                _ => Ok(command::AuthTokens::placeholder(&cfg)),
            }
        })
        .await
        .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??
    };

    // 实例 id 会拼进路径，先过一遍白名单；预览/启动都不该接受 `../`
    let instance_dir = match instance {
        Some(inst) => {
            let safe = crate::modpack::mrpack::safe_rel_path(&inst)?;
            if safe.contains('/') {
                return Err(AppError::Message(format!("实例名不合法：{inst}")));
            }
            Some(crate::modpack::install::instance_dir(&root, &safe))
        }
        None => None,
    };
    let plan = {
        let (root, cfg, id) = (root.clone(), cfg.clone(), id.clone());
        tauri::async_runtime::spawn_blocking(move || {
            command::build_plan_with_auth(&root, &cfg, &id, &auth, instance_dir.as_deref())
        })
        .await
            .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??
    };

    let prep = {
        let (plan, cfg, root) = (plan.clone(), cfg.clone(), root.clone());
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || prepare(&plan, &cfg, &root, &app))
            .await
            .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??
    };

    let running = proc::spawn(&plan, &prep.java, &app)?;
    let summary = LaunchSummary {
        version_id: running.info.version_id.clone(),
        pid: running.info.pid,
        java: running.info.java.clone(),
        log_path: running.info.log_path.clone(),
        natives_extracted: prep.natives.extracted,
        natives_skipped: prep.natives.skipped,
        natives_missing: prep.natives.missing.len(),
        missing_files: plan.missing_files.len(),
    };
    *state.game.lock().unwrap_or_else(|e| e.into_inner()) = Some(running);
    Ok(summary)
}

#[tauri::command]
pub fn stop_game(state: State<'_, AppState>) -> AppResult<()> {
    let guard = state.game.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(g) if g.is_running() => g.stop(),
        _ => Ok(()),
    }
}

#[tauri::command]
pub fn game_status(state: State<'_, AppState>) -> Option<GameStatus> {
    let guard = state.game.lock().unwrap_or_else(|e| e.into_inner());
    let running = guard.as_ref()?;
    Some(GameStatus {
        running: running.is_running(),
        pid: running.info.pid,
        version_id: running.info.version_id.clone(),
        java: running.info.java.clone(),
        started_at: running.info.started_at.clone(),
        log_path: running.info.log_path.clone(),
        exit: running.exit_info(),
    })
}

#[tauri::command]
pub fn list_logs() -> Vec<logs::LogFileInfo> {
    logs::list(&fsutil::logs_dir())
}

#[tauri::command]
pub fn read_log(name: String, lines: Option<usize>) -> AppResult<LogTail> {
    let (lines, truncated) = logs::read_tail(
        &fsutil::logs_dir(),
        &name,
        lines.unwrap_or(500).clamp(50, 5000),
    )?;
    Ok(LogTail { lines, truncated })
}

#[tauri::command]
pub fn delete_log(name: String) -> AppResult<()> {
    logs::delete(&fsutil::logs_dir(), &name)
}
