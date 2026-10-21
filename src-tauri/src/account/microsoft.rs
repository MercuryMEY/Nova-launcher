//! 微软账户登录（OAuth 设备码流程）。
//!
//! 为什么用设备码而不是回调：启动器是桌面程序，等回调要起本地 HTTP 服务、
//! 还要处理端口占用与防火墙，而设备码只需要用户在浏览器输一个 8 位码，
//! 对跨平台（Windows/macOS/Linux）也最友好。
//!
//! 认证链是本文件的核心：
//! 微软授权码 → Xbox Live 令牌 → XSTS 令牌 → Minecraft 令牌 → 玩家档案
//! 每一环都会给下一环当输入，断在哪一环就要给出对应的可读提示。
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::account::secret;
use crate::error::{AppError, AppResult};

const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const XBL_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MC_LOGIN_URL: &str = "https://api.minecraftservices.com/authentication/login_with_xbox";
const MC_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";
const SCOPE: &str = "XboxLive.signin offline_access";

/// 公开的启动器应用 ID。这不是密钥（设备码流程不需要 client secret），
/// 想换成自己注册的 Azure 应用时，设环境变量 NOVA_MS_CLIENT_ID 即可。
pub const DEFAULT_CLIENT_ID: &str = "c36a9fb6-4f2a-41ff-90bd-ae7cc92031eb";

/// 单次 HTTP 给 20 秒：微软登录端点在国外，4 秒的镜像超时必然不够
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

pub fn client_id() -> String {
    std::env::var("NOVA_MS_CLIENT_ID")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_CLIENT_ID.to_string())
}

/// 设备码票据。`device_code` 不对外暴露，只留在后端等轮询用。
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeTicket {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub message: String,
    /// 票据寿命（秒）
    #[serde(default = "default_expires")]
    pub expires_in: u64,
    /// 官方建议的轮询间隔（秒），太急会被判定为 slow_down
    #[serde(default = "default_interval")]
    pub interval: u64,
}

fn default_expires() -> u64 {
    900
}

fn default_interval() -> u64 {
    5
}

/// 轮询一次的结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollOutcome {
    /// 用户还没在浏览器里确认
    Pending,
    /// 请求太密，官方要求拉长间隔
    SlowDown,
    /// 用户点了拒绝
    Denied,
    /// 票据过期，得重新生成
    Expired,
    /// 成功，带着微软的 tokens
    Done {
        ms_access: String,
        refresh: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct TokenBody {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

/// 把 token 端点的响应翻成轮询结果。抽成纯函数是为了能单测错误分支。
pub fn read_token_body(status: u16, body: &str) -> AppResult<PollOutcome> {
    let parsed: TokenBody = serde_json::from_str(body)
        .map_err(|e| AppError::Message(format!("微软返回了看不懂的内容：{e}")))?;

    if status == 200 {
        let ms_access = parsed
            .access_token
            .ok_or_else(|| AppError::Message("微软没有返回 access_token".into()))?;
        let refresh = parsed
            .refresh_token
            .ok_or_else(|| AppError::Message("微软没有返回 refresh_token".into()))?;
        return Ok(PollOutcome::Done { ms_access, refresh });
    }

    match parsed.error.as_deref() {
        Some("authorization_pending") => Ok(PollOutcome::Pending),
        Some("slow_down") => Ok(PollOutcome::SlowDown),
        Some("authorization_declined") | Some("access_denied") => Ok(PollOutcome::Denied),
        Some("expired_token") | Some("code_expired") => Ok(PollOutcome::Expired),
        Some("bad_verification_code") => Ok(PollOutcome::Expired),
        other => Err(AppError::Message(format!(
            "微软登录失败（{}）：{}",
            other.unwrap_or("未知错误"),
            parsed
                .error_description
                .unwrap_or_else(|| "没有更多信息".into())
        ))),
    }
}

/// Xbox 会让玩家去 xbox.com 建号或处理家庭设置，这些码必须翻成人话
pub fn xsts_hint(xerr: &str) -> String {
    match xerr {
        "2148916233" => "这个微软账户还没有 Xbox 账号，请先去 xbox.com 建一个（免费）".into(),
        "2148916235" => "这个微软账户所在地区不支持 Xbox 服务".into(),
        "2148916236" | "2148916237" => "这个微软账户需要先完成年龄验证".into(),
        "2148916238" => "这个账户被判定为未成年人，需要家长在家庭组里同意".into(),
        other => format!("Xbox 认证被拒绝（{other}）"),
    }
}

/// 玩家的 Minecraft 档案
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
}

impl Profile {
    /// 官方给的是 32 位无横线 uuid，离线账户也用同一个形态，保持一致
    pub fn uuid(&self) -> String {
        self.id.replace('-', "").to_lowercase()
    }
}

/// 一次可用的登录态（只在内存里，refresh token 才落凭据库）
#[derive(Debug, Clone)]
pub struct Session {
    pub access_token: String,
    pub name: String,
    pub uuid: String,
    pub expires_at: Instant,
}

impl Session {
    pub fn is_fresh(&self) -> bool {
        // 提前一分钟判过期，避免卡在边界上把过期 token 喂给游戏
        self.expires_at > Instant::now() + Duration::from_secs(60)
    }
}

fn cache() -> &'static Mutex<HashMap<String, Session>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn forget(account_id: &str) {
    if let Ok(mut m) = cache().lock() {
        m.remove(account_id);
    }
    secret::delete(account_id);
}

