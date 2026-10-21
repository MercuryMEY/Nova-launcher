//! 拉起游戏进程、收输出、看退出。
//!
//! 设计取舍：子进程句柄放在 `Arc<Mutex<Child>>` 里，
//! stdout/stderr 各开一个读线程，另有一个看门狗线程轮询 try_wait。
//! 这样「停止游戏」和「检测退出」不需要抢同一个阻塞式 wait()。
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::launch::command::LaunchPlan;

#[derive(Debug, Clone, Serialize)]
pub struct RunningInfo {
    pub version_id: String,
    pub pid: u32,
    pub java: String,
    pub started_at: String,
    pub log_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub success: bool,
    pub seconds: f64,
}

pub struct RunningGame {
    pub info: RunningInfo,
    child: Arc<Mutex<Child>>,
    exit: Arc<Mutex<Option<ExitInfo>>>,
}

impl RunningGame {
    pub fn is_running(&self) -> bool {
        self.exit
            .lock()
            .map(|slot| slot.is_none())
            .unwrap_or(false)
    }

    pub fn exit_info(&self) -> Option<ExitInfo> {
        self.exit.lock().ok().and_then(|s| s.clone())
    }

    /// kill 之后最多等 2 秒确认进程真的消失，不等就明确报错
    pub fn stop(&self) -> AppResult<()> {
        if !self.is_running() {
            return Ok(());
        }
        {
            let mut guard = self
                .child
                .lock()
                .map_err(|_| AppError::LaunchFailed("进程状态锁失效".into()))?;
            guard
                .kill()
                .map_err(|e| AppError::LaunchFailed(format!("结束进程失败：{e}")))?;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(80));
            let gone = match self.child.lock() {
                Ok(mut g) => matches!(g.try_wait(), Ok(Some(_))),
                Err(_) => return Ok(()),
            };
            if gone {
                return Ok(());
            }
        }
        Err(AppError::LaunchFailed(
            "进程两秒内没有退出，可能被系统保护了".into(),
        ))
    }
}

/// 启动游戏。日志文件名带时间戳，历史日志不会被覆盖。
pub fn spawn(plan: &LaunchPlan, java: &Path, app: &AppHandle) -> AppResult<RunningGame> {
    // 走到 spawn 时 natives 已经解压过了，core 与 natives 两条都可以查
    plan.precheck_core()?;
    plan.precheck_natives()?;
    fsutil::ensure_dir(&plan.game_dir)?;
    fsutil::ensure_dir(&plan.natives_dir)?;

    let logs = fsutil::logs_dir();
    fsutil::ensure_dir(&logs)?;
    let name = format!(
        "{}-{}.log",
        plan.id,
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let log_path: PathBuf = logs.join(name);
    let mut log = File::create(&log_path)?;

    let argv = plan.command_args();
    writeln!(log, "==== Nova Launcher {} ====", env!("CARGO_PKG_VERSION"))?;
    writeln!(log, "版本：{}", plan.id)?;
    writeln!(log, "Java：{}", java.display())?;
    writeln!(log, "工作目录：{}", plan.game_dir.display())?;
    writeln!(log, "命令行：{} {}", java.display(), argv.join(" "))?;
    writeln!(log, "==== 游戏输出 ====" )?;
    log.flush()?;

    let mut cmd = Command::new(java);
    cmd.args(&argv)
        .current_dir(&plan.game_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：不在桌面弹控制台黑框，游戏自己的窗口不受影响
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::LaunchFailed(format!("无法启动 Java 进程：{e}")))?;
    let pid = child.id();

    let log = Arc::new(Mutex::new(log));
    if let Some(out) = child.stdout.take() {
        read_stream(out, log.clone(), plan.id.clone(), app.clone(), "stdout");
    }
    if let Some(err) = child.stderr.take() {
        read_stream(err, log.clone(), plan.id.clone(), app.clone(), "stderr");
    }

    let child = Arc::new(Mutex::new(child));
    let exit: Arc<Mutex<Option<ExitInfo>>> = Arc::new(Mutex::new(None));
    let info = RunningInfo {
        version_id: plan.id.clone(),
        pid,
        java: java.to_string_lossy().into_owned(),
        started_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        log_path: log_path.to_string_lossy().into_owned(),
    };

    {
        let child = child.clone();
        let exit = exit.clone();
        let app = app.clone();
        let log = log.clone();
        let id = plan.id.clone();
        let log_path = log_path.clone();
        let started = Instant::now();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(400));
            let status = match child.lock() {
                Ok(mut guard) => guard.try_wait().ok().flatten(),
                Err(_) => return,
            };
            let Some(status) = status else { continue };
            let seconds = started.elapsed().as_secs_f64();
            let done = ExitInfo {
                code: status.code(),
                success: status.success(),
                seconds,
            };
            if let Ok(mut f) = log.lock() {
                let _ = writeln!(
                    f,
                    "==== 游戏已退出（退出码 {:?}，{:.1} 秒）====",
                    done.code, seconds
                );
                let _ = f.flush();
            }
            if let Ok(mut slot) = exit.lock() {
                *slot = Some(done.clone());
            }
            let _ = app.emit(
                "game:exit",
                serde_json::json!({
                    "id": id.clone(),
                    "code": done.code,
                    "success": done.success,
                    "seconds": done.seconds,
                    "log_path": log_path.to_string_lossy().into_owned(),
                }),
            );
            return;
        });
    }

    Ok(RunningGame { info, child, exit })
}

/// 每行都写文件 + 推事件，界面不用轮询文件就能实时滚动
fn read_stream<R: Read + Send + 'static>(
    src: R,
    log: Arc<Mutex<File>>,
    id: String,
    app: AppHandle,
    stream: &'static str,
) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(src);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let text = line.trim_end_matches(&['\r', '\n'][..]).to_string();
                    if let Ok(mut f) = log.lock() {
                        let _ = writeln!(f, "{text}");
                        let _ = f.flush();
                    }
                    let _ = app.emit(
                        "game:log",
                        // id 在循环里要反复用，必须 clone；text 每次是新字符串不用
                        serde_json::json!({ "id": id.clone(), "line": text, "stream": stream }),
                    );
                }
                Err(_) => break,
            }
        }
    });
}
