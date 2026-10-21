//! 版本 JSON 的父子合并（`inheritsFrom`）。
//!
//! 加载器版本（Fabric / Quilt / Forge / NeoForge）的 JSON 只有自己那一小段：
//! 主类、自己的库、自己的参数，其余全部靠 `inheritsFrom` 指向原版。
//! 不合并的话，classpath 里没有原版的库、assets 拿不到、mainClass 也是错的，
//! 装完只能启动到一半崩掉。
//!
//! 合并规则照抄官方启动器：
//! - 主类 / 资源 / Java 版本：子有就用子，否则落父
//! - 依赖库：父在前，子在后；同一个 `group:artifact` 用子覆盖父（保持父的位置）
//! - 启动参数：父在前，子在后，逐项拼接
use std::path::Path;

use crate::error::{AppError, AppResult};
use crate::fsutil;
use crate::minecraft::isolate;
use crate::minecraft::version_json::{Arguments, Library, VersionJson};

/// 防环：正常最多两层（加载器 → 原版），超过这个数一定有问题
const MAX_DEPTH: usize = 4;

/// 从库坐标里取去重键：`group:artifact`，忽略版本与 classifier
fn lib_key(name: &str) -> String {
    let base = name.split('@').next().unwrap_or(name);
    let mut parts = base.split(':');
    match (parts.next(), parts.next()) {
        (Some(g), Some(a)) => format!("{g}:{a}"),
        _ => base.to_string(),
    }
}

fn merge_libraries(parent: &[Library], child: &[Library]) -> Vec<Library> {
    let mut out: Vec<Library> = parent.to_vec();
    for lib in child {
        let key = lib_key(&lib.name);
        match out.iter().position(|p| lib_key(&p.name) == key) {
            // 子版本更准确（例如 Forge 换掉了某个 asm 版本），但位置保持不动，
            // 否则 LWJGL 这类对顺序敏感的库会被挤到后面
            Some(idx) => out[idx] = lib.clone(),
            None => out.push(lib.clone()),
        }
    }
    out
}

fn merge_args(parent: Option<&Arguments>, child: Option<&Arguments>) -> Option<Arguments> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => {
            let mut game = p.game.clone();
            game.extend(c.game.iter().cloned());
            let mut jvm = p.jvm.clone();
            jvm.extend(c.jvm.iter().cloned());
            let mut user = p.default_user_jvm.clone();
            user.extend(c.default_user_jvm.iter().cloned());
            Some(Arguments {
                game,
                jvm,
                default_user_jvm: user,
            })
        }
    }
}

/// 1.6 之前的老格式是一整行字符串，同样父在前子在后
fn merge_minecraft_arguments(parent: Option<&str>, child: Option<&str>) -> Option<String> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.to_string()),
        (None, Some(c)) => Some(c.to_string()),
        (Some(p), Some(c)) => Some(format!("{p} {c}")),
    }
}

pub fn merge(parent: &VersionJson, child: &VersionJson) -> VersionJson {
    VersionJson {
        // id 必须是子版本自己的，界面与磁盘目录都靠它
        id: child.id.clone(),
        main_class: child.main_class.clone().or_else(|| parent.main_class.clone()),
        assets: child.assets.clone().or_else(|| parent.assets.clone()),
        asset_index: child
            .asset_index
            .clone()
            .or_else(|| parent.asset_index.clone()),
        downloads: child.downloads.clone().or_else(|| parent.downloads.clone()),
        java_version: child
            .java_version
            .clone()
            .or_else(|| parent.java_version.clone()),
        libraries: merge_libraries(&parent.libraries, &child.libraries),
        arguments: merge_args(parent.arguments.as_ref(), child.arguments.as_ref()),
        minecraft_arguments: merge_minecraft_arguments(
            parent.minecraft_arguments.as_deref(),
            child.minecraft_arguments.as_deref(),
        ),
        release_time: child
            .release_time
            .clone()
            .or_else(|| parent.release_time.clone()),
        kind: child.kind.clone().or_else(|| parent.kind.clone()),
        inherits_from: child.inherits_from.clone(),
        logging: child.logging.clone().or_else(|| parent.logging.clone()),
    }
}

fn load_one(root: &Path, id: &str) -> AppResult<VersionJson> {
    let path = isolate::json_path(root, id);
    fsutil::read_json::<VersionJson>(&path)?.ok_or_else(|| {
        AppError::VersionInvalid(format!(
            "{id} 尚未安装，缺少 {id}.json；如果这是加载器版本，请先装好它依赖的原版"
        ))
    })
}

/// 读一个版本并把它沿 `inheritsFrom` 一路合并到底
pub fn load_with_parent(root: &Path, id: &str) -> AppResult<VersionJson> {
    load_rec(root, id, 0)
}

