//! 加载器安装执行。
//!
//! 两条完全不同的路：
//! - Fabric / Quilt：官方 meta 直接给一份 profile JSON（带 `inheritsFrom`），
//!   落盘 + 下自己的库 + 把原版 jar 复制一份，就完事了
//! - Forge / NeoForge：必须跑官方 installer（它要执行一堆 processor 产出
//!   混淆映射和 patched jar），我们只负责下安装器、选 Java、转发日志
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::download::file::{self, FileTask};
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::install::plan as install_plan;
use crate::java::probe;
use crate::minecraft::classpath;
use crate::minecraft::inherit;
use crate::minecraft::isolate;
use crate::minecraft::rules::{Features, Platform};
use crate::minecraft::version_json::VersionJson;
use crate::state::AppConfig;

use super::meta::{self, LoaderKind};

/// 安装过程中回传给界面的事件。日志行可能很多，界面自己决定只显示尾巴。
/// 持有 String 而不是 &str：这些枚举要穿过 `dyn FnMut` 边界，
/// 借用版本会把闭包逼成高阶生命周期，编译器在推导上很容易翻车，
/// 而每行日志一次小分配在这个量级（几百行）根本不算成本
pub enum Event {
    Stage(String),
    Progress { done: usize, total: usize },
    Log(String),
}

#[derive(Debug, Clone)]
pub struct InstalledLoader {
    /// 装好后的版本目录名，启动页直接拿它去启动
    pub id: String,
    pub kind: LoaderKind,
    pub version: String,
    pub downloaded: usize,
    pub skipped: usize,
}

fn features() -> Features {
    Features {
        demo: false,
        custom_resolution: true,
        quick_play: false,
    }
}

/// 合并后的版本里所有“能下且有地址”的库，包括 natives 包
/// （natives 也要先落到 libraries 目录，启动时才解压）
fn library_tasks(version: &VersionJson, root: &Path) -> Vec<FileTask> {
    let platform = Platform::current();
    let libs = isolate::libraries_dir(root);
    classpath::collect(version, &platform, &features())
        .into_iter()
        .filter_map(|e| {
            let url = e.url.clone()?;
            Some(FileTask::new(
                url,
                libs.join(&e.path),
                e.sha1.clone().unwrap_or_default(),
                e.size.unwrap_or(0),
            ))
        })
        .collect()
}

fn download_all(
    client: &reqwest::blocking::Client,
    tasks: &[FileTask],
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> AppResult<(usize, usize)> {
    let total = tasks.len();
    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    for (i, task) in tasks.iter().enumerate() {
        if file::already_ok(task) {
            skipped += 1;
        } else {
            // 加载器的库都是小 jar，不给逐字节进度，按文件数报就够
            file::download_file(client, task, timeout, &mut |_, _| {})?;
            downloaded += 1;
        }
        emit(Event::Progress {
            done: i + 1,
            total,
        });
    }
    Ok((downloaded, skipped))
}

fn scan_ids(root: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(entries) = std::fs::read_dir(isolate::versions_dir(root)) else {
        return out;
    };
    for entry in entries.flatten() {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            out.insert(entry.file_name().to_string_lossy().into_owned());
        }
    }
    out
}

fn inherits_of(root: &Path, id: &str) -> Option<String> {
    let path = isolate::json_path(root, id);
    fsutil::read_json::<VersionJson>(&path)
        .ok()
        .flatten()?
        .inherits_from
}

/// 安装器把版本写到哪由它自己决定（`1.20.1-forge-47.2.0` 或 `neoforge-21.1.9`
/// 这类命名都出现过），所以只认“装之前没有、装之后多出来的目录”，
/// 并且优先挑 `inheritsFrom` 指向本体的那个。
fn pick_new_version(root: &Path, game: &str, before: &HashSet<String>) -> Option<String> {
    let after = scan_ids(root);
    let mut candidates: Vec<String> = after.difference(before).cloned().collect();
    candidates.sort();
    if let Some(hit) = candidates
        .iter()
        .find(|id| inherits_of(root, id).as_deref() == Some(game))
    {
        return Some(hit.clone());
    }
    candidates.into_iter().next()
}

