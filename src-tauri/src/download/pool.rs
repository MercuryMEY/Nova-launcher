//! 有限并发下载池。
//!
//! 为什么不用 tokio：整个项目用的是 reqwest 的 blocking 版本，
//! 为了几个下载任务再引一套 async 运行时，得不偿失，
//! 用 N 个线程抢一个队列就够了（资源文件几千个，4 线程已能跑满带宽）。
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::download::file::{self, FileTask};
use crate::error::{AppError, AppResult};

#[derive(Debug, Default, Clone)]
pub struct PoolReport {
    /// 本次真正下载的文件数
    pub downloaded: usize,
    /// 已存在且校验通过、跳过的
    pub skipped: usize,
    pub bytes: u64,
    pub failed: Vec<String>,
}

/// `stage` 会原样回传给进度回调，界面据此显示“正在下载依赖库”这类文案。
/// `on_progress(stage, done, total)` 在多个工作线程里被调用，内部会加锁串行化。
pub fn run<F>(
    client: &reqwest::blocking::Client,
    stage: &'static str,
    tasks: Vec<FileTask>,
    workers: usize,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
    on_progress: F,
) -> AppResult<PoolReport>
where
    F: FnMut(&'static str, usize, usize) + Send + 'static,
{
    let total = tasks.len();
    if total == 0 {
        return Ok(PoolReport::default());
    }

    let queue = Arc::new(Mutex::new(VecDeque::from(tasks)));
    let report = Arc::new(Mutex::new(PoolReport::default()));
    let counter = Arc::new(AtomicUsize::new(0));
    let callback = Arc::new(Mutex::new(on_progress));
    let limit = workers.clamp(1, 8).min(total);

    let mut handles = Vec::with_capacity(limit);
    for _ in 0..limit {
        let client = client.clone();
        let queue = queue.clone();
        let report = report.clone();
        let counter = counter.clone();
        let callback = callback.clone();
        let cancel = cancel.clone();

        handles.push(std::thread::spawn(move || -> AppResult<()> {
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err(AppError::Message("下载已取消".into()));
                }
                let next = {
                    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
                    q.pop_front()
                };
                let Some(task) = next else { return Ok(()) };

                // 先判已就位，避免 download_file 内部再算一次 SHA1
                if file::already_ok(&task) {
                    if let Ok(mut r) = report.lock() {
                        r.skipped += 1;
                    }
                } else {
                    let mut noop = |_d: u64, _t: u64| {};
                    match file::download_file(&client, &task, timeout, &mut noop) {
                        Ok(()) => {
                            if let Ok(mut r) = report.lock() {
                                r.downloaded += 1;
                                r.bytes += task.size;
                            }
                        }
                        Err(e) => {
                            if let Ok(mut r) = report.lock() {
                                if r.failed.len() < 10 {
                                    r.failed
                                        .push(format!("{}：{e}", task.dest.display()));
                                }
                            }
                        }
                    }
                }

                let done = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if let Ok(mut cb) = callback.lock() {
                    cb(stage, done, total);
                }
            }
        }));
    }

    for h in handles {
        h.join()
            .map_err(|_| AppError::Message("下载线程异常退出".into()))??;
    }

    let report = Arc::try_unwrap(report)
        .map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .unwrap_or_default();

    if !report.failed.is_empty() {
        return Err(AppError::Message(format!(
            "有文件没下载成功（{} 个），例如 {}；可以再点一次重试，已完成的不会重复下载",
            report.failed.len(),
            report.failed[0]
        )));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_task_list_is_ok() {
        let cancel = Arc::new(AtomicBool::new(false));
        // 不联网：任务为空时直接返回，不会碰 client
        let client = reqwest::blocking::Client::new();
        let r = run(&client, "测试", Vec::new(), 4, Duration::from_secs(1), cancel, |_, _, _| {});
        assert!(r.is_ok());
        assert_eq!(r.unwrap().downloaded, 0);
    }
}
