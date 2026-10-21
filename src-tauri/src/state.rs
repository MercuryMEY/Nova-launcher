use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::fsutil;

/// 配置结构版本号，用于后续迁移
pub const CONFIG_VERSION: u32 = 3;

/// 版本隔离模式，对应 PCL2 的三种语义
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationMode {
    Full,
    None,
    NonFake,
}

impl Default for IsolationMode {
    fn default() -> Self {
        IsolationMode::NonFake
    }
}

/// 全屏策略。原版没有「启动参数」可以直接控制全屏，只能在 options.txt 里改，
/// 所以默认值必须是 Ignore：不替用户做决定，也不覆盖他在游戏里自己的选择
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FullscreenMode {
    /// 不碰 options.txt
    Ignore,
    /// 启动前把 fullscreen 写成 true
    On,
    /// 启动前把 fullscreen 写成 false
    Off,
}

impl Default for FullscreenMode {
    fn default() -> Self {
        FullscreenMode::Ignore
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub max_memory_mb: u32,
    pub java_path: Option<String>,
    /// 游戏分辨率（拼进 --width / --height）。历史原因字段名是 window_*
    pub window_width: u32,
    pub window_height: u32,
    /// 旧字段：只用于迁移到 fullscreen_mode，界面不再展示
    #[serde(default)]
    pub fullscreen: bool,
    /// 游戏启动时是否全屏。Ignore 表示不去改 options.txt
    pub fullscreen_mode: FullscreenMode,
    /// 启动器窗口本身的尺寸（与游戏分辨率无关）
    pub launcher_width: u32,
    pub launcher_height: u32,
    /// 启动器窗口是否最大化启动
    pub launcher_maximized: bool,
    pub isolation: IsolationMode,
    pub theme: String,
    pub accent: String,
    pub mirror_timeout_secs: u64,
    pub prefer_mirror: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            max_memory_mb: 2048,
            java_path: None,
            window_width: 1280,
            window_height: 720,
            fullscreen: false,
            fullscreen_mode: FullscreenMode::default(),
            launcher_width: 1180,
            launcher_height: 760,
            launcher_maximized: false,
            isolation: IsolationMode::default(),
            theme: "dark".into(),
            accent: "blue".into(),
            mirror_timeout_secs: 4,
            prefer_mirror: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AccountKind {
    Microsoft,
    Offline,
}

/// 离线账户 UUID 的生成策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UuidMode {
    /// 按 `OfflinePlayer:<名字>` 派生，与离线服务器算出来的一致
    NameBased,
    /// 随机 v4，改名字不会丢存档
    Random,
}

impl Default for UuidMode {
    fn default() -> Self {
        UuidMode::NameBased
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub kind: AccountKind,
    pub name: String,
    pub uuid: String,
    /// 旧配置文件里没有这个字段，serde(default) 兜底为 name_based
    #[serde(default)]
    pub uuid_mode: UuidMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub settings: Settings,
    pub accounts: Vec<Account>,
    pub current_account_id: Option<String>,
    pub minecraft_dir: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            settings: Settings::default(),
            accounts: Vec::new(),
            current_account_id: None,
            minecraft_dir: None,
        }
    }
}

