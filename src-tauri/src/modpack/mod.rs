//! 整合包：目前只支持 Modrinth 的 `.mrpack`。
//!
//! `mrpack.rs` 负责读懂索引，`install.rs` 负责把包落到 `instances/<id>/`。
//! CurseForge 的 zip 结构完全不同（manifest.json + overrides），等真要做再单开模块。
pub mod install;
pub mod mrpack;