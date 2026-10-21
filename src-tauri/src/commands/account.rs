//! 账户管理命令。
//!
//! 当前只支持离线账户；微软账户会在 kind 上扩展，不改这里的结构。

use serde::Serialize;
use tauri::State;

use crate::account::{microsoft, offline, secret};
use crate::error::{AppError, AppResult};
use crate::state::{Account, AccountKind, AppState, UuidMode};

/// 面向界面的账户视图，脱敏字段（凭据）永远不出现在这里
#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub id: String,
    pub kind: AccountKind,
    pub name: String,
    pub uuid: String,
    pub uuid_mode: UuidMode,
    pub is_current: bool,
    /// 微软账户：凭据库里还有refresh token，即“不用重新登录”
    pub has_secret: bool,
}

/// 账户视图的唯一构造函数：微软登录那边写完账户也要走这里，
/// 否则两边对 is_current / has_secret 的判断迟早会漂移
pub fn view_of(account: &Account, cfg: &crate::state::AppConfig) -> AccountView {
    AccountView {
        id: account.id.clone(),
        kind: account.kind,
        name: account.name.clone(),
        uuid: account.uuid.clone(),
        uuid_mode: account.uuid_mode,
        is_current: cfg.current_account_id.as_deref() == Some(account.id.as_str()),
        has_secret: account.kind == AccountKind::Microsoft && microsoft::has_secret(&account.id),
    }
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Vec<AccountView> {
    let cfg = state.snapshot();
    cfg.accounts
        .iter()
        .map(|a| view_of(a, &cfg))
        .collect()
}

#[tauri::command]
pub fn add_offline_account(
    state: State<'_, AppState>,
    name: String,
    uuid_mode: UuidMode,
) -> AppResult<AccountView> {
    let name = offline::validate_name(&name)?;
    let uuid = offline::uuid_for(&name, uuid_mode).simple().to_string();
    let account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        kind: AccountKind::Offline,
        name,
        uuid,
        uuid_mode,
    };
    let id = account.id.clone();

    state.update(|cfg| {
        let dup = cfg
            .accounts
            .iter()
            .any(|a| a.kind == AccountKind::Offline && a.name.eq_ignore_ascii_case(&account.name));
        if dup {
            return Err(AppError::Message(format!(
                "已经有一个叫「{}」的离线账户了",
                account.name
            )));
        }
        cfg.accounts.push(account);
        // 首个账户自动成为主账户，省得用户多点一次
        if cfg.current_account_id.is_none() {
            cfg.current_account_id = Some(id.clone());
        }
        Ok(())
    })?;

    let cfg = state.snapshot();
    let created = cfg
        .accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::Message("账户写入后未能读回".into()))?;
    Ok(view_of(created, &cfg))
}

#[tauri::command]
pub fn update_offline_account(
    state: State<'_, AppState>,
    id: String,
    name: String,
    uuid_mode: UuidMode,
) -> AppResult<AccountView> {
    let name = offline::validate_name(&name)?;

    state.update(|cfg| {
        let idx = cfg
            .accounts
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| AppError::NotFound("账户".into()))?;

        // 微软账户的名字与 UUID 都是官方档案给的，本地改了就登不上
        if cfg.accounts[idx].kind != AccountKind::Offline {
            return Err(AppError::Message(
                "微软账户的信息由官方档案决定，不能在这里修改".into(),
            ));
        }
        let dup = cfg.accounts.iter().enumerate().any(|(i, a)| {
            i != idx && a.kind == AccountKind::Offline && a.name.eq_ignore_ascii_case(&name)
        });
        if dup {
            return Err(AppError::Message(format!(
                "已经有一个叫「{name}」的离线账户了"
            )));
        }

        // 随机 UUID 的账户改名要保住原 UUID，否则存档就换了身份
        let keep_existing = uuid_mode == UuidMode::Random
            && cfg.accounts[idx].uuid_mode == UuidMode::Random
            && cfg.accounts[idx].uuid.len() == 32;
        let uuid = if keep_existing {
            cfg.accounts[idx].uuid.clone()
        } else {
            offline::uuid_for(&name, uuid_mode).simple().to_string()
        };

        cfg.accounts[idx].name = name;
        cfg.accounts[idx].uuid_mode = uuid_mode;
        cfg.accounts[idx].uuid = uuid;
        Ok(())
    })?;

    let cfg = state.snapshot();
    let updated = cfg
        .accounts
        .iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::Message("账户更新后未能读回".into()))?;
    Ok(view_of(updated, &cfg))
}

#[tauri::command]
pub fn remove_account(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.update(|cfg| {
        let before = cfg.accounts.len();
        cfg.accounts.retain(|a| a.id != id);
        if cfg.accounts.len() == before {
            return Err(AppError::NotFound("账户".into()));
        }
        if cfg.current_account_id.as_deref() == Some(id.as_str()) {
            // 删掉当前账户后落到第一个，一个不剩则置空
            cfg.current_account_id = cfg.accounts.first().map(|a| a.id.clone());
        }
        Ok(())
    })?;
    // 账户都删了，残留凭据与内存里的令牌都清掉；清不掉也不阻断流程
    microsoft::forget(&id);
    Ok(())
}

#[tauri::command]
pub fn set_current_account(state: State<'_, AppState>, id: Option<String>) -> AppResult<()> {
    state.update(|cfg| {
        match id.as_deref() {
            Some(v) => {
                if !cfg.accounts.iter().any(|a| a.id == v) {
                    return Err(AppError::NotFound("账户".into()));
                }
                cfg.current_account_id = Some(v.to_string());
            }
            None => cfg.current_account_id = None,
        }
        Ok(())
    })
}

/// 凭据存储的可用状态，界面用它提示“密码会不会被保存”
#[derive(Debug, Clone, Serialize)]
pub struct SecretStatus {
    pub backend: String,
    pub backend_name: String,
    pub available: bool,
    pub degraded: bool,
    pub note: String,
}

#[tauri::command]
pub fn secret_store_status() -> SecretStatus {
    let backend = secret::backend();
    let degraded = secret::is_degraded();
    let available = backend != secret::SecretBackend::Memory && !degraded;
    let note = if degraded {
        "系统凭据存储调用失败，已降级为内存存储，密码不会持久化".to_string()
    } else if available {
        format!("账户凭据将保存在{}", backend.describe())
    } else {
        "当前平台没有系统凭据存储，账户凭据只在本次运行内有效".to_string()
    };
    SecretStatus {
        backend: backend.id().to_string(),
        backend_name: backend.describe().to_string(),
        available,
        degraded,
        note,
    }
}