// ---------- 网络部分 ----------

fn post_json(
    client: &reqwest::blocking::Client,
    url: &str,
    body: serde_json::Value,
) -> AppResult<(u16, String)> {
    let resp = client
        .post(url)
        .timeout(HTTP_TIMEOUT)
        .json(&body)
        .send()
        .map_err(|e| AppError::Message(format!("连不上微软登录服务：{e}")))?;
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    Ok((status, text))
}

fn get_json(
    client: &reqwest::blocking::Client,
    url: &str,
    bearer: &str,
) -> AppResult<(u16, String)> {
    let resp = client
        .get(url)
        .timeout(HTTP_TIMEOUT)
        .bearer_auth(bearer)
        .send()
        .map_err(|e| AppError::Message(format!("连不上 Minecraft 服务：{e}")))?;
    let status = resp.status().as_u16();
    let text = resp.text().unwrap_or_default();
    Ok((status, text))
}

/// 官方错误响应有时是一整页 HTML，直接椎给界面太长
fn brief(text: &str) -> String {
    let t = text.trim();
    let cut: String = t.chars().take(160).collect();
    if t.chars().count() > 160 {
        format!("{cut}…")
    } else {
        cut
    }
}

/// 私有的解析小工具：字段缺失就返回 None，由调用方拼提示
fn text_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}

pub fn request_device_code(
    client: &reqwest::blocking::Client,
) -> AppResult<DeviceCodeTicket> {
    let body = serde_json::json!({
        "client_id": client_id(),
        "scope": SCOPE,
    });
    let (status, text) = post_json(client, DEVICE_CODE_URL, body)?;
    if status != 200 {
        return Err(AppError::Message(format!(
            "申请登录码失败（HTTP {status}）：{}",
            brief(&text)
        )));
    }
    serde_json::from_str::<DeviceCodeTicket>(&text)
        .map_err(|e| AppError::Message(format!("登录码响应解析失败：{e}")))
}

/// 只轮一次，间隔由前端控制：后端不帮用户等着，界面才能随时取消
pub fn poll_device_once(
    client: &reqwest::blocking::Client,
    ticket: &DeviceCodeTicket,
) -> AppResult<PollOutcome> {
    let body = serde_json::json!({
        "client_id": client_id(),
        "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        "device_code": ticket.device_code,
    });
    let (status, text) = post_json(client, TOKEN_URL, body)?;
    read_token_body(status, &text)
}

pub fn refresh_tokens(client: &reqwest::blocking::Client, refresh: &str) -> AppResult<PollOutcome> {
    let body = serde_json::json!({
        "client_id": client_id(),
        "scope": SCOPE,
        "grant_type": "refresh_token",
        "refresh_token": refresh,
    });
    let (status, text) = post_json(client, TOKEN_URL, body)?;
    read_token_body(status, &text)
}

