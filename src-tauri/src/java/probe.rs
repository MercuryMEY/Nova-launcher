//! Java 探测：找出机器上可用的 java 可执行文件，读出主版本。
//!
//! 注意 `java -version` 的输出写在 stderr，只读 stdout 会一个版本都解析不到。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaInfo {
    pub path: PathBuf,
    pub version: String,
    pub major: u32,
    pub arch: String,
    /// 来自哪里：设置 / JAVA_HOME / 安装目录 / PATH
    pub source: String,
}

fn is_sep(c: char) -> bool {
    c == '.' || c == '-' || c == '_' || c == '+'
}

/// 从 `java -version` 首行解析主版本。
/// `openjdk version "25.0.1"` → 25；`java version "1.8.0_392"` → 8。
pub fn parse_major(line: &str) -> Option<u32> {
    let start = line.find('"')? + 1;
    let rest = &line[start..];
    let end = rest.find('"')?;
    let ver = &rest[..end];
    let mut parts = ver.split(is_sep);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// 执行 `java -version` 并解析；文件不存在或不是 java 时返回 None
pub fn inspect(path: &Path) -> Option<JavaInfo> {
    if !path.is_file() {
        return None;
    }
    let mut cmd = Command::new(path);
    cmd.arg("-version");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let text = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let first = text.lines().next()?;
    let major = parse_major(first)?;
    let version = first
        .split('"')
        .nth(1)
        .unwrap_or_default()
        .to_string();
    Some(JavaInfo {
        path: path.to_path_buf(),
        version,
        major,
        arch: std::env::consts::ARCH.to_string(),
        source: String::new(),
    })
}

fn java_exe() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

fn subdirs(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(base) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                out.push(p);
            }
        }
    }
    out
}

