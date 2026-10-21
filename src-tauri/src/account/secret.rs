//! 账户凭据存储。
//!
//! 离线账户用不上 secret，但微软登录需要保存 refresh token，
//! 所以这里先把抽象铺好：Windows 走凭据管理器，macOS 走钥匙串，
//! 其余平台（含开发期的 Linux）退化为进程内存。
//!
//! 降级策略：系统调用失败不阻断用户操作，但会把 DEGRADED 置位，
//! 由 `secret_store_status` 告诉界面“密码不会持久化”。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::error::AppResult;

/// 凭据真正落在哪里
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretBackend {
    WindowsCredential,
    MacKeychain,
    Memory,
}

impl SecretBackend {
    pub fn id(&self) -> &'static str {
        match self {
            SecretBackend::WindowsCredential => "windows-credential",
            SecretBackend::MacKeychain => "mac-keychain",
            SecretBackend::Memory => "memory",
        }
    }

    pub fn describe(&self) -> &'static str {
        match self {
            SecretBackend::WindowsCredential => "Windows 凭据管理器",
            SecretBackend::MacKeychain => "macOS 钥匙串",
            SecretBackend::Memory => "内存（退出后失效）",
        }
    }
}

/// 系统凭据存储是否已经降级过
static DEGRADED: AtomicBool = AtomicBool::new(false);

pub fn is_degraded() -> bool {
    DEGRADED.load(Ordering::Relaxed)
}

fn mark_degraded() {
    DEGRADED.store(true, Ordering::Relaxed);
}

/// 当前编译平台用的后端
pub fn backend() -> SecretBackend {
    #[cfg(target_os = "windows")]
    {
        SecretBackend::WindowsCredential
    }
    #[cfg(target_os = "macos")]
    {
        SecretBackend::MacKeychain
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        SecretBackend::Memory
    }
}

/// 统一的条目名，Windows 直接拿它当 TargetName
fn target(id: &str) -> String {
    format!("NovaLauncher/account/{id}")
}

// ---------- 内存兜底 ----------

fn memory() -> &'static Mutex<HashMap<String, String>> {
    static MEM: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MEM.get_or_init(|| Mutex::new(HashMap::new()))
}

fn mem_set(key: &str, secret: &str) {
    memory()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.to_string(), secret.to_string());
}

fn mem_get(key: &str) -> Option<String> {
    memory()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(key)
        .cloned()
}

fn mem_delete(key: &str) {
    memory()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(key);
}

// ---------- 对外接口 ----------

pub fn set(id: &str, secret: &str) -> AppResult<()> {
    let key = target(id);
    match imp::set(&key, secret) {
        Ok(()) => Ok(()),
        Err(e) => {
            // 用户不该因为系统凭据出问题就登不上号，这里兜到内存
            mark_degraded();
            log::warn!("凭据写入降级为内存：{e}");
            mem_set(&key, secret);
            Ok(())
        }
    }
}

pub fn get(id: &str) -> AppResult<Option<String>> {
    let key = target(id);
    match imp::get(&key) {
        Ok(Some(v)) => Ok(Some(v)),
        Ok(None) => Ok(mem_get(&key)),
        Err(e) => {
            mark_degraded();
            log::warn!("凭据读取降级为内存：{e}");
            Ok(mem_get(&key))
        }
    }
}

/// 删除不返回错误：账户都已经删了，残留条目清不掉也不该中断流程
pub fn delete(id: &str) {
    let key = target(id);
    if let Err(e) = imp::delete(&key) {
        mark_degraded();
        log::warn!("凭据删除失败：{e}");
    }
    mem_delete(&key);
}

