//! 安装：把一份版本 JSON 变成磁盘上能启动的目录。
//!
//! 分两层：plan 只算“要下什么”，executor 负责真的下。
//! 拆开是因为预览功能只想要计划、不想动磁盘。
pub mod executor;
pub mod plan;
