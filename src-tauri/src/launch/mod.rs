//! 启动链路：拼参数 → 解压 natives → 拉进程 → 日志落盘。
pub mod command;
pub mod lock;
pub mod logs;
pub mod options;
pub mod proc;
pub use lock::LaunchGuard;
