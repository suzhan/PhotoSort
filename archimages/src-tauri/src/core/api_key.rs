//! Google API Key 安全存储（需求 §七/§三十）。
//!
//! 用 keyring 把 Key 存进 OS 原生凭据存储（macOS Keychain / Windows Credential Manager），
//! **绝不**进 settings.json，**绝不**进日志。

use keyring::Entry;

use crate::error::{AppError, Result};

const SERVICE: &str = "archimages";
const USERNAME: &str = "google_maps_api_key";

pub struct ApiKeyStore;

impl ApiKeyStore {
    pub fn set(key: &str) -> Result<()> {
        let entry = Entry::new(SERVICE, USERNAME)
            .map_err(|e| AppError::Permission(format!("open keyring: {e}")))?;
        entry
            .set_password(key)
            .map_err(|e| AppError::Permission(format!("store api key: {e}")))
    }

    pub fn get() -> Result<Option<String>> {
        let entry = Entry::new(SERVICE, USERNAME)
            .map_err(|e| AppError::Permission(format!("open keyring: {e}")))?;
        match entry.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Permission(format!("read api key: {e}"))),
        }
    }

    pub fn clear() -> Result<()> {
        let entry = Entry::new(SERVICE, USERNAME)
            .map_err(|e| AppError::Permission(format!("open keyring: {e}")))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Permission(format!("delete api key: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI/无 keyring 环境下跳过；本地有 keyring 时验证往返。
    #[test]
    fn set_get_clear_roundtrip() {
        let _ = ApiKeyStore::set("test-key-xyz");
        let got = ApiKeyStore::get().unwrap_or(None);
        if got.is_none() {
            // 无 keyring 后端：跳过断言但不 panic
            return;
        }
        assert_eq!(got.as_deref(), Some("test-key-xyz"));
        let _ = ApiKeyStore::clear();
        assert!(ApiKeyStore::get().unwrap_or(None).is_none());
    }
}