fn load_rec(root: &Path, id: &str, depth: usize) -> AppResult<VersionJson> {
    if depth > MAX_DEPTH {
        return Err(AppError::VersionInvalid(format!(
            "{id} 的 inheritsFrom 链太深，可能互相继承成环"
        )));
    }
    let child = load_one(root, id)?;
    let Some(parent_id) = child.inherits_from.clone() else {
        return Ok(child);
    };
    if parent_id == id {
        return Err(AppError::VersionInvalid(format!("{id} 继承了自己")));
    }
    let parent = load_rec(root, &parent_id, depth + 1)?;
    Ok(merge(&parent, &child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::minecraft::version_json::{ArgValue, Artifact, AssetIndex, JavaVersion};

    fn lib(name: &str) -> Library {
        Library {
            name: name.into(),
            downloads: None,
            natives: None,
            rules: Vec::new(),
            extract: None,
            url: None,
            sha1: None,
            size: None,
        }
    }

    fn base() -> VersionJson {
        VersionJson {
            id: "1.20.1".into(),
            main_class: Some("net.minecraft.client.main.Main".into()),
            assets: Some("5".into()),
            asset_index: Some(AssetIndex {
                id: "5".into(),
                sha1: None,
                size: None,
                total_size: None,
                url: Some("https://example.invalid/5.json".into()),
            }),
            downloads: None,
            java_version: Some(JavaVersion {
                major_version: 17,
                component: Some("java-runtime-gamma".into()),
            }),
            libraries: vec![lib("org.ow2.asm:asm:9.5"), lib("com.mojang:brigadier:1.0.18")],
            arguments: Some(Arguments {
                game: vec![ArgValue::Plain("--username".into())],
                jvm: Vec::new(),
                default_user_jvm: Vec::new(),
            }),
            minecraft_arguments: None,
            release_time: Some("2023-06-12T00:00:00+00:00".into()),
            kind: Some("release".into()),
            inherits_from: None,
            logging: None,
        }
    }

    fn child() -> VersionJson {
        VersionJson {
            id: "fabric-loader-0.15.0-1.20.1".into(),
            main_class: Some("net.fabricmc.loader.impl.launch.knot.KnotClient".into()),
            assets: None,
            asset_index: None,
            downloads: None,
            java_version: None,
            libraries: vec![lib("org.ow2.asm:asm:9.6"), lib("net.fabricmc:fabric-loader:0.15.0")],
            arguments: Some(Arguments {
                game: Vec::new(),
                jvm: vec![ArgValue::Plain("-DFabricMcEmu=net.minecraft.client.main.Main".into())],
                default_user_jvm: Vec::new(),
            }),
            minecraft_arguments: None,
            release_time: None,
            kind: None,
            inherits_from: Some("1.20.1".into()),
            logging: None,
        }
    }

    #[test]
    fn child_main_class_wins() {
        let m = merge(&base(), &child());
        assert_eq!(
            m.main_class.as_deref(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        assert_eq!(m.id, "fabric-loader-0.15.0-1.20.1");
    }

    #[test]
    fn library_dedup_keeps_parent_position() {
        let m = merge(&base(), &child());
        // asm 被子的版本覆盖，但位置仍是父里的第一个
        assert_eq!(m.libraries[0].name, "org.ow2.asm:asm:9.6");
        assert_eq!(m.libraries[1].name, "com.mojang:brigadier:1.0.18");
        assert_eq!(m.libraries[2].name, "net.fabricmc:fabric-loader:0.15.0");
        assert_eq!(m.libraries.len(), 3);
    }

    #[test]
    fn args_parent_before_child() {
        let m = merge(&base(), &child());
        let args = m.arguments.unwrap();
        assert_eq!(args.jvm.len(), 1);
        assert_eq!(args.game.len(), 1);
        // 父的 --username 必须还在
        match &args.game[0] {
            ArgValue::Plain(s) => assert_eq!(s, "--username"),
            _ => panic!("预期是纯字符串"),
        }
    }

    #[test]
    fn resource_and_java_inherit_from_parent() {
        let m = merge(&base(), &child());
        // 先取引用再比对：asset_index 是 Option<AssetIndex>，unwrap() 会把值移走，
        // 后面再调 java_major() 借 m 就是「borrow after partial move」
        assert_eq!(m.asset_index.as_ref().unwrap().id, "5");
        assert_eq!(m.assets.as_deref(), Some("5"));
        assert_eq!(m.java_major(), Some(17));
    }

    #[test]
    fn lib_key_strips_version_and_classifier() {
        assert_eq!(lib_key("org.lwjgl:lwjgl:3.3.3:natives-windows"), "org.lwjgl:lwjgl");
        assert_eq!(lib_key("de.oceanlabs.mcp:mcp:1.0@zip"), "de.oceanlabs.mcp:mcp");
    }

    #[test]
    fn artifact_type_kept_for_downloads() {
        // 父版本没写 downloads，子版本有就必须保留，否则 client.jar 没有地址
        let mut c = child();
        c.downloads = Some(crate::minecraft::version_json::Downloads {
            client: Some(Artifact {
                id: None,
                path: None,
                url: Some("https://example.invalid/client.jar".into()),
                sha1: None,
                size: None,
            }),
            server: None,
        });
        let m = merge(&base(), &c);
        assert_eq!(
            m.downloads.unwrap().client.unwrap().url.as_deref(),
            Some("https://example.invalid/client.jar")
        );
    }
}