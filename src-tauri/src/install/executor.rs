//! 按计划分阶段下载。
//!
//! 顺序是 client → libraries → 日志配置 → assets：
//! assets 动辄几千个文件，放最后，用户能先看到一个“已经装好了”的进度条在动。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::download::file;
use crate::download::pool;
use crate::download::file::FileTask;
use crate::error::{AppError, AppResult};
use crate::install::plan::{self, Planned};
use crate::state::AppConfig;

/// 界面直接把这些当文案显示，所以写在常量里保持一处
pub const STAGE_CLIENT: &str = "正在下载客户端";
pub const STAGE_LIBS: &str = "正在下载依赖库";
pub const STAGE_CONFIG: &str = "正在下载日志配置";
pub const STAGE_ASSETS: &str = "正在下载资源文件";

/// 4 线程：官方 CDN 单线程很难吃满带宽，再往上加也快不了多少，
/// 反而会因资源文件太小、连接建立开销占比过高而变慢
const WORKERS: usize = 4;

#[derive(Debug, Default, Clone)]
pub struct Report {
    pub downloaded: usize,
    pub skipped: usize,
    pub bytes: u64,
}

fn cancelled() -> AppError {
    AppError::Message("安装已取消".into())
}

pub fn run<P>(
    client: &reqwest::blocking::Client,
    cfg: &AppConfig,
    planned: Planned,
    cancel: Arc<AtomicBool>,
    on_progress: P,
) -> AppResult<Report>
where
    P: FnMut(&'static str, usize, usize) + Send + 'static,
{
    let timeout = plan::timeout_of(cfg);
    let cb = Arc::new(Mutex::new(on_progress));
    let mut report = Report::default();

    // client.jar 是单个几十 MB 的文件，扔进池子只会白写一套线程调度
    if let Some(task) = planned.client {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        if file::already_ok(&task) {
            report.skipped += 1;
        } else {
            let c = cb.clone();
            file::download_file(client, &task, timeout, &mut |done, total| {
                if let Ok(mut f) = c.lock() {
                    f(STAGE_CLIENT, done as usize, total as usize);
                }
            })?;
            report.downloaded += 1;
            report.bytes = report.bytes.saturating_add(task.size);
        }
    }

    let config_task: Vec<FileTask> = planned.logging.into_iter().collect();
    let stages: [(&'static str, Vec<FileTask>); 3] = [
        (STAGE_LIBS, planned.libraries),
        (STAGE_CONFIG, config_task),
        (STAGE_ASSETS, planned.assets),
    ];

    for (stage, tasks) in stages {
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let c = cb.clone();
        let part = pool::run(
            client,
            stage,
            tasks,
            WORKERS,
            timeout,
            cancel.clone(),
            move |s, done, total| {
                if let Ok(mut f) = c.lock() {
                    f(s, done, total);
                }
            },
        )?;
        report.downloaded += part.downloaded;
        report.skipped += part.skipped;
        report.bytes = report.bytes.saturating_add(part.bytes);
    }

    Ok(report)
}

/// 每阶段的文件数，界面上用来画“多少 / 多少”
pub fn stage_totals(p: &Planned) -> Vec<(&'static str, usize)> {
    vec![
        (STAGE_CLIENT, if p.client.is_some() { 1 } else { 0 }),
        (STAGE_LIBS, p.libraries.len()),
        (STAGE_CONFIG, if p.logging.is_some() { 1 } else { 0 }),
        (STAGE_ASSETS, p.assets.len()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_plan_finishes_without_network() {
        let client = reqwest::blocking::Client::new();
        let cfg = AppConfig::default();
        let cancel = Arc::new(AtomicBool::new(false));
        let r = run(&client, &cfg, Planned::default(), cancel, |_, _, _| {}).unwrap();
        assert_eq!(r.downloaded, 0);
        assert_eq!(r.bytes, 0);
    }

    #[test]
    fn cancelled_before_start() {
        let client = reqwest::blocking::Client::new();
        let cfg = AppConfig::default();
        let cancel = Arc::new(AtomicBool::new(true));
        let mut p = Planned::default();
        p.libraries.push(FileTask::new("http://x/y.jar", "/tmp/nope.jar".into(), "", 1));
        assert!(run(&client, &cfg, p, cancel, |_, _, _| {}).is_err());
    }

    #[test]
    fn stage_totals_counts_each_part() {
        let mut p = Planned::default();
        p.client = Some(FileTask::new("http://x/c.jar", "/tmp/c.jar".into(), "", 2));
        p.assets.push(FileTask::new("http://x/a", "/tmp/a".into(), "", 3));
        let t = stage_totals(&p);
        assert_eq!(t[0].1, 1);
        assert_eq!(t[3].1, 1);
    }
}
