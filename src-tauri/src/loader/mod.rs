//! 加载器模块：查版本、装加载器。
//!
//! 这里只 `pub mod`，不做函数级再导出：`install` 既是模块名，里面又有同名函数，
//! 一起 re-export 会让 `crate::loader::install` 同时落在类型与值两个命名空间，
//! 调用方容易看错，不如让路径明确写成 `install::install`。
pub mod install;
pub mod meta;
pub use meta::LoaderKind;