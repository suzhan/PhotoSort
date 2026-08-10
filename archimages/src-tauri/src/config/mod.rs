//! 设置持久化。
//!
//! Phase 2 用 JSON 文件（原子写入：tmp + rename，崩溃不留半截配置）。
//! Phase 10 引入 SQLite 后将在 `SettingsStore` trait 后面换成 settings 表，
//! 调用方不受影响。

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};
use crate::models::settings::AppSettings;

pub trait SettingsStore {
    fn load(&self) -> Result<AppSettings>;
    fn save(&self, settings: &AppSettings) -> Result<()>;
}

pub struct JsonSettingsStore {
    path: PathBuf,
}

impl JsonSettingsStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl SettingsStore for JsonSettingsStore {
    fn load(&self) -> Result<AppSettings> {
        match fs::read_to_string(&self.path) {
            Ok(text) => {
                let settings: AppSettings = serde_json::from_str(&text)
                    .map_err(|e| AppError::Config(format!("settings file is corrupted: {e}")))?;
                settings.validate()?;
                Ok(settings)
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(AppSettings::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, settings: &AppSettings) -> Result<()> {
        settings.validate()?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(settings)
            .map_err(|e| AppError::Config(format!("serialize settings: {e}")))?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &Path) -> JsonSettingsStore {
        JsonSettingsStore::new(dir.join("settings.json"))
    }

    #[test]
    fn missing_file_returns_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(dir.path());
        assert_eq!(store.load().expect("load"), AppSettings::default());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(dir.path());
        let s = AppSettings {
            language: "en".to_string(),
            source_directory: Some(PathBuf::from("/tmp/photos")),
            ..Default::default()
        };
        store.save(&s).expect("save");
        assert_eq!(store.load().expect("load"), s);
        // 原子写入不应残留 tmp 文件
        assert!(!store.path().with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupted_file_errors_not_panics() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(dir.path());
        fs::write(store.path(), "{ not json").expect("write garbage");
        let err = store.load().expect_err("should fail");
        assert_eq!(err.user_key(), "error.config");
    }

    #[test]
    fn save_rejects_invalid_settings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store_in(dir.path());
        let s = AppSettings {
            filename_template: String::new(),
            ..Default::default()
        };
        assert!(store.save(&s).is_err());
        assert!(!store.path().exists());
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = JsonSettingsStore::new(dir.path().join("nested/deep/settings.json"));
        store.save(&AppSettings::default()).expect("save");
        assert!(store.path().exists());
    }
}
