//! rules 求值。官方语义：
//! 1. 没有 rules 的条目默认允许；
//! 2. 有 rules 时默认拒绝，从上到下逐条应用命中的 action，最后一条命中者说了算。
//!
//! 两个真实数据里才看得到的坑：
//! - os.version 既可能是正则字符串，也可能是 { min, max } 区间对象；
//! - os.arch 会写 x86 / x86_64 / arm64，用错一个就是漏参数。
//! 写错这一块的典型后果：macOS 上漏掉 -XstartOnFirstThread，窗口直接黑屏。

use std::process::Command;

use regex::Regex;

use crate::minecraft::version_json::{Rule, VersionRange};

#[derive(Debug, Clone)]
pub struct Platform {
    /// 官方叫法：windows / osx / linux
    pub os_name: String,
    /// x86_64 / arm64 / x86
    pub os_arch: String,
    /// 用于 os.version 匹配，比如 10.0.19045 / 14.2
    pub os_version: String,
}

impl Platform {
    pub fn current() -> Self {
        let os_name = if cfg!(target_os = "windows") {
            "windows"
        } else if cfg!(target_os = "macos") {
            "osx"
        } else {
            "linux"
        }
        .to_string();

        let os_arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "arm64",
            "x86" => "x86",
            other => other,
        }
        .to_string();

        Self {
            os_name,
            os_arch,
            os_version: detect_os_version(),
        }
    }

    /// 供单测构造任意平台
    #[cfg(test)]
    pub fn new(name: &str, arch: &str, version: &str) -> Self {
        Self {
            os_name: name.to_string(),
            os_arch: arch.to_string(),
            os_version: version.to_string(),
        }
    }
}

fn detect_os_version() -> String {
    if cfg!(target_os = "windows") {
        // 必须给到三段（10.0.19045）：官方 versionRange 会比对 10.0.17134 这种粒度
        return windows_version().unwrap_or_else(|| "10.0".to_string());
    }
    if cfg!(target_os = "macos") {
        return Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "10.15".to_string());
    }
    "5.0".to_string()
}

/// cmd /C ver 输出：Microsoft Windows [Version 10.0.19045.3693]
fn windows_version() -> Option<String> {
    let out = Command::new("cmd").args(["/C", "ver"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let start = text.find("Version")? + "Version".len();
    let rest = text[start..].trim_start();
    let end = rest.find(']').unwrap_or(rest.len());
    let raw = rest[..end].trim();
    let parts: Vec<&str> = raw.split('.').collect();
    if parts.len() >= 3 {
        Some(format!("{}.{}.{}", parts[0], parts[1], parts[2]))
    } else if !raw.is_empty() {
        Some(raw.to_string())
    } else {
        None
    }
}

/// 启动器支持的扩展功能开关。官方将来加新 feature 时，未知项一律视为 false。
#[derive(Debug, Clone, Default)]
pub struct Features {
    pub demo: bool,
    pub custom_resolution: bool,
    pub quick_play: bool,
}

impl Features {
    pub fn get(&self, key: &str) -> bool {
        match key {
            "is_demo_user" => self.demo,
            "has_custom_resolution" | "has_custom_quickplay" => self.custom_resolution,
            "has_quick_plays_support"
            | "is_quick_play_singleplayer"
            | "is_quick_play_multiplayer"
            | "is_quick_play_realms" => self.quick_play,
            _ => false,
        }
    }
}

/// 判断一条 rules 列表在给定平台/功能下是否放行
pub fn allows(rules: &[Rule], platform: &Platform, features: &Features) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if matches(rule, platform, features) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn matches(rule: &Rule, platform: &Platform, features: &Features) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != &platform.os_name {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != &platform.os_arch {
                return false;
            }
        }
        if let Some(range) = &os.version {
            if !version_matches(range, &platform.os_version) {
                return false;
            }
        }
    }

    for (key, want) in rule.features.iter() {
        if features.get(key) != *want {
            return false;
        }
    }

    true
}

fn version_matches(range: &VersionRange, version: &str) -> bool {
    match range {
        VersionRange::Regex(pattern) => Regex::new(pattern)
            .map(|re| re.is_match(version))
            .unwrap_or(false),
        VersionRange::Range { min, max } => {
            if let Some(min) = min {
                if cmp_version(version, min) < 0 {
                    return false;
                }
            }
            if let Some(max) = max {
                if cmp_version(version, max) > 0 {
                    return false;
                }
            }
            true
        }
    }
}

