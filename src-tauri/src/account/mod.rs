//! 账户体系。
//!
//! 两条线：离线账户（offline）与微软账户（microsoft）。
//! 凭据统一走 secret 抽象：Windows 凭据管理器 / macOS 钥匙串 / 其余平台内存兜底。
pub mod microsoft;
pub mod offline;
pub mod secret;