/// 微软 access token → Xbox Live 令牌，带出用户哈希（uhs）
fn xbl_authenticate(client: &reqwest::blocking::Client, ms_access: &str) -> AppResult<(String, String)> {
    let body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={ms_access}"),
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT",
    });
    let (status, text) = post_json(client, XBL_URL, body)?;
    if status != 200 {
        return Err(AppError::Message(format!(
            "Xbox Live 认证失败（HTTP {status}）：{}",
            brief(&text)
        )));
    }
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let token = text_field(&v, "Token")
        .ok_or_else(|| AppError::Message("Xbox Live 没有返回令牌".into()))?;
    let uhs = v
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|x| x.as_str())
        .ok_or_else(|| AppError::Message("Xbox Live 没有返回用户哈希".into()))?
        .to_string();
    Ok((token, uhs))
}

/// Xbox Live 令牌 → XSTS 令牌。XR 系列错误码就在这里出
fn xsts_authorize(client: &reqwest::blocking::Client, xbl: &str) -> AppResult<String> {
    let body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl],
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT",
    });
    let (status, text) = post_json(client, XSTS_URL, body)?;
    if status != 200 {
        // 这一层失败几乎都是账户侧问题，要把 XErr 翻成人话
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let xerr = v.get("XErr").map(|x| x.to_string()).unwrap_or_default();
        let xerr = xerr.trim_matches('"').to_string();
        if xerr.is_empty() {
            return Err(AppError::Message(format!(
                "XSTS 认证失败（HTTP {status}）：{}",
                brief(&text)
            )));
        }
        return Err(AppError::Message(xsts_hint(&xerr)));
    }
    let v: serde_json::Value = serde_json::from_str(&text)?;
    text_field(&v, "Token").ok_or_else(|| AppError::Message("XSTS 没有返回令牌".into()))
}

/// XSTS → Minecraft 登录令牌
fn minecraft_login(
    client: &reqwest::blocking::Client,
    uhs: &str,
    xsts: &str,
) -> AppResult<(String, Instant)> {
    let body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={uhs};{xsts}"),
    });
    let (status, text) = post_json(client, MC_LOGIN_URL, body)?;
    if status == 401 || status == 403 {
        return Err(AppError::Message(
            "这个微软账户没有买过 Minecraft，或者档案被限制了".into(),
        ));
    }
    if status != 200 {
        return Err(AppError::Message(format!(
            "Minecraft 登录失败（HTTP {status}）：{}",
            brief(&text)
        )));
    }
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let token = text_field(&v, "access_token")
        .ok_or_else(|| AppError::Message("Minecraft 没有返回令牌".into()))?;
    let secs = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(86_400);
    Ok((token, Instant::now() + Duration::from_secs(secs)))
}

/// 拉玩家档案：没有档案时官方给 404，得提醒去建一次
fn fetch_profile(client: &reqwest::blocking::Client, mc_token: &str) -> AppResult<Profile> {
    let (status, text) = get_json(client, MC_PROFILE_URL, mc_token)?;
    if status == 404 {
        return Err(AppError::Message(
            "这个账户还没有 Minecraft 档案，请用官方启动器登录一次再回来".into(),
        ));
    }
    if status == 401 {
        return Err(AppError::Message("Minecraft 令牌无效，请重新登录".into()));
    }
    if status != 200 {
        return Err(AppError::Message(format!(
            "读取玩家档案失败（HTTP {status}）：{}",
            brief(&text)
        )));
    }
    serde_json::from_str::<Profile>(&text)
        .map_err(|e| AppError::Message(format!("玩家档案解析失败：{e}")))
}

/// 走完剩下的链条，并把登录态写入内存缓存
pub fn complete_login(
    client: &reqwest::blocking::Client,
    account_id: &str,
    refresh: String,
    ms_access: &str,
) -> AppResult<Session> {
    let (session, _profile) = probe_login(client, ms_access)?;
    remember(account_id, &session, &refresh)?;
    Ok(session)
}

