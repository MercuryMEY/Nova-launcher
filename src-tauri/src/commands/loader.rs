//! 加载器相关命令：四个加载器的版本列表、安装、已装清单。
//!
//! 安装是重活（下几十个 jar，Forge 还要跑一个 JVM 子进程），
//! 所以整体塞进 spawn_blocking，进度与日志用事件往回推。
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::loader::{install as loader_install, meta, LoaderKind};
use crate::minecraft::isolate;
use crate::minecraft::version_json::VersionJson;
use crate::state::{AppConfig, AppState};

/// 同一时间只允许一个加载器安装：两个安装器同时往 libraries 里写，谁也装不明白
static INSTALLING: AtomicBool = AtomicBool::new(false);

/// 失败也要把标志放掉，否则一次报错之后整个启动器再也装不了加载器
struct BusyGuard;

impl Drop for BusyGuard {
    fn drop(&mut self) {
        INSTALLING.store(false, Ordering::SeqCst);
    }
}

#[derive(Serialize)]
pub struct KindInfo {
    pub key: String,
    pub label: String,
    /// 界面上据此提示“要跑安装器，可能需要等一会儿”
    pub needs_installer: bool,
    pub note: String,
}

#[derive(Serialize)]
pub struct LoaderSummary {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub downloaded: usize,
    pub skipped: usize,
    pub seconds: u64,
}

#[derive(Serialize)]
pub struct InstalledLoaderView {
    pub id: String,
    /// 从目录名认出来的加载器类型；认不出就是 None
    pub kind: Option<String>,
    pub loader_version: Option<String>,
}

/// 元数据接口不用给太长超时：它是纯文本，慢就是网络不通
fn meta_timeout(cfg: &AppConfig) -> Duration {
    Duration::from_secs(cfg.settings.mirror_timeout_secs.max(3).saturating_mul(3).max(30))
}

#[tauri::command]
pub fn loader_kinds() -> Vec<KindInfo> {
    [
        (LoaderKind::Fabric, "轻量、更新快，绝大多数 Mod 都支持"),
        (LoaderKind::Forge, "老牌加载器，整合包生态最全"),
        (LoaderKind::NeoForge, "Forge 分叉，1.20.2 之后的新选择"),
        (LoaderKind::Quilt, "Fabric 分叉，兼容部分 Fabric Mod"),
    ]
    .into_iter()
    .map(|(k, note)| KindInfo {
        key: k.key().to_string(),
        label: k.label().to_string(),
        needs_installer: k.needs_installer(),
        note: note.to_string(),
    })
    .collect()
}

