//! 崩溃分析命令：读日志尾部 → 跑一遍本地规则 → 返回结论。
//!
//! 纯本地、无网络，所以同步执行就够快（四千行扫十几条规则是毫秒级），
//! 不用往 `spawn_blocking` 里塞。
use crate::crash;
use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::launch::logs;

// name：要分析的日志文件名，不传就取最新的那份
// code：进程退出码，用来补一条通俗解读
// lines：看多少行，默认四千
#[tauri::command]
pub fn crash_analyze(
    name: Option<String>,
    code: Option<i32>,
    lines: Option<usize>,
) -> AppResult<crash::Analysis> {
    let dir = fsutil::logs_dir();
    let picked = match name {
        Some(n) => n,
        None => {
            // 列表已按文件名倒序，第一条就是最新的
            let list = logs::list(&dir);
            if list.is_empty() {
                return Err(AppError::Message("还没有日志，先启动一次游戏再来".into()));
            }
            list[0].name.clone()
        }
    };
    let (text, truncated) =
        logs::read_tail(&dir, &picked, lines.unwrap_or(4000).clamp(200, 20000))?;
    Ok(crash::analyze(&text, truncated, code))
}