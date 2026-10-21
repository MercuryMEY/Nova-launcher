//! Java 相关命令：列出本机 Java、判断某个版本需要的 Java、一键装官方运行时。
//!
//! 探测本身会 spawn 好几个进程，必须走 spawn_blocking。
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::java::{probe, runtime};
use crate::launch::command::{load_version, DEFAULT_JAVA_MAJOR};
use crate::state::AppState;

#[derive(Serialize)]
pub struct JavaEntry {
    pub path: String,
    pub version: String,
    pub major: u32,
    pub arch: String,
    pub source: String,
}

#[derive(Serialize)]
pub struct JavaStatus {
    /// 这个版本要求的 Java 大版本
    pub target: u32,
    /// 版本 JSON 点名的运行时组件，仅作展示
    pub component: Option<String>,
    /// 当前会被选中的那个，None 表示跑不起来
    pub chosen: Option<String>,
    pub detected: Vec<JavaEntry>,
}

#[derive(Serialize)]
pub struct JavaInstallResult {
    pub java: String,
    pub component: String,
    pub major: u32,
}

fn to_entry(j: &probe::JavaInfo) -> JavaEntry {
    JavaEntry {
        path: j.path.to_string_lossy().into_owned(),
        version: j.version.clone(),
        major: j.major,
        arch: j.arch.clone(),
        source: j.source.clone(),
    }
}

#[tauri::command]
pub async fn list_java(state: State<'_, AppState>, force: Option<bool>) -> AppResult<Vec<JavaEntry>> {
    let extra = state.snapshot().settings.java_path;
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let list = probe::detect(extra.as_deref(), force);
        Ok::<_, AppError>(list.iter().map(to_entry).collect())
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

#[tauri::command]
pub async fn java_status(state: State<'_, AppState>, id: String) -> AppResult<JavaStatus> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    tauri::async_runtime::spawn_blocking(move || {
        let version = load_version(&root, &id)?;
        let target = version.java_major().unwrap_or(DEFAULT_JAVA_MAJOR);
        let component = version.java_version.as_ref().and_then(|j| j.component.clone());
        let list = probe::detect(cfg.settings.java_path.as_deref(), false);
        let chosen = probe::pick(&list, target).map(|j| j.path.to_string_lossy().into_owned());
        Ok::<_, AppError>(JavaStatus {
            target,
            component,
            chosen,
            detected: list.iter().map(to_entry).collect(),
        })
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
}

#[tauri::command]
pub async fn install_java_runtime(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> AppResult<JavaInstallResult> {
    let root = state.minecraft_root();
    let http = state.http.clone();

    let result = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            install_impl(&app, &http, &root, &id)
        })
        .await
        .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??
    };

    // 装好之后写回设置：下次探测就会优先命中这份运行时
    let java_path = result.java.clone();
    state.update(|cfg| {
        cfg.settings.java_path = Some(java_path);
        Ok(())
    })?;
    Ok(result)
}

fn install_impl(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    root: &std::path::Path,
    id: &str,
) -> AppResult<JavaInstallResult> {
    let version = load_version(root, id)?;
    let target = version.java_major().unwrap_or(DEFAULT_JAVA_MAJOR);
    let preferred = version.java_version.as_ref().and_then(|j| j.component.clone());
    let platform = runtime::platform_key();

    // 单个文件给 180 秒：官方 jre 里最大的 modules 文件也有几十 MB，
    // 用设置里那个 4 秒的镜像超时会全部失败
    let timeout = Duration::from_secs(180);

    let _ = app.emit("java:progress", serde_json::json!({ "stage": "正在获取官方运行时清单" }));
    let index = runtime::fetch_index(client, timeout)?;
    let component = runtime::choose_component(&index, platform, preferred.as_deref(), target)
        .ok_or_else(|| {
            AppError::Message(format!("官方清单里没有适合本机的 Java {target}（{platform}）"))
        })?;
    let entry = index
        .get(platform)
        .and_then(|table| table.get(&component))
        .ok_or_else(|| AppError::Message("运行时清单结构异常，没有找到对应组件".to_string()))?;

    let mut progress = |stage: &str, done: u64, total: u64| {
        let _ = app.emit(
            "java:progress",
            serde_json::json!({ "stage": stage, "done": done, "total": total }),
        );
    };
    let java = runtime::install(client, root, &component, platform, entry, timeout, &mut progress)?;

    Ok(JavaInstallResult {
        java: java.to_string_lossy().into_owned(),
        component,
        major: target,
    })
}
