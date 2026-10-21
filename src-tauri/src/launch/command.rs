//! 启动参数的唯一装配点。
//!
//! 预览（commands::version::preview_version_args）和真实启动都调 build_plan，
//! 否则两套拼参数逻辑迟早会漂移——界面上看到的和真正喂给 JVM 的对不上。
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::minecraft::arguments;
use crate::minecraft::classpath;
use crate::minecraft::inherit;
use crate::minecraft::isolate;
use crate::minecraft::rules::{Features, Platform};
use crate::minecraft::version_json::VersionJson;
use crate::state::{AccountKind, AppConfig};

/// 老版本 JSON 里没有 javaVersion，官方默认就是 Java 8
pub const DEFAULT_JAVA_MAJOR: u32 = 8;

#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub id: String,
    pub main_class: String,
    pub java_major: u32,
    /// 版本 JSON 里声明的 Java 主版本；没写就是 None（展示用）
    pub java_declared: Option<u32>,
    /// 版本 JSON 点名的运行时组件，例如 `java-runtime-delta`
    pub java_component: Option<String>,
    pub asset_index: String,
    pub game_dir: PathBuf,
    pub natives_dir: PathBuf,
    pub client_jar: PathBuf,
    pub library_count: usize,
    pub native_count: usize,
    pub missing_files: Vec<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
}

impl LaunchPlan {
    /// 完整参数（不含 java 可执行文件本身）
    pub fn command_args(&self) -> Vec<String> {
        let mut argv = self.jvm_args.clone();
        argv.push(self.main_class.clone());
        argv.extend(self.game_args.iter().cloned());
        argv
    }

    /// 拉起进程前的硬性检查：宁可在这里拦住，也别让 JVM 报 ClassNotFoundException
    ///
    /// 解压 natives **之前**能查的：client.jar 与依赖库
    ///
    /// native 目录不能在这里查 —— 首次启动时它天然是空的，解压发生在后面，
    /// 查了就会出现「自己把自己拦住」的死循环（B11 的回归）
    pub fn precheck_core(&self) -> AppResult<()> {
        if !self.client_jar.is_file() {
            return Err(AppError::VersionInvalid(format!(
                "缺少 client.jar，请先补全 {} 的客户端文件",
                self.id
            )));
        }
        if !self.missing_files.is_empty() {
            // missing_files 是在 build_plan 里顺手收集的，最多 20 条
            let names: Vec<String> = self
                .missing_files
                .iter()
                .take(3)
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
                .collect();
            return Err(AppError::VersionInvalid(format!(
                "缺少 {} 个依赖库（{} 等），请到下载页对 {} 点「修复安装」",
                self.missing_files.len(),
                names.join("、"),
                self.id
            )));
        }
        Ok(())
    }

    /// 解压 natives **之后**才查的：目录必须非空
    ///
    /// 只有带 native 依赖的版本才需要这个目录（1.19 之后基本都在 jar 里了）
    pub fn precheck_natives(&self) -> AppResult<()> {
        if self.native_count == 0 {
            return Ok(());
        }
        let has_file = std::fs::read_dir(&self.natives_dir)
            .map(|mut it| it.next().is_some())
            .unwrap_or(false);
        if !has_file {
            return Err(AppError::VersionInvalid(format!(
                "{} 的 native 目录是空的，请到下载页点「修复安装」",
                self.id
            )));
        }
        Ok(())
    }
}

pub fn load_version(root: &Path, id: &str) -> AppResult<VersionJson> {
    // 走继承合并：加载器版本的库、参数、资源全靠父版本补齐
    inherit::load_with_parent(root, id)
}

/// 特性开关：demo 与 quickPlay 一律关掉，自定义分辨率交给设置
pub fn default_features() -> Features {
    Features {
        demo: false,
        custom_resolution: true,
        quick_play: false,
    }
}

/// 真正喂给 JVM 的身份信息。微软账户的 access token 由调用方先取好，
/// 这里只负责拼参数，不碰网络（预览也能安全调用）。
pub struct AuthTokens {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
}

