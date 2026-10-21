//! 启动互斥锁。
//!
//! 为什么不用 `Mutex<bool>`：`launch_game` 是 async 命令，抢锁之后还要跨好几个
//! `await`（账户验证、拼参数、解压、探测 Java）。`MutexGuard` 不是 `Send`，
//! 跨 await 持有直接编译失败。所以这里用 `AtomicBool` + `Drop`，
//! 既不阻塞、也不怕提前 `?` 返回。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{AppError, AppResult};

/// 拿到手就代表「本次启动归我」，离开作用域自动释放
pub struct LaunchGuard {
    flag: Arc<AtomicBool>,
}

impl LaunchGuard {
    /// 抢占启动权。已经有人在启动/运行就直接报错，不做排队
    pub fn acquire(flag: &Arc<AtomicBool>) -> AppResult<Self> {
        match flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst) {
            Ok(_) => Ok(Self {
                flag: flag.clone(),
            }),
            Err(_) => Err(AppError::LaunchFailed(
                "正在启动游戏，先等这一轮结束".into(),
            )),
        }
    }
}

impl Drop for LaunchGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    /// 测试里直接读标志，正式代码不需要这个封装
    fn busy(f: &Arc<AtomicBool>) -> bool {
        f.load(Ordering::SeqCst)
    }

    #[test]
    fn second_acquire_fails_while_held() {
        let f = flag();
        let g = LaunchGuard::acquire(&f).unwrap();
        assert!(busy(&f));
        assert!(LaunchGuard::acquire(&f).is_err());
        drop(g);
    }

    #[test]
    fn release_on_drop_allows_retry() {
        let f = flag();
        {
            let _g = LaunchGuard::acquire(&f).unwrap();
        }
        assert!(!busy(&f));
        assert!(LaunchGuard::acquire(&f).is_ok());
    }

    #[test]
    fn error_path_also_releases() {
        let f = flag();
        // 模拟启动中途失败：guard 被 drop 后锁必须回到可用状态
        let result: AppResult<()> = {
            let _g = LaunchGuard::acquire(&f).unwrap();
            Err(AppError::LaunchFailed("模拟失败".into()))
        };
        assert!(result.is_err());
        assert!(!busy(&f), "失败路径也必须释放启动锁");
    }
}
