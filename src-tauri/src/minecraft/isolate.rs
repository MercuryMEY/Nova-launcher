//!目录布局与版本隔离。
//!三种模式对应 PCL2 的语义：
//!- full：每个版本独立 gameDir，Mod 互不影响；
//!- none：全部版本共用 .minecraft 根目录；
//!- non-fake：原版不隔离，带 Mod 的版本才隔离（PCL2 默认）。

use std::path::{Path, PathBuf};

use crate::state::IsolationMode;

/// 手动标记：在版本目录里放这个文件就强制隔离
pub const ISOLATE_MARKER: &str = ".isolate";

pub fn versions_dir(root: &Path) -> PathBuf {
    root.join("versions")
}

pub fn version_dir(root: &Path, id: &str) -> PathBuf {
    versions_dir(root).join(id)
}

pub fn libraries_dir(root: &Path) -> PathBuf {
    root.join("libraries")
}

pub fn assets_dir(root: &Path) -> PathBuf {
    root.join("assets")
}

pub fn natives_dir(root: &Path, id: &str) -> PathBuf {
    version_dir(root, id).join("natives")
}

/// 26.3 的 JVM 参数写的是 -Djava.library.path=${natives_directory}/java，
/// 说明官方把 native 库解压到 natives/java（jna / lwjgl / netty 各有子目录）。
/// B5 解压时必须落到这个目录，否则 LWJGL 找不到 .so / .dll。
pub fn native_extract_dir(root: &Path, id: &str) -> PathBuf {
    natives_dir(root, id).join("java")
}

pub fn json_path(root: &Path, id: &str) -> PathBuf {
    version_dir(root, id).join(format!("{id}.json"))
}

pub fn jar_path(root: &Path, id: &str) -> PathBuf {
    version_dir(root, id).join(format!("{id}.jar"))
}

/// 这个版本是否值得单独隔离
fn should_isolate(root: &Path, id: &str) -> bool {
    let dir = version_dir(root, id);
    dir.join("mods").is_dir() || dir.join(ISOLATE_MARKER).exists()
}

/// 结算出实际使用的 gameDir
pub fn game_dir(root: &Path, id: &str, mode: IsolationMode) -> PathBuf {
    match mode {
        IsolationMode::None => root.to_path_buf(),
        IsolationMode::Full => version_dir(root, id),
        IsolationMode::NonFake => {
            if should_isolate(root, id) {
                version_dir(root, id)
            } else {
                root.to_path_buf()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_mode_uses_root() {
        let root = Path::new("/mc");
        assert_eq!(game_dir(root, "26.3", IsolationMode::None), Path::new("/mc"));
    }

    #[test]
    fn full_mode_uses_version_dir() {
        let root = Path::new("/mc");
        assert_eq!(
            game_dir(root, "26.3", IsolationMode::Full),
            Path::new("/mc/versions/26.3")
        );
    }

    #[test]
    fn paths_are_stable() {
        let root = Path::new("/mc");
        assert_eq!(json_path(root, "a"), Path::new("/mc/versions/a/a.json"));
        assert_eq!(libraries_dir(root), Path::new("/mc/libraries"));
        assert_eq!(natives_dir(root, "a"), Path::new("/mc/versions/a/natives"));
    }
}
