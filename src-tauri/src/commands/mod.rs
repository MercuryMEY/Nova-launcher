pub mod account;
pub mod crash;
pub mod install;
pub mod java;
pub mod launch;
pub mod loader;
pub mod modpack;
pub mod mods;
pub mod microsoft;
pub mod version;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::{AppConfig, AppState, Settings};

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub minecraft_dir: String,
    pub data_dir: String,
}

#[tauri::command]
pub fn ping() -> String {
    "pong".into()
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        minecraft_dir: state.minecraft_root().to_string_lossy().into_owned(),
        data_dir: crate::fsutil::data_dir().to_string_lossy().into_owned(),
    }
}

/// 启动阶段的非致命错误，前端拿到后弹一条可关闭的提示
#[tauri::command]
pub fn get_init_error(state: State<'_, AppState>) -> Option<String> {
    state.init_error.clone()
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> AppConfig {
    state.snapshot()
}

/// 把设置里的启动器窗口尺寸应用到主窗口。
///
/// 拆出来是因为两个地方要用：启动时（setup）和每次保存设置后。
/// 失败只记日志不动界面 —— 窗口尺寸不是关键功能，报错反而更烦人
pub fn apply_window(app: &AppHandle, s: &Settings) {
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    if s.launcher_maximized {
        let _ = win.maximize();
        return;
    }
    let _ = win.unmaximize();
    let _ = win.set_size(tauri::LogicalSize::new(s.launcher_width, s.launcher_height));
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<()> {
    let snapshot = settings.clone();
    state.update(|cfg| {
        cfg.settings = settings;
        Ok(())
    })?;
    // 落盘成功之后才动窗口：否则用户看到窗口变了、重启又变回去
    apply_window(&app, &snapshot);
    Ok(())
}

/// URL 里允许出现的字符：RFC3986 的 unreserved + reserved + `%`（已编码）
///
/// 刻意不含单引号与分号：它们在 URL 里合法，但各平台 shell 对它们的处理不一致，
/// 而微软的授权链接里也不会出现，直接拒掉最省心。`&` 必须放行——查询串到处都是
const URL_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~:/?#[]@!$&()*+,=%";

/// 链接能不能交给系统打开
///
/// 不只是看前缀：`cmd /C start` 会把参数再解析一遍，`&`、`"`、`|` 这类字符
/// 能改掉命令结构，所以做一层字符白名单。这是纵深防御——目前的调用方
/// 都是自己拼的微软授权地址，但不该依赖这一点
fn is_safe_url(url: &str) -> bool {
    if url.len() > 2048 {
        return false;
    }
    let rest = match url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        Some(r) => r,
        None => return false,
    };
    // 必须有主机名：拒绝 http:///x 这种
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return false;
    }
    url.chars().all(|c| URL_CHARS.contains(c))
}

/// 用系统默认浏览器打开链接。
///
/// 不走 opener 插件：它的权限集在不同小版本里改过名，多一个 capability 白名单
/// 就多一处会随依赖升级而失效的地方；直接调系统命令，行为在各平台都可预期。
#[tauri::command]
pub fn open_external(url: String) -> AppResult<()> {
    // 只放行 http/https：前端传进来的东西不能变成 file:// 或任意本地命令
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(AppError::Message("只支持打开 http/https 链接".into()));
    }
    if !is_safe_url(&url) {
        return Err(AppError::Message(
            "链接里有不安全字符，已拒绝打开".into(),
        ));
    }

    #[cfg(target_os = "windows")]
    let mut cmd = {
        // 不用 `cmd /C start`：那会把 URL 再交回命令解释器解析一遍，
        // 查询串里的 `&` 会被当成命令分隔符。rundll32 直接走 ShellExecute，
        // 参数不经过 shell，`&` 在这里是安全的
        let mut c = std::process::Command::new("rundll32.exe");
        c.args(["url.dll,FileProtocolHandler", &url]);
        c
    };

    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(&url);
        c
    };

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&url);
        c
    };

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| AppError::Message(format!("打不开浏览器：{e}，请手动访问 {url}")))
}

#[cfg(test)]
mod tests {
    use super::is_safe_url;

    #[test]
    fn accepts_normal_links() {
        assert!(is_safe_url("https://login.microsoftonline.com/common/oauth2/v2.0/devicecode"));
        assert!(is_safe_url("https://www.microsoft.com/link"));
        // 查询串里的 & 与 = 是正常的，不能拒
        assert!(is_safe_url("https://a.example.com/x?y=1&z=2"));
        assert!(is_safe_url("http://127.0.0.1:8080/"));
    }

    #[test]
    fn rejects_shell_specials() {
        assert!(!is_safe_url("https://a.com/\"&calc&\""));
        assert!(!is_safe_url("https://a.com/x|dir"));
        assert!(!is_safe_url("https://a.com/x`id`"));
        assert!(!is_safe_url("https://a.com/x y"));
        assert!(!is_safe_url("https://a.com/x\nrm -rf /"));
        assert!(!is_safe_url("https://a.com/x\\y"));
        assert!(!is_safe_url("https://a.com/x;y"));
        assert!(!is_safe_url("https://a.com/x'y"));
    }

    #[test]
    fn rejects_other_schemes() {
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("cmd://calc"));
    }

    #[test]
    fn rejects_empty_host_and_overlong() {
        assert!(!is_safe_url("https:///path"));
        assert!(!is_safe_url("http://?x=1"));
        let long = format!("https://a.com/{}", "a".repeat(3000));
        assert!(!is_safe_url(&long));
    }
}
