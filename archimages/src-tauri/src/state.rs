//! 全局应用状态：设置快照 + 持久化句柄 + 日志 guard。
//! 后续 Phase 会在此挂载数据库连接与任务管理器。

use std::sync::RwLock;

use crate::config::{JsonSettingsStore, SettingsStore};
use crate::core::task_manager::TaskManager;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::settings::AppSettings;
use crate::utils::logging::LogGuard;

pub struct AppState {
    settings: RwLock<AppSettings>,
    settings_store: JsonSettingsStore,
    task_manager: TaskManager,
    db: Database,
    // 持有以保持非阻塞日志 worker 存活。
    _log_guard: LogGuard,
}

impl AppState {
    pub fn new(
        settings_store: JsonSettingsStore,
        log_guard: LogGuard,
        db: Database,
    ) -> Result<Self> {
        let settings = settings_store.load()?;
        Ok(Self {
            settings: RwLock::new(settings),
            settings_store,
            task_manager: TaskManager::new(),
            db,
            _log_guard: log_guard,
        })
    }

    pub fn task_manager(&self) -> &TaskManager {
        &self.task_manager
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub fn snapshot(&self) -> Result<AppSettings> {
        self.settings
            .read()
            .map(|g| g.clone())
            .map_err(|e| AppError::Config(format!("settings lock poisoned: {e}")))
    }

    pub fn save_settings(&self, next: AppSettings) -> Result<()> {
        self.settings_store.save(&next)?;
        let mut guard = self
            .settings
            .write()
            .map_err(|e| AppError::Config(format!("settings lock poisoned: {e}")))?;
        *guard = next;
        Ok(())
    }
}