impl AppConfig {
    /// 旧配置兼容与兜底校验。所有非法值都在这里拉回安全区间。
    pub fn migrate(mut self) -> Self {
        if self.version < CONFIG_VERSION {
            // 未来版本在这里做字段重命名/换算
            self.version = CONFIG_VERSION;
        }
        self.settings.max_memory_mb = self.settings.max_memory_mb.clamp(512, 65536);
        if self.settings.mirror_timeout_secs == 0 {
            self.settings.mirror_timeout_secs = 4;
        }
        if self.settings.window_width < 640 {
            self.settings.window_width = 1280;
        }
        if self.settings.window_height < 480 {
            self.settings.window_height = 720;
        }
        // v3：旧配置只有一个 fullscreen 开关，语义是「启动就全屏」
        if self.settings.fullscreen {
            self.settings.fullscreen_mode = FullscreenMode::On;
            self.settings.fullscreen = false;
        }
        if self.settings.launcher_width < 640 {
            self.settings.launcher_width = 1180;
        }
        if self.settings.launcher_height < 480 {
            self.settings.launcher_height = 760;
        }
        if !matches!(self.settings.theme.as_str(), "light" | "dark" | "system") {
            self.settings.theme = "dark".into();
        }
        // v2：账户多了 uuid_mode 字段，同时把残缺数据拉回可用状态
        self.accounts.retain(|a| !a.name.trim().is_empty());
        let mut seen: Vec<String> = Vec::new();
        self.accounts.retain(|a| {
            if seen.contains(&a.id) {
                false
            } else {
                seen.push(a.id.clone());
                true
            }
        });
        for acc in &mut self.accounts {
            let name = acc.name.trim().to_string();
            acc.name = name.clone();
            let valid_uuid = acc.uuid.len() == 32
                && acc.uuid.chars().all(|c| c.is_ascii_hexdigit());
            if !valid_uuid {
                acc.uuid = crate::account::offline::uuid_for(&name, acc.uuid_mode)
                    .simple()
                    .to_string();
            }
        }
        if let Some(id) = &self.current_account_id {
            if !self.accounts.iter().any(|a| &a.id == id) {
                self.current_account_id = None;
            }
        }
        self
    }

    /// 当前选中的账户，找不到时返回 None
    pub fn current_account(&self) -> Option<&Account> {
        let id = self.current_account_id.as_deref()?;
        self.accounts.iter().find(|a| a.id == id)
    }
}

pub struct AppState {
    pub http: reqwest::blocking::Client,
    pub config: Mutex<AppConfig>,
    /// 启动阶段的非致命错误，前端展示为一条提示
    pub init_error: Option<String>,
    /// 当前这一次游戏进程。退出后仍保留句柄，方便前端回看退出码
    pub game: Mutex<Option<crate::launch::proc::RunningGame>>,
    /// 启动流程占位：从点下按钮到进程真正起来这段时间也算「忙」。
    ///
    /// 用 AtomicBool 而不是 Mutex<bool>：async 命令里跨 await 持 guard 不是 Send
    pub launching: Arc<AtomicBool>,
}

impl AppState {
    pub fn load() -> Self {
        let mut init_error = None;

        let http = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("NovaLauncher/", env!("CARGO_PKG_VERSION")))
            .build()
        {
            Ok(c) => c,
            Err(_) => {
                init_error = Some("网络组件初始化失败，部分在线功能将不可用".to_string());
                reqwest::blocking::Client::new()
            }
        };

        let config = match fsutil::read_json::<AppConfig>(&fsutil::config_path()) {
            Ok(Some(c)) => c.migrate(),
            Ok(None) => AppConfig::default(),
            Err(e) => {
                init_error = Some(format!("配置文件损坏，已重置为默认设置：{e}"));
                AppConfig::default()
            }
        };

        Self {
            http,
            config: Mutex::new(config),
            init_error,
            game: Mutex::new(None),
            launching: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 取配置快照，避免长时间持锁
    pub fn snapshot(&self) -> AppConfig {
        self.config.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 先落盘、成功后才替换内存状态，避免写盘失败但界面已显示新值
    pub fn update<F: FnOnce(&mut AppConfig) -> AppResult<()>>(&self, f: F) -> AppResult<()> {
        let mut next = self.snapshot();
        f(&mut next)?;
        let next = next.migrate();
        fsutil::write_json_atomic(&fsutil::config_path(), &next)?;
        *self.config.lock().unwrap_or_else(|e| e.into_inner()) = next;
        Ok(())
    }

    /// 当前生效的 .minecraft 根目录
    pub fn minecraft_root(&self) -> std::path::PathBuf {
        let cfg = self.snapshot();
        match cfg.minecraft_dir {
            Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
            _ => fsutil::default_minecraft_dir(),
        }
    }
}
