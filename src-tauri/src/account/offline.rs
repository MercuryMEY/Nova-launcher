//! 离线账户的用户名校验与 UUID 生成。

use md5::{Digest, Md5};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::state::UuidMode;

/// 用户名长度下限，与 Minecraft 的账号规则一致
pub const NAME_MIN: usize = 3;
/// 用户名长度上限
pub const NAME_MAX: usize = 16;

/// Java `UUID.nameUUIDFromBytes` 的等价实现。
///
/// 对原始字节做 MD5，再把第 6 字节的低 4 位置成 3（version 3），
/// 第 8 字节的高 2 位置成 10（RFC 4122 variant）。
/// 不能直接用 `Uuid::new_v3`：它会把 namespace 的 16 字节也拼进摘要，
/// 和 Java 只哈希一份数据的行为对不上。
pub fn uuid_v3_from_bytes(data: &[u8]) -> Uuid {
    let digest = Md5::digest(data);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x30;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// 离线服务器（online-mode=false）按 `OfflinePlayer:<名字>` 派生 UUID。
/// 只有用同一个算法，本地算出的身份才和服务器侧的玩家数据对得上。
pub fn offline_uuid(name: &str) -> Uuid {
    uuid_v3_from_bytes(format!("OfflinePlayer:{name}").as_bytes())
}

/// 随机 UUID，改名字也不会丢存档，但和服务器算出来的对不上
pub fn random_uuid() -> Uuid {
    Uuid::new_v4()
}

/// 按账户选择的方式生成 UUID
pub fn uuid_for(name: &str, mode: UuidMode) -> Uuid {
    match mode {
        UuidMode::NameBased => offline_uuid(name),
        UuidMode::Random => random_uuid(),
    }
}

/// 校验并规范化用户名，返回去掉首尾空格后的名字。
pub fn validate_name(raw: &str) -> AppResult<String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(AppError::Message("请填写用户名".into()));
    }
    let len = name.chars().count();
    if !(NAME_MIN..=NAME_MAX).contains(&len) {
        return Err(AppError::Message(format!(
            "用户名需要 {NAME_MIN}-{NAME_MAX} 个字符，当前 {len} 个"
        )));
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AppError::Message(
            "用户名只能使用英文字母、数字和下划线".into(),
        ));
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java_algorithm() {
        assert_eq!(
            offline_uuid("Notch").to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert_eq!(
            offline_uuid("Steve").to_string(),
            "5627dd98-e6be-3c21-b8a8-e92344183641"
        );
        assert_eq!(
            offline_uuid("Player").to_string(),
            "a01e3843-e521-3998-958a-f459800e4d11"
        );
    }

    #[test]
    fn uuid_version_bits_are_correct() {
        assert_eq!(offline_uuid("Player").get_version_num(), 3);
        assert_eq!(random_uuid().get_version_num(), 4);
    }

    #[test]
    fn name_is_case_sensitive() {
        assert_ne!(offline_uuid("Notch"), offline_uuid("notch"));
    }

    #[test]
    fn validate_name_rules() {
        assert_eq!(validate_name("  Steve  ").unwrap(), "Steve");
        assert!(validate_name("ab").is_err());
        assert!(validate_name("abcdefghijklmnopq").is_err());
        assert!(validate_name("中文名字").is_err());
        assert!(validate_name("has space").is_err());
        assert!(validate_name("   ").is_err());
        assert!(validate_name("ok_name_1").is_ok());
    }
}