// ---------- Windows 凭据管理器 ----------

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;

    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    use crate::error::{AppError, AppResult};

    /// ERROR_NOT_FOUND，凭据不存在时 CredRead/CredDelete 会返回它
    const ERROR_NOT_FOUND: i32 = 1168;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set(key: &str, secret: &str) -> AppResult<()> {
        let mut target = wide(key);
        let mut user = wide("NovaLauncher");
        let mut blob = secret.as_bytes().to_vec();
        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            Comment: PWSTR::null(),
            LastWritten: FILETIME::default(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: PWSTR::null(),
            UserName: PWSTR(user.as_mut_ptr()),
        };
        // SAFETY: 所有指针都指向本函数栈上的缓冲区，调用期间有效
        unsafe { CredWriteW(&cred, 0) }
            .map_err(|e| AppError::Message(format!("写入系统凭据失败（{}）", e.code().0)))
    }

    pub fn get(key: &str) -> AppResult<Option<String>> {
        let target = wide(key);
        let mut raw: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: raw 由 CredReadW 分配，读取完立即 CredFree
        match unsafe {
            CredReadW(
                PCWSTR(target.as_ptr()),
                CRED_TYPE_GENERIC,
                0,
                &mut raw,
            )
        } {
            Ok(()) => {}
            Err(e) if e.code().0 == ERROR_NOT_FOUND => return Ok(None),
            Err(e) => {
                return Err(AppError::Message(format!(
                    "读取系统凭据失败（{}）",
                    e.code().0
                )))
            }
        }
        if raw.is_null() {
            return Ok(None);
        }
        let text = unsafe {
            let cred = &*raw;
            let bytes = std::slice::from_raw_parts(
                cred.CredentialBlob,
                cred.CredentialBlobSize as usize,
            );
            let text = String::from_utf8_lossy(bytes).into_owned();
            CredFree(raw as *const c_void);
            text
        };
        Ok(Some(text))
    }

    pub fn delete(key: &str) -> AppResult<()> {
        let target = wide(key);
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0) } {
            Ok(()) => Ok(()),
            Err(e) if e.code().0 == ERROR_NOT_FOUND => Ok(()),
            Err(e) => Err(AppError::Message(format!(
                "删除系统凭据失败（{}）",
                e.code().0
            ))),
        }
    }
}

// ---------- macOS 钥匙串 ----------

#[cfg(target_os = "macos")]
mod imp {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    use crate::error::{AppError, AppResult};

    /// 钥匙串条目的账户名，service 用完整的 target 串
    const ACCOUNT: &str = "novalauncher";

    pub fn set(key: &str, secret: &str) -> AppResult<()> {
        set_generic_password(key, ACCOUNT, secret.as_bytes())
            .map_err(|e| AppError::Message(format!("写入钥匙串失败：{e}")))
    }

    pub fn get(key: &str) -> AppResult<Option<String>> {
        match get_generic_password(key, ACCOUNT) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            // 条目不存在时该 API 只给错误码，统一按“没有”处理
            Err(_) => Ok(None),
        }
    }

    pub fn delete(key: &str) -> AppResult<()> {
        let _ = delete_generic_password(key, ACCOUNT);
        Ok(())
    }
}

// ---------- 其余平台：进程内存 ----------

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    use super::{mem_delete, mem_get, mem_set};
    use crate::error::AppResult;

    pub fn set(key: &str, secret: &str) -> AppResult<()> {
        mem_set(key, secret);
        Ok(())
    }

    pub fn get(key: &str) -> AppResult<Option<String>> {
        Ok(mem_get(key))
    }

    pub fn delete(key: &str) -> AppResult<()> {
        mem_delete(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_round_trip() {
        // 只有内存后端才测，避免在真机上往系统凭据库写垃圾条目
        if backend() != SecretBackend::Memory {
            return;
        }
        let id = format!("test-{}", uuid::Uuid::new_v4());
        assert!(get(&id).unwrap().is_none());
        set(&id, "s3cret").unwrap();
        assert_eq!(get(&id).unwrap().as_deref(), Some("s3cret"));
        delete(&id);
        assert!(get(&id).unwrap().is_none());
    }
}