impl AuthTokens {
    /// 预览用：不打网络。微软账户照样标成 msa，参数形态才是真的
    pub fn placeholder(cfg: &AppConfig) -> Self {
        match cfg.current_account() {
            Some(a) => Self {
                name: a.name.clone(),
                uuid: a.uuid.clone(),
                access_token: "0".into(),
                user_type: if a.kind == AccountKind::Microsoft {
                    "msa"
                } else {
                    "legacy"
                }
                .into(),
            },
            None => Self {
                name: "Player".into(),
                uuid: "00000000000000000000000000000000".into(),
                access_token: "0".into(),
                user_type: "legacy".into(),
            },
        }
    }
}

/// 预览入口：不联网，直接用占位身份
pub fn build_plan(root: &Path, cfg: &AppConfig, id: &str) -> AppResult<LaunchPlan> {
    let auth = AuthTokens::placeholder(cfg);
    build_plan_with_auth(root, cfg, id, &auth, None)
}

/// 启动入口：真实身份由调用方取好（微软账户要先刷新令牌）。
///
/// `game_dir` 是给整合包实例用的覆盖值：版本目录（JSON / 加载器 / 库）仍按 `id` 找，
/// 只是把「存档、Mod、配置」那层指到 `instances/<id>/` 去。
pub fn build_plan_with_auth(
    root: &Path,
    cfg: &AppConfig,
    id: &str,
    auth: &AuthTokens,
    game_dir: Option<&Path>,
) -> AppResult<LaunchPlan> {
    let version = load_version(root, id)?;
    let platform = Platform::current();
    let features = default_features();

    let lib_dir = isolate::libraries_dir(root);
    let libs = classpath::collect(&version, &platform, &features);
    let client_jar = isolate::jar_path(root, id);
    let cp = classpath::classpath_string(&libs, &lib_dir, &client_jar);

    let mut native_count = 0usize;
    let mut missing = Vec::new();
    for e in libs.iter() {
        if e.native {
            native_count += 1;
        }
        if missing.len() < 20 && !lib_dir.join(&e.path).is_file() {
            missing.push(e.path.clone());
        }
    }

    let game_dir = match game_dir {
        Some(dir) => dir.to_path_buf(),
        None => isolate::game_dir(root, id, cfg.settings.isolation),
    };
    // 必须用 native_extract_dir：26.3 的 -Djava.library.path 写死了后面的 /java
    let natives = isolate::native_extract_dir(root, id);
    let assets_root = isolate::assets_dir(root);

    let asset_index = version
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| version.assets.clone())
        .unwrap_or_else(|| "legacy".to_string());

    let mut vars: HashMap<String, String> = HashMap::new();
    let pairs: [(&str, String); 20] = [
        ("auth_player_name", auth.name.clone()),
        ("version_name", id.to_string()),
        ("game_directory", game_dir.to_string_lossy().into_owned()),
        ("assets_root", assets_root.to_string_lossy().into_owned()),
        ("game_assets", assets_root.to_string_lossy().into_owned()),
        ("assets_index_name", asset_index.clone()),
        ("auth_uuid", auth.uuid.clone()),
        ("auth_access_token", auth.access_token.clone()),
        ("auth_session", "0".to_string()),
        ("user_type", auth.user_type.clone()),
        ("user_properties", "{}".to_string()),
        ("natives_directory", natives.to_string_lossy().into_owned()),
        ("launcher_name", arguments::BRAND.to_string()),
        ("launcher_version", env!("CARGO_PKG_VERSION").to_string()),
        ("classpath", cp),
        ("classpath_separator", if cfg!(windows) { ";" } else { ":" }.to_string()),
        ("library_directory", lib_dir.to_string_lossy().into_owned()),
        ("version_type", version.kind.clone().unwrap_or_else(|| "release".to_string())),
        // 真实启动时用设置里的窗口尺寸，不再写死 854x480
        ("resolution_width", cfg.settings.window_width.to_string()),
        ("resolution_height", cfg.settings.window_height.to_string()),
    ];
    for (k, v) in pairs {
        vars.insert(k.to_string(), v);
    }

    let (jvm_args, game_args) = arguments::build(&version, &platform, &features, &vars);
    // 1.12 起官方用 log4j 配置文件：装了就得在 JVM 参数里指过去，
    // 否则日志布局退回默认，行为与官方启动器不一致
    let mut jvm_args = jvm_args;
    if let Some(name) = logging_file_name(&version) {
        let cfg_path = assets_root.join("log_configs").join(name);
        if cfg_path.is_file() {
            // 参数名官方是写在 logging.client.argument 里的（-Dlog4j.configurationFile），
            // 以前硬编码，遇到用别的键的版本就对不上了
            let arg = logging_arg(&version);
            jvm_args.push(format!("{arg}={}", cfg_path.to_string_lossy()));
        }
    }

    Ok(LaunchPlan {
        id: id.to_string(),
        main_class: version.main_class_or_default(),
        java_major: version.java_major().unwrap_or(DEFAULT_JAVA_MAJOR),
        java_declared: version.java_major(),
        java_component: version
            .java_version
            .as_ref()
            .and_then(|j| j.component.clone()),
        asset_index,
        game_dir,
        natives_dir: natives,
        client_jar,
        library_count: libs.len(),
        native_count,
        missing_files: missing,
        jvm_args,
        game_args,
    })
}

