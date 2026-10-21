//! 整合包命令：预览、导入、列表、删除。
//!
//! 导入是个重活（解开 zip + 下载几十个 Mod + 可能还要跑加载器安装器），
//! 一律扔进 `spawn_blocking`，进度靠事件回传，前端只负责画。
use std::path::Path;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::minecraft::isolate;
use crate::modpack::{install as pack, mrpack};
use crate::state::AppState;

/// 预览卡上要显示的东西。`existing` 为 true 说明已经有一个同名实例了，
/// 真正导入时会自动变成 `名字-2`
#[derive(Serialize)]
pub struct PackPreview {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub game: String,
    pub loader: Option<String>,
    pub loader_label: Option<String>,
    pub loader_version: Option<String>,
    pub file_count: usize,
    pub total_size: u64,
    pub dir: String,
    pub existing: bool,
    /// 原版装没装：没装的话加载器这一步会被跳过
    pub base_installed: bool,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub instance: pack::Instance,
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub overrides: usize,
    /// 加载器装成了才有的启动版本 id
    pub launch_id: Option<String>,
    pub seconds: u64,
}

#[tauri::command]
pub fn modpack_inspect(state: State<'_, AppState>, path: String) -> AppResult<PackPreview> {
    let root = state.minecraft_root();
    let plan = pack::plan(&root, Path::new(&path))?;
    // id 被加了 `-2` 这类后缀，说明同名实例已经存在，这次会另存一份
    let existing = plan.id != plan.base_id;
    Ok(PackPreview {
        id: plan.id.clone(),
        name: plan.name.clone(),
        summary: plan.summary.clone(),
        game: plan.game.clone(),
        loader: plan.loader.as_ref().map(|(k, _)| k.key().to_string()),
        loader_label: plan.loader.as_ref().map(|(k, _)| k.label().to_string()),
        loader_version: plan.loader.as_ref().map(|(_, v)| v.clone()),
        file_count: plan.files.len(),
        total_size: plan.total,
        dir: plan.dir.to_string_lossy().into_owned(),
        existing,
        base_installed: base_ready(&root, &plan.game),
    })
}

fn base_ready(root: &Path, game: &str) -> bool {
    isolate::json_path(root, game).is_file() && isolate::jar_path(root, game).is_file()
}