pub fn install(
    client: &reqwest::blocking::Client,
    root: &Path,
    cfg: &AppConfig,
    game: &str,
    kind: LoaderKind,
    loader: &str,
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> AppResult<InstalledLoader> {
    // 加载器只是原版外面的一层，原版不在就没什么可装的
    if !isolate::json_path(root, game).is_file() || !isolate::jar_path(root, game).is_file() {
        return Err(AppError::Message(format!(
            "先把 {game} 原版装好，再装 {}",
            kind.label()
        )));
    }
    if kind.needs_installer() {
        install_by_installer(client, root, cfg, game, kind, loader, timeout, emit)
    } else {
        install_by_profile(client, root, cfg, game, kind, loader, timeout, emit)
    }
}

fn install_by_profile(
    client: &reqwest::blocking::Client,
    root: &Path,
    cfg: &AppConfig,
    game: &str,
    kind: LoaderKind,
    loader: &str,
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> AppResult<InstalledLoader> {
    emit(Event::Stage("正在获取加载器配置".into()));
    let (id, body) = meta::profile(client, kind, game, loader, cfg.settings.prefer_mirror, timeout)?;
    // 先落盘再读回来合并：这样“装了哪些库”与实际磁盘状态永远一致
    install_plan::save_version_json(root, &id, &body)?;

    emit(Event::Stage("正在合并原版信息".into()));
    let merged = inherit::load_with_parent(root, &id)?;

    emit(Event::Stage("正在下载加载器依赖".into()));
    let tasks = library_tasks(&merged, root);
    let (downloaded, skipped) = download_all(client, &tasks, timeout, emit)?;

    // 加载器版本目录里要有自己的 client jar（官方启动器也是复制一份）
    emit(Event::Stage("正在准备客户端".into()));
    let src = isolate::jar_path(root, game);
    let dst = isolate::jar_path(root, &id);
    if !dst.is_file() {
        if let Some(dir) = dst.parent() {
            fsutil::ensure_dir(dir)?;
        }
        std::fs::copy(&src, &dst)
            .map_err(|e| AppError::Message(format!("复制客户端 jar 失败：{e}")))?;
    }

    Ok(InstalledLoader {
        id,
        kind,
        version: loader.to_string(),
        downloaded,
        skipped,
    })
}

fn install_by_installer(
    client: &reqwest::blocking::Client,
    root: &Path,
    cfg: &AppConfig,
    game: &str,
    kind: LoaderKind,
    loader: &str,
    timeout: Duration,
    emit: &mut dyn FnMut(Event),
) -> AppResult<InstalledLoader> {
    let major = meta::installer_java_major(game);
    emit(Event::Stage(format!("正在查找 Java {major}")));
    let list = probe::detect(cfg.settings.java_path.as_deref(), false);
    let java = probe::pick(&list, major).ok_or_else(|| {
        AppError::Message(format!(
            "{} 的安装器需要 Java {major}，本机没找到；请到设置 → Java 页下载或手动指定",
            kind.label()
        ))
    })?;

    emit(Event::Stage("正在下载安装器".into()));
    let (url, name) = meta::installer_url(kind, game, loader)?;
    let tmp_dir = fsutil::data_dir().join("tmp");
    fsutil::ensure_dir(&tmp_dir)?;
    let jar = tmp_dir.join(&name);
    file::download_file(client, &FileTask::new(url, jar.clone(), "", 0), timeout, &mut |_, _| {})?;

    emit(Event::Stage("正在运行安装器".into()));
    let before = scan_ids(root);
    let mut cmd = Command::new(&java.path);
    // headless：不带这个，某些平台会尝试弹 Swing 安装界面然后卡住
    cmd.arg("-Djava.awt.headless=true")
        .arg("-jar")
        .arg(&jar)
        // 不传这个会装到用户默认的 .minecraft 里去
        .arg("--installClient")
        .arg(root)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Message(format!("启动安装器失败：{e}")))?;

    // stderr 单独开线程收：安装器失败时异常栈都在这，主线程忙着转发 stdout
    let stderr_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    if let Some(err) = child.stderr.take() {
        let buf = stderr_buf.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                if let Ok(mut v) = buf.lock() {
                    if v.len() < 200 {
                        v.push(line);
                    }
                }
            }
        });
    }

    let mut tail: Vec<String> = Vec::new();
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            emit(Event::Log(line.clone()));
            if tail.len() >= 5 {
                tail.remove(0);
            }
            tail.push(line);
        }
    }

    let status = child
        .wait()
        .map_err(|e| AppError::Message(format!("等待安装器结束时出错：{e}")))?;
    if !status.success() {
        let errs = stderr_buf
            .lock()
            .map(|v| v.join("\n"))
            .unwrap_or_default();
        return Err(AppError::Message(format!(
            "安装器退出码 {}，最后输出：\n{}\n{}",
            status.code().unwrap_or(-1),
            tail.join("\n"),
            errs
        )));
    }

    let id = match pick_new_version(root, game, &before) {
        Some(id) => id,
        None => {
            // 重装同一个版本时目录本来就在，diff 为空，这时按命名规则回退一次
            let guess = meta::loader_id(kind, game, loader);
            if isolate::json_path(root, &guess).is_file() {
                guess
            } else {
                return Err(AppError::Message(
                    "安装器跑完了，但 versions/ 里没找到新版本，看看日志里有没有报错".into(),
                ));
            }
        }
    };

    // 安装器几十 MB，装完就没用了
    let _ = std::fs::remove_file(&jar);

    Ok(InstalledLoader {
        id,
        kind,
        version: loader.to_string(),
        downloaded: 1,
        skipped: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_tasks_skip_entries_without_url() {
        // Fabric profile 里 url 是仓库根，classpath 会拼成完整地址；
        // 完全没有地址的条目直接跳过，交给别处报错
        let v = VersionJson {
            id: "x".into(),
            main_class: None,
            assets: None,
            asset_index: None,
            downloads: None,
            java_version: None,
            libraries: vec![crate::minecraft::version_json::Library {
                name: "org.ow2.asm:asm:9.6".into(),
                downloads: None,
                url: None,
                sha1: None,
                size: None,
                natives: None,
                rules: Vec::new(),
                extract: None,
            }],
            arguments: None,
            minecraft_arguments: None,
            release_time: None,
            kind: None,
            inherits_from: None,
            logging: None,
        };
        let tasks = library_tasks(&v, Path::new("/tmp/nova-test-root"));
        // 没有 url 也没有仓库根 -> 拼不出地址，应当被过滤掉
        assert!(tasks.is_empty());
    }
}