#[tauri::command]
pub async fn loader_versions(
    state: State<'_, AppState>,
    game: String,
    kind: String,
) -> AppResult<Vec<meta::LoaderVersion>> {
    let k = LoaderKind::parse(&kind)?;
    let cfg = state.snapshot();
    let http = state.http.clone();
    let timeout = meta_timeout(&cfg);
    tauri::async_runtime::spawn_blocking(move || {
        meta::list(&http, k, &game, cfg.settings.prefer_mirror, timeout)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

#[tauri::command]
pub async fn loader_install(
    state: State<'_, AppState>,
    app: AppHandle,
    game: String,
    kind: String,
    version: String,
) -> AppResult<LoaderSummary> {
    let k = LoaderKind::parse(&kind)?;
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Message("已经有一个加载器安装在跑了".into()));
    }
    let _guard = BusyGuard;

    let cfg = state.snapshot();
    let root = state.minecraft_root();
    let http = state.http.clone();
    // 安装器本身几十 MB，用安装流程那套放宽后的超时
    let timeout = crate::install::plan::timeout_of(&cfg);
    let started = Instant::now();

    let evt_app = app.clone();
    let game_for_task = game.clone();
    let summary = tauri::async_runtime::spawn_blocking(move || {
        let sink_app = evt_app.clone();
        let mut emit = move |ev: loader_install::Event| match ev {
            loader_install::Event::Stage(stage) => {
                let _ = sink_app.emit("loader:stage", serde_json::json!({ "stage": stage }));
            }
            loader_install::Event::Progress { done, total } => {
                let _ = sink_app.emit(
                    "loader:progress",
                    serde_json::json!({ "done": done, "total": total }),
                );
            }
            loader_install::Event::Log(line) => {
                let _ = sink_app.emit("loader:log", serde_json::json!({ "line": line }));
            }
        };
        let done = loader_install::install(
            &http,
            &root,
            &cfg,
            &game_for_task,
            k,
            &version,
            timeout,
            &mut emit,
        )?;
        Ok::<_, AppError>(done)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??;

    let reply = LoaderSummary {
        id: summary.id.clone(),
        kind: k.key().to_string(),
        version: summary.version.clone(),
        downloaded: summary.downloaded,
        skipped: summary.skipped,
        seconds: started.elapsed().as_secs(),
    };
    let _ = app.emit(
        "loader:done",
        serde_json::json!({
            "id": reply.id,
            "kind": reply.kind,
            "version": reply.version,
            "downloaded": reply.downloaded,
            "skipped": reply.skipped,
            "seconds": reply.seconds,
        }),
    );
    Ok(reply)
}

/// 这个原版版本下已经装了哪些加载器（看目录 + inheritsFrom）
#[tauri::command]
pub fn loader_installed(state: State<'_, AppState>, game: String) -> Vec<InstalledLoaderView> {
    let root = state.minecraft_root();
    installed_impl(&root, &game)
}

fn installed_impl(root: &Path, game: &str) -> Vec<InstalledLoaderView> {
    let mut out: Vec<InstalledLoaderView> = Vec::new();
    let Ok(entries) = std::fs::read_dir(isolate::versions_dir(root)) else {
        return out;
    };
    let mut seen: HashSet<String> = HashSet::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        if !seen.insert(id.clone()) {
            continue;
        }
        let json = isolate::json_path(root, &id);
        let Some(v) = fsutil::read_json::<VersionJson>(&json).ok().flatten() else {
            continue;
        };
        if v.inherits_from.as_deref() != Some(game) {
            continue;
        }
        let kind = kind_of(&id);
        // 版本名里那串数字就是加载器版本，认得出类型才拆
        let loader_version = kind.as_deref().and_then(|_| pick_version(&id, game));
        out.push(InstalledLoaderView {
            id,
            kind,
            loader_version,
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn kind_of(id: &str) -> Option<String> {
    let lower = id.to_ascii_lowercase();
    if lower.contains("fabric-loader") {
        Some("fabric".into())
    } else if lower.contains("quilt-loader") {
        Some("quilt".into())
    } else if lower.contains("neoforge") {
        Some("neoforge".into())
    } else if lower.contains("forge") {
        Some("forge".into())
    } else {
        None
    }
}

/// `fabric-loader-0.16.5-1.21.4` → `0.16.5`；`1.20.1-forge-47.2.0` → `47.2.0`
fn pick_version(id: &str, game: &str) -> Option<String> {
    if let Some(rest) = id.strip_prefix("fabric-loader-") {
        return rest.strip_suffix(&format!("-{game}")).map(|s| s.to_string());
    }
    if let Some(rest) = id.strip_prefix("quilt-loader-") {
        return rest.strip_suffix(&format!("-{game}")).map(|s| s.to_string());
    }
    for tag in ["-forge-", "-neoforge-"] {
        if let Some((_, ver)) = id.split_once(tag) {
            return Some(ver.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_detection() {
        assert_eq!(kind_of("fabric-loader-0.16.5-1.21.4").as_deref(), Some("fabric"));
        assert_eq!(kind_of("quilt-loader-0.26.4-1.21.4").as_deref(), Some("quilt"));
        assert_eq!(kind_of("1.21.1-neoforge-21.1.9").as_deref(), Some("neoforge"));
        assert_eq!(kind_of("1.20.1-forge-47.2.0").as_deref(), Some("forge"));
        // 原版自己不该被当成加载器
        assert_eq!(kind_of("1.20.1"), None);
    }

    #[test]
    fn loader_version_extraction() {
        assert_eq!(
            pick_version("fabric-loader-0.16.5-1.21.4", "1.21.4").as_deref(),
            Some("0.16.5")
        );
        assert_eq!(
            pick_version("1.20.1-forge-47.2.0", "1.20.1").as_deref(),
            Some("47.2.0")
        );
        assert_eq!(
            pick_version("1.21.1-neoforge-21.1.9", "1.21.1").as_deref(),
            Some("21.1.9")
        );
    }
}