/// 待探测的候选路径，按优先级排列
pub fn candidates(extra: Option<&str>) -> Vec<(PathBuf, &'static str)> {
    let exe = java_exe();
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();

    if let Some(raw) = extra {
        let raw = raw.trim();
        if !raw.is_empty() {
            let pb = PathBuf::from(raw);
            if pb.is_file() {
                out.push((pb, "设置"));
            } else {
                let in_bin = pb.join("bin").join(exe);
                if in_bin.is_file() {
                    out.push((in_bin, "设置"));
                }
                let direct = pb.join(exe);
                if direct.is_file() {
                    out.push((direct, "设置"));
                }
            }
        }
    }

    if let Ok(home) = std::env::var("JAVA_HOME") {
        let p = PathBuf::from(home).join("bin").join(exe);
        if p.is_file() {
            out.push((p, "JAVA_HOME"));
        }
    }

    #[cfg(windows)]
    {
        let mut roots: Vec<PathBuf> = Vec::new();
        for key in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Ok(v) = std::env::var(key) {
                roots.push(PathBuf::from(v));
            }
        }
        let vendors = [
            "Java",
            "Eclipse Adoptium",
            "Microsoft",
            "Zulu",
            "BellSoft",
            "Amazon Corretto",
            "Semeru",
        ];
        for root in roots {
            for vendor in vendors {
                for dir in subdirs(&root.join(vendor)) {
                    let p = dir.join("bin").join(exe);
                    if p.is_file() {
                        out.push((p, "安装目录"));
                    }
                }
            }
            // LOCALAPPDATA 下还有 Programs\Eclipse Adoptium 这种结构
            for dir in subdirs(&root.join("Programs")) {
                let p = dir.join("bin").join(exe);
                if p.is_file() {
                    out.push((p, "安装目录"));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for dir in subdirs(Path::new("/Library/Java/JavaVirtualMachines")) {
            let p = dir.join("Contents/Home/bin/java");
            if p.is_file() {
                out.push((p, "系统安装"));
            }
        }
        for dir in subdirs(Path::new("/System/Library/Java/JavaVirtualMachines")) {
            let p = dir.join("Contents/Home/bin/java");
            if p.is_file() {
                out.push((p, "系统安装"));
            }
        }
        let brew = PathBuf::from("/opt/homebrew/opt");
        for dir in subdirs(&brew) {
            if dir.file_name().map(|n| n.to_string_lossy().starts_with("openjdk")).unwrap_or(false) {
                let p = dir.join("bin/java");
                if p.is_file() {
                    out.push((p, "Homebrew"));
                }
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        for base in ["/usr/lib/jvm", "/opt/java"] {
            for dir in subdirs(Path::new(base)) {
                let p = dir.join("bin").join(exe);
                if p.is_file() {
                    out.push((p, "系统安装"));
                }
            }
        }
    }

    // PATH 里最后看：大多数是软链接，容易和上面的重复
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe);
            if p.is_file() {
                out.push((p, "PATH"));
            }
        }
    }

    out
}

/// 缓存和“当时用的 extra 路径”一起存：
/// 否则用户在设置里填了新路径、或者刚装完运行时，都会读到旧列表
static CACHE: OnceLock<Mutex<Option<(Option<String>, Vec<JavaInfo>)>>> = OnceLock::new();

/// 探测全部可用 Java。`force` 为真时忽略缓存重新扫。
pub fn detect(extra: Option<&str>, force: bool) -> Vec<JavaInfo> {
    let cell = CACHE.get_or_init(|| Mutex::new(None));
    if !force {
        if let Ok(guard) = cell.lock() {
            if let Some((key, list)) = guard.as_ref() {
                if key.as_deref() == extra {
                    return list.clone();
                }
            }
        }
    }

    let mut seen: Vec<PathBuf> = Vec::new();
    let mut list: Vec<JavaInfo> = Vec::new();
    for (path, source) in candidates(extra) {
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        if let Some(mut info) = inspect(&path) {
            info.source = source.to_string();
            // 同一份 JDK 可能被 PATH 和安装目录各扫到一次
            if list.iter().any(|j| j.major == info.major && j.version == info.version) {
                continue;
            }
            list.push(info);
        }
    }
    list.sort_by(|a, b| b.major.cmp(&a.major));

    if let Ok(mut guard) = cell.lock() {
        *guard = Some((extra.map(|s| s.to_string()), list.clone()));
    }
    list
}

/// 选一个能跑目标版本的 Java：优先恰好相等，其次取高于目标里最低的
pub fn pick(list: &[JavaInfo], target: u32) -> Option<JavaInfo> {
    if let Some(exact) = list.iter().find(|j| j.major == target) {
        return Some(exact.clone());
    }
    let mut best: Option<&JavaInfo> = None;
    for j in list.iter().filter(|j| j.major > target) {
        match best {
            Some(b) if b.major <= j.major => {}
            _ => best = Some(j),
        }
    }
    best.cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(major: u32) -> JavaInfo {
        JavaInfo {
            path: PathBuf::from(format!("/jvm/{major}/bin/java")),
            version: format!("{major}.0.1"),
            major,
            arch: "x86_64".into(),
            source: "test".into(),
        }
    }

    #[test]
    fn parse_modern_version() {
        assert_eq!(parse_major("openjdk version \"25.0.1\" 2025-10-21"), Some(25));
        assert_eq!(parse_major("java version \"17.0.8\" 2023-07-18"), Some(17));
    }

    #[test]
    fn parse_legacy_version() {
        assert_eq!(parse_major("java version \"1.8.0_392\""), Some(8));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert_eq!(parse_major("no quotes here"), None);
        assert_eq!(parse_major("java version \"abc\""), None);
    }

    #[test]
    fn pick_prefers_exact_then_lowest_above() {
        let list = vec![info(8), info(17), info(21), info(25)];
        assert_eq!(pick(&list, 21).unwrap().major, 21);
        assert_eq!(pick(&list, 16).unwrap().major, 17);
        assert_eq!(pick(&list, 30).map(|j| j.major), None);
    }
}