/// 按 . - _ 分段比较版本号，缺失段当 0。不追求语义化版本规范，游戏数据只需要数字比较。
pub fn cmp_version(a: &str, b: &str) -> i32 {
    let split = |s: &str| -> Vec<String> { s.split(|c: char| c == '.' || c == '-' || c == '_').map(|x| x.to_string()).collect() };
    let pa = split(a);
    let pb = split(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).map(|s| s.as_str()).unwrap_or("0");
        let y = pb.get(i).map(|s| s.as_str()).unwrap_or("0");
        let nx: u64 = x.parse().unwrap_or(0);
        let ny: u64 = y.parse().unwrap_or(0);
        if nx != ny {
            return if nx > ny { 1 } else { -1 };
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::version_json::OsRule;

    fn os_rule(name: &str, action: &str) -> Rule {
        Rule {
            action: action.to_string(),
            os: Some(OsRule {
                name: Some(name.to_string()),
                arch: None,
                version: None,
            }),
            features: Default::default(),
        }
    }

    #[test]
    fn empty_rules_allow() {
        let p = Platform::new("windows", "x86_64", "10.0.19045");
        assert!(allows(&[], &p, &Features::default()));
    }

    #[test]
    fn allow_only_windows() {
        let rules = vec![os_rule("windows", "allow")];
        assert!(allows(&rules, &Platform::new("windows", "x86_64", "10.0.19045"), &Features::default()));
        assert!(!allows(&rules, &Platform::new("osx", "arm64", "14.2"), &Features::default()));
    }

    #[test]
    fn disallow_windows() {
        let rules = vec![os_rule("windows", "disallow"), os_rule("osx", "allow")];
        assert!(!allows(&rules, &Platform::new("windows", "x86_64", "10.0.19045"), &Features::default()));
        assert!(allows(&rules, &Platform::new("osx", "arm64", "14.2"), &Features::default()));
    }

    #[test]
    fn unknown_os_never_matches() {
        let rules = vec![os_rule("windows", "allow")];
        assert!(!allows(&rules, &Platform::new("linux", "x86_64", "5.0"), &Features::default()));
    }

    #[test]
    fn feature_rule_needs_matching_flag() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: None,
            features: [("is_demo_user".to_string(), true)].into_iter().collect(),
        }];
        let mut f = Features::default();
        assert!(!allows(&rules, &Platform::new("windows", "x86_64", "10.0.19045"), &f));
        f.demo = true;
        assert!(allows(&rules, &Platform::new("windows", "x86_64", "10.0.19045"), &f));
    }

    #[test]
    fn version_regex_range() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some("osx".into()),
                arch: None,
                version: Some(VersionRange::Regex("^10\\.".into())),
            }),
            features: Default::default(),
        }];
        assert!(allows(&rules, &Platform::new("osx", "x86_64", "10.15"), &Features::default()));
        assert!(!allows(&rules, &Platform::new("osx", "arm64", "14.2"), &Features::default()));
    }

    /// 对应 26.3 的 default-user-jvm：windows 低于 10.0.17134 走 G1 分支
    #[test]
    fn version_min_max_object() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some("windows".into()),
                arch: None,
                version: Some(VersionRange::Range {
                    min: Some("10.0.17134".into()),
                    max: None,
                }),
            }),
            features: Default::default(),
        }];
        assert!(allows(&rules, &Platform::new("windows", "x86_64", "10.0.19045"), &Features::default()));
        assert!(!allows(&rules, &Platform::new("windows", "x86_64", "10.0.10240"), &Features::default()));

        let rules_max = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some("windows".into()),
                arch: None,
                version: Some(VersionRange::Range {
                    min: None,
                    max: Some("10.0.17134".into()),
                }),
            }),
            features: Default::default(),
        }];
        assert!(allows(&rules_max, &Platform::new("windows", "x86_64", "10.0.10240"), &Features::default()));
        assert!(!allows(&rules_max, &Platform::new("windows", "x86_64", "10.0.19045"), &Features::default()));
    }

    #[test]
    fn arch_x86_only_matches_32bit() {
        let rules = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: None,
                arch: Some("x86".into()),
                version: None,
            }),
            features: Default::default(),
        }];
        assert!(!allows(&rules, &Platform::new("windows", "x86_64", "10.0.19045"), &Features::default()));
        assert!(allows(&rules, &Platform::new("windows", "x86", "10.0.19045"), &Features::default()));
    }

    #[test]
    fn compare_versions() {
        assert_eq!(cmp_version("10.0.19045", "10.0.17134"), 1);
        assert_eq!(cmp_version("10.0.17134", "10.0.17134"), 0);
        assert_eq!(cmp_version("10.0", "10.0.17134"), -1);
    }
}
