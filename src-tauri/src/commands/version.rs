//! 版本相关命令：远程清单、本地已安装、启动参数预览。
//! 真正的启动在 commands::launch 里；这里只负责“看得到版本、算得对参数”，
//! 而参数装配和启动共用 launch::command::build_plan，不另写一份。
//!
//! 三个命令全部走 async + spawn_blocking：
//! Tauri 的同步命令在 UI 线程执行，拉清单一旦超时就会把窗口卡住。

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::download::mirror::SourceChoice;
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::launch::command::build_plan;
use crate::minecraft::inherit;
use crate::minecraft::isolate;
use crate::minecraft::manifest;

use crate::state::{AppConfig, AppState};

const CACHE_FILE: &str = "version_manifest.json";

#[derive(Serialize)]
pub struct RemoteVersion {
    pub id: String,
    pub kind: String,
    pub release_time: String,
    pub installed: bool,
}

#[derive(Serialize)]
pub struct RemoteVersions {
    pub latest_release: String,
    pub latest_snapshot: String,
    /// official / mirror / cache
    pub source: String,
    pub cached: bool,
    pub versions: Vec<RemoteVersion>,
}

#[derive(Serialize)]
pub struct InstalledVersion {
    pub id: String,
    pub path: String,
    pub java_major: Option<u32>,
    pub has_json: bool,
    pub isolated: bool,
}

#[derive(Serialize)]
pub struct VersionPreview {
    pub id: String,
    pub main_class: String,
    pub java_major: Option<u32>,
    pub asset_index: Option<String>,
    pub game_dir: String,
    pub libraries: usize,
    pub natives: usize,
    pub client_jar_exists: bool,
    pub missing_files: Vec<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

/// 把阻塞 IO 挪到后台线程，回调里拿到的都是自己拥有的数据
async fn run_blocking<F, T>(f: F) -> AppResult<T>
where
    F: FnOnce() -> AppResult<T> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

fn timeout_of(cfg: &AppConfig) -> Duration {
    Duration::from_secs(cfg.settings.mirror_timeout_secs.max(3))
}

fn choice_of(cfg: &AppConfig) -> SourceChoice {
    if cfg.settings.prefer_mirror {
        SourceChoice::MirrorOnly
    } else {
        SourceChoice::OfficialFirst
    }
}

/// 扫本地 versions 目录，失败就返回空列表，不把错误推给界面
pub fn scan_installed(root: &Path) -> Vec<InstalledVersion> {
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(isolate::versions_dir(root)) {
        Ok(e) => e,
        Err(_) => return out,
    };

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let json = entry.path().join(format!("{id}.json"));
        let has_json = json.is_file();

        let mut java_major = None;
        if has_json {
            // 加载器版本自己没写 javaVersion，得沿 inheritsFrom 找父版本
            if let Ok(v) = inherit::load_with_parent(root, &id) {
                java_major = v.java_major();
            }
        }

        out.push(InstalledVersion {
            id,
            path: entry.path().to_string_lossy().into_owned(),
            java_major,
            has_json,
            isolated: false,
        });
    }

    out.sort_by(|a, b| a.id.to_lowercase().cmp(&b.id.to_lowercase()));
    out
}

#[tauri::command]
pub async fn list_remote_versions(state: State<'_, AppState>) -> AppResult<RemoteVersions> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    let http = state.http.clone();
    run_blocking(move || remote_versions_impl(&http, &root, &cfg)).await
}

fn remote_versions_impl(
    client: &reqwest::blocking::Client,
    root: &Path,
    cfg: &AppConfig,
) -> AppResult<RemoteVersions> {
    let cache = fsutil::data_dir().join(CACHE_FILE);

    match manifest::fetch(client, choice_of(cfg), timeout_of(cfg)) {
        Ok((m, url)) => {
            let _ = fsutil::write_json_atomic(&cache, &m);
            Ok(build_remote(m, &url, false, root))
        }
        Err(net_err) => {
            // 断网时退回上次缓存的清单，至少能看到版本列表
            match fsutil::read_json::<manifest::VersionManifest>(&cache) {
                Ok(Some(m)) => Ok(build_remote(m, "cache", true, root)),
                _ => Err(net_err),
            }
        }
    }
}

fn build_remote(
    m: manifest::VersionManifest,
    used_url: &str,
    cached: bool,
    root: &Path,
) -> RemoteVersions {
    let installed: HashSet<String> = scan_installed(root).into_iter().map(|v| v.id).collect();
    let latest_release = m.latest.release.clone();
    let latest_snapshot = m.latest.snapshot.clone();

    let source = if cached {
        "cache"
    } else if used_url.contains("bmclapi") {
        "mirror"
    } else {
        "official"
    }
    .to_string();

    let versions = m
        .versions
        .into_iter()
        .map(|e| RemoteVersion {
            installed: installed.contains(&e.id),
            id: e.id,
            kind: e.kind,
            release_time: e.release_time,
        })
        .collect();

    RemoteVersions {
        latest_release,
        latest_snapshot,
        source,
        cached,
        versions,
    }
}

#[tauri::command]
pub async fn list_installed_versions(state: State<'_, AppState>) -> AppResult<Vec<InstalledVersion>> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    run_blocking(move || Ok::<_, AppError>(installed_impl(&root, &cfg))).await
}

fn installed_impl(root: &Path, cfg: &AppConfig) -> Vec<InstalledVersion> {
    let mut list = scan_installed(root);
    for v in list.iter_mut() {
        let dir = isolate::game_dir(root, &v.id, cfg.settings.isolation);
        v.isolated = dir == isolate::version_dir(root, &v.id);
    }
    list
}

/// 把参数真实组装一遍，但不启动。前端可以拿它做“启动参数”面板和缺文件提醒。
#[tauri::command]
pub async fn preview_version_args(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<VersionPreview> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    run_blocking(move || preview_impl(&root, &cfg, &id)).await
}

fn preview_impl(root: &Path, cfg: &AppConfig, id: &str) -> AppResult<VersionPreview> {
    // 参数装配只有一份：build_plan。预览看到的命令行就是启动时真正跑的那份
    let plan = build_plan(root, cfg, id)?;
    Ok(VersionPreview {
        id: plan.id.clone(),
        main_class: plan.main_class.clone(),
        java_major: plan.java_declared,
        asset_index: Some(plan.asset_index.clone()),
        game_dir: plan.game_dir.to_string_lossy().into_owned(),
        libraries: plan.library_count,
        natives: plan.native_count,
        client_jar_exists: plan.client_jar.is_file(),
        missing_files: plan.missing_files.clone(),
        jvm_args: plan.jvm_args.clone(),
        game_args: plan.game_args.clone(),
    })
}
