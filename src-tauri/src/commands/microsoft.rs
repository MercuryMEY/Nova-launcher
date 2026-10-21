//! 微软登录命令。
//!
//! 设备码流程拆成两步：start 拿票据，poll 由前端按官方给的间隔轮询。
//! 后端不替用户睡着等——界面才随时能取消，也不占一个线程十三分钟。
use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::account::microsoft::{self, DeviceCodeTicket, PollOutcome};
use crate::commands::account::{view_of, AccountView};
use crate::error::{AppError, AppResult};
use crate::state::{Account, AccountKind, AppState, UuidMode};

/// 正在进行的设备码票据。同一时间只支持一个登录，避免两套票据互相覆盖
static PENDING: Mutex<Option<DeviceCodeTicket>> = Mutex::new(None);

fn clear_pending() {
    if let Ok(mut slot) = PENDING.lock() {
        *slot = None;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MsDeviceLogin {
    /// 8 位验证码，界面要把它放大显示
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub expires_in: u64,
    /// 前端每隔这么多秒轮一次，太急会被微软判 slow_down
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MsPoll {
    /// pending / slow_down / success / denied / expired
    pub status: String,
    pub message: String,
    pub account: Option<AccountView>,
}

impl MsPoll {
    fn state(status: &str, message: &str) -> Self {
        Self {
            status: status.into(),
            message: message.into(),
            account: None,
        }
    }
}

#[tauri::command]
pub async fn ms_login_start(state: State<'_, AppState>) -> AppResult<MsDeviceLogin> {
    let http = state.http.clone();
    let ticket = tauri::async_runtime::spawn_blocking(move || microsoft::request_device_code(&http))
        .await
        .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??;

    let message = if ticket.message.trim().is_empty() {
        format!("打开 {} 并输入代码 {}", ticket.verification_uri, ticket.user_code)
    } else {
        ticket.message.clone()
    };
    let reply = MsDeviceLogin {
        user_code: ticket.user_code.clone(),
        verification_uri: ticket.verification_uri.clone(),
        message,
        expires_in: ticket.expires_in,
        interval: ticket.interval.max(3),
    };
    if let Ok(mut slot) = PENDING.lock() {
        *slot = Some(ticket);
    }
    Ok(reply)
}

#[tauri::command]
pub async fn ms_login_poll(state: State<'_, AppState>) -> AppResult<MsPoll> {
    let ticket = {
        let guard = PENDING
            .lock()
            .map_err(|_| AppError::Message("登录状态被占用，重试一下".into()))?;
        guard.clone()
    }
    .ok_or_else(|| AppError::Message("没有正在进行的登录，请重新获取验证码".into()))?;

    let http = state.http.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || microsoft::poll_device_once(&http, &ticket))
        .await
        .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??;

    match outcome {
        PollOutcome::Pending => Ok(MsPoll::state("pending", "等待你在浏览器里确认")),
        PollOutcome::SlowDown => Ok(MsPoll::state("slow_down", "浏览器还没确认，我放慢点再问")),
        PollOutcome::Denied => {
            clear_pending();
            Ok(MsPoll::state("denied", "你在浏览器里拒绝了这次登录"))
        }
        PollOutcome::Expired => {
            clear_pending();
            Ok(MsPoll::state("expired", "验证码过期了，重新获取一个吧"))
        }
        PollOutcome::Done { ms_access, refresh } => {
            clear_pending();
            finish_login(state, ms_access, refresh).await
        }
    }
}

/// 微软令牌 → Xbox → Minecraft → 档案，然后把账户写进配置
async fn finish_login(
    state: State<'_, AppState>,
    ms_access: String,
    refresh: String,
) -> AppResult<MsPoll> {
    let http = state.http.clone();
    let (session, _profile) = tauri::async_runtime::spawn_blocking(move || {
        microsoft::probe_login(&http, &ms_access)
    })
    .await
    .map_err(|e| AppError::Message(format!("后台任务异常：{e}")))??;

    // 同一个微软账户重复登录时复用原条目，否则列表里会出两个一模一样的号
    let cfg = state.snapshot();
    let existing = cfg
        .accounts
        .iter()
        .find(|a| a.kind == AccountKind::Microsoft && a.uuid == session.uuid)
        .map(|a| a.id.clone());
    let account_id = existing.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let name = session.name.clone();
    let uuid_value = session.uuid.clone();

    state.update(|cfg| {
        match cfg.accounts.iter_mut().find(|a| a.id == account_id) {
            Some(a) => {
                a.name = name.clone();
                a.uuid = uuid_value.clone();
            }
            None => cfg.accounts.push(Account {
                id: account_id.clone(),
                kind: AccountKind::Microsoft,
                name: name.clone(),
                uuid: uuid_value.clone(),
                // 微软账户的 uuid 由官方给，这个字段对它没意义
                uuid_mode: UuidMode::NameBased,
            }),
        }
        if cfg.current_account_id.is_none() {
            cfg.current_account_id = Some(account_id.clone());
        }
        Ok(())
    })?;

    microsoft::remember(&account_id, &session, &refresh)?;

    let cfg = state.snapshot();
    let view = cfg
        .accounts
        .iter()
        .find(|a| a.id == account_id)
        .map(|a| view_of(a, &cfg))
        .ok_or_else(|| AppError::Message("账户写入后未能读回".into()))?;
    Ok(MsPoll {
        status: "success".into(),
        message: format!("已登录：{name}"),
        account: Some(view),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct MsAccountState {
    pub id: String,
    /// 凭据库里还有 refresh token，即“不用重新登录”
    pub has_secret: bool,
    /// 内存里有未过期的 Minecraft 令牌
    pub live: bool,
}

#[tauri::command]
pub fn ms_account_state(id: String) -> MsAccountState {
    MsAccountState {
        live: microsoft::is_live(&id),
        has_secret: microsoft::has_secret(&id),
        id,
    }
}

/// 取消正在等的登录，留个接口给界面上的“算了”
#[tauri::command]
pub fn ms_login_cancel() {
    clear_pending();
}
