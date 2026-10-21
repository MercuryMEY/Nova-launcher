//! 启动参数组装。四个容易踩的坑都在这里处理：
//! 1. 新版 arguments.jvm 里带 rules 条件项，旧版只有 minecraftArguments 一整行；
//! 2. 官方在 jvm 段用 ${natives_directory} 这类占位符，必须全量替换；
//! 3. default-user-jvm 里带 -Xms2G / -Xmx4G，内存必须由启动器按用户设置决定，
//!    否则官方默认值会把用户在设置里填的内存盖掉；
//! 4. 条件项的 value 既可能是字符串，也可能是数组。

use std::collections::HashMap;

use crate::minecraft::rules::{allows, Features, Platform};
use crate::minecraft::version_json::{ArgValue, ArgValueValue, VersionJson};

pub const BRAND: &str = "NovaLauncher";

/// 返回 (jvm 参数, 游戏参数)。natives 目录、classpath 等由调用方预置进 vars
pub fn build(
    version: &VersionJson,
    platform: &Platform,
    features: &Features,
    vars: &HashMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut jvm: Vec<String> = match &version.arguments {
        Some(args) => {
            let mut list = expand(&args.jvm, platform, features, vars);

            for arg in expand(&args.default_user_jvm, platform, features, vars) {
                if is_memory_flag(&arg) {
                    continue;
                }
                list.push(arg);
            }

            if list.is_empty() {
                list = fallback_jvm(vars);
            }
            list
        }
        None => fallback_jvm(vars),
    };

    let game: Vec<String> = match (&version.arguments, &version.minecraft_arguments) {
        (Some(args), _) => expand(&args.game, platform, features, vars),
        (None, Some(line)) => line
            .split_whitespace()
            .map(|t| substitute(t, vars))
            .collect(),
        (None, None) => Vec::new(),
    };

    // 官方启动器会带品牌字段，部分统计/日志模组依赖它们
    if !jvm.iter().any(|a| a.starts_with("-Dminecraft.launcher.brand")) {
        jvm.push(format!("-Dminecraft.launcher.brand={BRAND}"));
    }
    if !jvm
        .iter()
        .any(|a| a.starts_with("-Dminecraft.launcher.version"))
    {
        jvm.push(format!(
            "-Dminecraft.launcher.version={}",
            env!("CARGO_PKG_VERSION")
        ));
    }

    (jvm, game)
}

/// 内存与栈大小一律交给启动器控制
fn is_memory_flag(arg: &str) -> bool {
    arg.starts_with("-Xms")
        || arg.starts_with("-Xmx")
        || arg.starts_with("-Xss")
        || arg.starts_with("-XX:MaxHeapSize")
        || arg.starts_with("-XX:InitialHeapSize")
}

fn fallback_jvm(vars: &HashMap<String, String>) -> Vec<String> {
    vec![
        substitute("-Djava.library.path=${natives_directory}", vars),
        "-cp".to_string(),
        substitute("${classpath}", vars),
    ]
}

fn expand(
    items: &[ArgValue],
    platform: &Platform,
    features: &Features,
    vars: &HashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        match item {
            ArgValue::Plain(s) => out.push(substitute(s, vars)),
            ArgValue::Conditional { rules, value } => {
                if !allows(rules, platform, features) {
                    continue;
                }
                match value {
                    ArgValueValue::One(s) => out.push(substitute(s, vars)),
                    ArgValueValue::Many(list) => {
                        for s in list {
                            out.push(substitute(s, vars));
                        }
                    }
                }
            }
        }
    }
    out
}