/// 导入。`install_loader` 为 false 时只铺文件，加载器留给下载页手动装
#[tauri::command]
pub async fn modpack_import(
    state: State<'_, AppState>,
    app: AppHandle,
    path: String,
    install_loader: bool,
) -> AppResult<ImportResult> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    let http = state.http.clone();
    let timeout = crate::install::plan::timeout_of(&cfg);
    let started = Instant::now();

    // 预览阶段已经校验过 zip 与索引，这里再算一次，保证 id 与预览一致
    let plan = pack::plan(&root, Path::new(&path))?;
    let plan_for_loader = plan.clone();

    let stage_app = app.clone();
    let log_app = app.clone();
    let progress_app = app.clone();

    let report = {
        let (http, root, task_plan) = (http.clone(), root.clone(), plan);
        tauri::async_runtime::spawn_blocking(move || {
            let mut emit = |ev: pack::Event| match ev {
                pack::Event::Stage(stage) => {
                    let _ = stage_app.emit("modpack:stage", serde_json::json!({ "stage": stage }));
                }
                pack::Event::Progress { done, total } => {
                    let _ = progress_app.emit(
                        "modpack:progress",
                        serde_json::json!({ "done": done, "total": total }),
                    );
                }
                pack::Event::Log(line) => {
                    let _ = log_app.emit("modpack:log", serde_json::json!({ "line": line }));
                }
            };
            pack::import(&http, &root, &task_plan, timeout, &mut emit)
        })
        .await
        .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))?
    }?;

    let mut launch_id: Option<String> = None;
    if let Some((kind, version)) = plan_for_loader.loader.clone() {
        let game = plan_for_loader.game.clone();
        if !install_loader {
            let _ = app.emit(
                "modpack:log",
                serde_json::json!({ "line": "按你的选择跳过加载器安装" }),
            );
        } else if !base_ready(&root, &game) {
            // 原版不在就没法装加载器；整合包本身已经落盘了，别把这一步当失败
            let _ = app.emit(
                "modpack:log",
                serde_json::json!({
                    "line": format!("{game} 原版还没装，加载器先跳过；装好原版后去下载页补装")
                }),
            );
        } else {
            let _ = app.emit(
                "modpack:stage",
                serde_json::json!({ "stage": format!("正在安装 {} {}", kind.label(), version) }),
            );
            let (http, root, cfg, app2, kind_label) = (
                http.clone(),
                root.clone(),
                cfg.clone(),
                app.clone(),
                kind.label(),
            );
            let stage_app = app.clone();
            let progress_app = app.clone();
            let log_app = app.clone();
            let installed = {
                let version = version.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let mut emit = |ev: crate::loader::install::Event| match ev {
                        crate::loader::install::Event::Stage(stage) => {
                            let _ = stage_app.emit(
                                "modpack:stage",
                                serde_json::json!({ "stage": stage }),
                            );
                        }
                        crate::loader::install::Event::Progress { done, total } => {
                            let _ = progress_app.emit(
                                "modpack:progress",
                                serde_json::json!({ "done": done as u64, "total": total as u64 }),
                            );
                        }
                        crate::loader::install::Event::Log(line) => {
                            let _ = log_app
                                .emit("modpack:log", serde_json::json!({ "line": line }));
                        }
                    };
                    crate::loader::install::install(
                        &http, &root, &cfg, &game, kind, &version, timeout, &mut emit,
                    )
                })
                .await
                .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))
            };
            match installed {
                Ok(Ok(done)) => {
                    launch_id = Some(done.id.clone());
                    let _ = app2.emit(
                        "modpack:log",
                        serde_json::json!({
                            "line": format!("{} {} 装好了：{}", kind_label, version, done.id)
                        }),
                    );
                }
                // 加载器失败不该让整包导入作废：文件已经在盘上了，重试一次加载器即可
                Ok(Err(e)) => {
                    let _ = app.emit(
                        "modpack:log",
                        serde_json::json!({ "line": format!("加载器没装上：{e}") }),
                    );
                }
                Err(e) => {
                    let _ = app.emit("modpack:log", serde_json::json!({ "line": format!("{e}") }));
                }
            }
        }
    }

    if let Some(id) = &launch_id {
        pack::set_launch_id(&root, &report.instance.id, id)?;
    }
    let instance = pack::read_instance(&root, &report.instance.id).unwrap_or(report.instance);

    let result = ImportResult {
        instance,
        downloaded: report.downloaded,
        skipped: report.skipped,
        failed: report.failed.len(),
        overrides: report.overrides,
        launch_id: launch_id.clone(),
        seconds: started.elapsed().as_secs(),
    };
    let _ = app.emit("modpack:done", serde_json::json!({ "id": result.instance.id }));
    Ok(result)
}

/// 只补上次没下下来的文件：不重解压 overrides，也不重装加载器
#[tauri::command]
pub async fn modpack_retry(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> AppResult<pack::Instance> {
    let cfg = state.snapshot();
    let root = state.minecraft_root();
    let http = state.http.clone();
    let timeout = crate::install::plan::timeout_of(&cfg);

    let stage_app = app.clone();
    let log_app = app.clone();
    let progress_app = app.clone();

    let done = tauri::async_runtime::spawn_blocking(move || {
        let mut emit = |ev: pack::Event| match ev {
            pack::Event::Stage(stage) => {
                let _ = stage_app.emit("modpack:stage", serde_json::json!({ "stage": stage }));
            }
            pack::Event::Progress { done, total } => {
                let _ = progress_app
                    .emit("modpack:progress", serde_json::json!({ "done": done, "total": total }));
            }
            pack::Event::Log(line) => {
                let _ = log_app.emit("modpack:log", serde_json::json!({ "line": line }));
            }
        };
        pack::retry(&http, &root, &id, timeout, &mut emit)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??;

    let _ = app.emit("modpack:done", serde_json::json!({ "id": done.id }));
    Ok(done)
}

#[tauri::command]
pub fn modpack_list(state: State<'_, AppState>) -> Vec<pack::Instance> {
    let root = state.minecraft_root();
    pack::list(&root)
}

#[tauri::command]
pub fn modpack_remove(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let root = state.minecraft_root();
    pack::remove(&root, &id)
}

/// 索引格式版本，前端提示「这个包太新」时用得上
#[tauri::command]
pub fn modpack_format() -> u32 {
    mrpack::SUPPORTED_FORMAT
}