/// 启动前的取票口：缓存新鲜就用缓存，否则拿 refresh token 换一套新的
pub fn ensure_session(
    client: &reqwest::blocking::Client,
    account_id: &str,
) -> AppResult<Session> {
    if let Some(s) = cache().lock().ok().and_then(|m| m.get(account_id).cloned()) {
        if s.is_fresh() {
            return Ok(s);
        }
    }
    let refresh = secret::get(account_id)?.ok_or_else(|| {
        AppError::Message("这个微软账户的登录凭据不在了，请重新登录".into())
    })?;
    match refresh_tokens(client, &refresh)? {
        PollOutcome::Done {
            ms_access,
            refresh: new_refresh,
        } => complete_login(client, account_id, new_refresh, &ms_access),
        PollOutcome::Denied | PollOutcome::Expired => {
            Err(AppError::Message("微软登录已过期，请重新登录".into()))
        }
        _ => Err(AppError::Message("刷新登录状态时被微软拒绝，请重新登录".into())),
    }
}

/// 已经登录过的账户：有没有可用的 refresh token
pub fn has_secret(account_id: &str) -> bool {
    matches!(secret::get(account_id), Ok(Some(v)) if !v.is_empty())
}
/// 内存里有没有还没过期的令牌：界面靠它显示“已就绪”，不用再点一次登录
pub fn is_live(account_id: &str) -> bool {
    cache()
        .lock()
        .ok()
        .and_then(|m| m.get(account_id).map(|s| s.is_fresh()))
        .unwrap_or(false)
}

/// 只走认证链、不动本地状态：首次登录时还不知道该挂到哪个账户 id 上
pub fn probe_login(
    client: &reqwest::blocking::Client,
    ms_access: &str,
) -> AppResult<(Session, Profile)> {
    let (xbl, uhs) = xbl_authenticate(client, ms_access)?;
    let xsts = xsts_authorize(client, &xbl)?;
    let (mc_token, expires_at) = minecraft_login(client, &uhs, &xsts)?;
    let profile = fetch_profile(client, &mc_token)?;
    let session = Session {
        access_token: mc_token,
        name: profile.name.clone(),
        uuid: profile.uuid(),
        expires_at,
    };
    Ok((session, profile))
}

/// 把登录态挂到某个账户 id 上：内存缓存给启动用，refresh token 进凭据库
pub fn remember(account_id: &str, session: &Session, refresh: &str) -> AppResult<()> {
    if let Ok(mut m) = cache().lock() {
        m.insert(account_id.to_string(), session.clone());
    }
    secret::set(account_id, refresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_success_carries_refresh() {
        let body = r#"{"access_token":"ms-a","refresh_token":"ms-r"}"#;
        match read_token_body(200, body).unwrap() {
            PollOutcome::Done { ms_access, refresh } => {
                assert_eq!(ms_access, "ms-a");
                assert_eq!(refresh, "ms-r");
            }
            other => panic!("应当成功，实际 {other:?}"),
        }
    }

    #[test]
    fn pending_and_slow_down_are_states_not_errors() {
        let pending = r#"{"error":"authorization_pending"}"#;
        let slow = r#"{"error":"slow_down"}"#;
        assert_eq!(read_token_body(400, pending).unwrap(), PollOutcome::Pending);
        assert_eq!(read_token_body(400, slow).unwrap(), PollOutcome::SlowDown);
    }

    #[test]
    fn denied_and_expired_are_terminal() {
        let denied = r#"{"error":"authorization_declined"}"#;
        let expired = r#"{"error":"expired_token"}"#;
        assert_eq!(read_token_body(400, denied).unwrap(), PollOutcome::Denied);
        assert_eq!(read_token_body(400, expired).unwrap(), PollOutcome::Expired);
    }

    #[test]
    fn unknown_error_keeps_description() {
        let body = r#"{"error":"invalid_grant","error_description":"AADSTS70000 什么"}"#;
        let err = read_token_body(400, body).unwrap_err().to_string();
        assert!(err.contains("invalid_grant"), "{err}");
        assert!(err.contains("AADSTS70000"), "{err}");
    }

    #[test]
    fn xsts_hints_are_actionable() {
        assert!(xsts_hint("2148916233").contains("xbox.com"));
        assert!(xsts_hint("2148916238").contains("家长"));
        assert!(xsts_hint("999").contains("999"));
    }

    #[test]
    fn profile_uuid_is_dashless_lowercase() {
        let p = Profile {
            id: "069A79F444E94726A5BAE090B6523B00".into(),
            name: "Notch".into(),
        };
        assert_eq!(p.uuid(), "069a79f444e94726a5bae090b6523b00");
    }
}