/// 替换 ${key} 占位符。未知占位符原样保留，出错时一眼能看出缺了哪项。
pub fn substitute(token: &str, vars: &HashMap<String, String>) -> String {
    if !token.contains("${") {
        return token.to_string();
    }
    let mut out = String::with_capacity(token.len());
    let mut rest = token;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let key = &after[..end];
                match vars.get(key) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&rest[start..start + 2 + end + 1]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::version_json::{OsRule, VersionRange};

    fn vars() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("auth_player_name".into(), "Steve".into());
        m.insert("version_name".into(), "26.3".into());
        m.insert("natives_directory".into(), "/tmp/n".into());
        m.insert("classpath".into(), "/tmp/a.jar".into());
        m
    }

    fn win() -> Platform {
        Platform::new("windows", "x86_64", "10.0.19045")
    }

    #[test]
    fn substitute_known_and_unknown() {
        let v = vars();
        assert_eq!(substitute("${auth_player_name}", &v), "Steve");
        assert_eq!(
            substitute("--version ${version_name}", &v),
            "--version 26.3"
        );
        assert_eq!(substitute("${unknown_key}", &v), "${unknown_key}");
        assert_eq!(substitute("plain", &v), "plain");
    }

    #[test]
    fn legacy_minecraft_arguments() {
        let json = r#"{
            "id": "1.5.2",
            "minecraftArguments": "--username ${auth_player_name} --version ${version_name}"
        }"#;
        let v: VersionJson = serde_json::from_str(json).unwrap();
        let (jvm, game) = build(&v, &win(), &Features::default(), &vars());
        assert_eq!(game, vec!["--username", "Steve", "--version", "26.3"]);
        assert!(jvm.iter().any(|a| a == "-cp"));
    }

    #[test]
    fn conditional_jvm_dropped_on_wrong_os() {
        let json = r#"{
            "id": "t",
            "arguments": {
                "jvm": [
                    "-cp",
                    { "rules": [{ "action": "allow", "os": { "name": "osx" } }], "value": "-XstartOnFirstThread" }
                ],
                "game": [
                    { "rules": [{ "action": "allow", "features": { "is_demo_user": true } }], "value": "--demo" }
                ]
            }
        }"#;
        let v: VersionJson = serde_json::from_str(json).unwrap();
        let (jvm, game) = build(&v, &win(), &Features::default(), &vars());
        assert!(!jvm.iter().any(|a| a == "-XstartOnFirstThread"));
        assert!(game.is_empty());

        let (jvm_mac, _) = build(
            &v,
            &Platform::new("osx", "arm64", "14.2"),
            &Features::default(),
            &vars(),
        );
        assert!(jvm_mac.iter().any(|a| a == "-XstartOnFirstThread"));
    }

    #[test]
    fn array_value_expands() {
        let json = r#"{
            "id": "t",
            "arguments": {
                "jvm": ["-cp"],
                "game": [
                    { "rules": [], "value": ["--width", "854", "--height", "480"] }
                ]
            }
        }"#;
        let v: VersionJson = serde_json::from_str(json).unwrap();
        let (_, game) = build(&v, &win(), &Features::default(), &vars());
        assert_eq!(game, vec!["--width", "854", "--height", "480"]);
    }

    /// 真实 26.3 的 default-user-jvm：第一项没有 rules，value 是数组
    #[test]
    fn default_user_jvm_memory_is_stripped() {
        let json = r#"{
            "id": "t",
            "arguments": {
                "jvm": ["-cp", "${classpath}"],
                "default-user-jvm": [
                    { "value": ["-Xms2G", "-Xmx4G", "-XX:+UseCompactObjectHeaders", "-XX:+AlwaysPreTouch"] }
                ]
            }
        }"#;
        let v: VersionJson = serde_json::from_str(json).unwrap();
        let (jvm, _) = build(&v, &win(), &Features::default(), &vars());
        assert!(!jvm.iter().any(|a| a.starts_with("-Xmx")));
        assert!(!jvm.iter().any(|a| a.starts_with("-Xms")));
        assert!(jvm.iter().any(|a| a == "-XX:+UseCompactObjectHeaders"));
        assert!(jvm.iter().any(|a| a == "-XX:+AlwaysPreTouch"));
    }

    /// 对应真实数据里的 windows versionRange 分支
    #[test]
    fn version_range_branch_selected_by_os_version() {
        let json = r#"{
            "id": "t",
            "arguments": {
                "jvm": ["-cp"],
                "default-user-jvm": [
                    { "rules": [{ "action": "allow", "os": { "name": "windows", "versionRange": { "min": "10.0.17134" } } }], "value": ["-XX:+UseZGC"] }
                ]
            }
        }"#;
        let v: VersionJson = serde_json::from_str(json).unwrap();

        let (old_jvm, _) = build(
            &v,
            &Platform::new("windows", "x86_64", "10.0.10240"),
            &Features::default(),
            &vars(),
        );
        assert!(!old_jvm.iter().any(|a| a == "-XX:+UseZGC"));

        let (new_jvm, _) = build(&v, &win(), &Features::default(), &vars());
        assert!(new_jvm.iter().any(|a| a == "-XX:+UseZGC"));
    }

    #[test]
    fn macos_rule_fixture() {
        let rules = vec![crate::minecraft::version_json::Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some("osx".into()),
                arch: None,
                version: Some(VersionRange::Regex("^1[0-9]\\.".into())),
            }),
            features: Default::default(),
        }];
        assert!(allows(
            &rules,
            &Platform::new("osx", "arm64", "14.2"),
            &Features::default()
        ));
    }
}