/// log4j 的 JVM 参数名。官方写在 logging.client.argument，
/// 老版本 JSON 里可能没有，退回官方的默认值
fn logging_arg(version: &crate::minecraft::version_json::VersionJson) -> String {
    version
        .logging
        .as_ref()
        .and_then(|l| l.client.as_ref())
        .and_then(|c| c.argument.clone())
        .filter(|s| s.starts_with("-D"))
        .unwrap_or_else(|| "-Dlog4j.configurationFile".to_string())
}

/// log4j 配置文件名：官方有时只给 url，就取最后一段
fn logging_file_name(version: &crate::minecraft::version_json::VersionJson) -> Option<String> {
    let f = version.logging.as_ref()?.client.as_ref()?.file.as_ref()?;
    f.id.clone().or_else(|| {
        f.url
            .as_ref()
            .and_then(|u| u.rsplit('/').next().map(|s| s.to_string()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("nova-precheck-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// 造一个「什么都不缺」的 plan，各用例再单独挖一个洞
    fn plan(dir: &Path) -> LaunchPlan {
        let jar = dir.join("client.jar");
        fs::write(&jar, b"x").unwrap();
        let natives = dir.join("natives");
        fs::create_dir_all(&natives).unwrap();
        LaunchPlan {
            id: "1.20.1".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            java_major: 17,
            java_declared: Some(17),
            java_component: None,
            asset_index: "17".into(),
            game_dir: dir.to_path_buf(),
            natives_dir: natives,
            client_jar: jar,
            library_count: 0,
            native_count: 0,
            missing_files: Vec::new(),
            jvm_args: Vec::new(),
            game_args: Vec::new(),
        }
    }

    #[test]
    fn passes_when_everything_is_there() {
        let dir = temp_dir();
        assert!(plan(&dir).precheck_core().is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_client_jar() {
        let dir = temp_dir();
        let p = plan(&dir);
        let _ = fs::remove_file(&p.client_jar);
        let e = p.precheck_core().unwrap_err().to_string();
        assert!(e.contains("client.jar"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_libraries() {
        let dir = temp_dir();
        let mut p = plan(&dir);
        p.missing_files = vec!["org/lwjgl/lwjgl.jar".into(), "com/a/b.jar".into()];
        let e = p.precheck_core().unwrap_err().to_string();
        assert!(e.contains("依赖库"), "{e}");
        // 文案里应带上数量与前几个文件名
        assert!(e.contains("2"), "{e}");
        assert!(e.contains("lwjgl.jar"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_empty_natives_only_when_needed() {
        let dir = temp_dir();
        let mut p = plan(&dir);
        // 没有 native 依赖时，空目录不算问题
        assert!(p.precheck_natives().is_ok());

        p.native_count = 3;
        let e = p.precheck_natives().unwrap_err().to_string();
        assert!(e.contains("native"), "{e}");

        // 目录里有文件就放行
        fs::write(p.natives_dir.join("lwjgl.dll"), b"x").unwrap();
        assert!(p.precheck_natives().is_ok());
        let _ = fs::remove_dir_all(&dir);
    }
}
