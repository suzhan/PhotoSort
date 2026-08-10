//! SQLite 持久化：集中 schema 迁移与各表 DAO。
//! 连接为 Arc<Mutex<Connection>>：rusqlite 连接非 Sync，单锁足够当前负载
//! （写入全部批量事务化；读多为单行查询）。

pub mod gps_cache;
pub mod hash_cache;
pub mod jobs;
pub mod schema;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// 打开（必要时创建）数据库并迁移到最新 schema。
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Database(format!("create db dir: {e}")))?;
        }
        let conn =
            Connection::open(path).map_err(|e| AppError::Database(format!("open db: {e}")))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let conn =
            Connection::open_in_memory().map_err(|e| AppError::Database(format!("open: {e}")))?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| AppError::Database(format!("set WAL: {e}")))?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| AppError::Database(format!("set foreign_keys: {e}")))?;
        conn.pragma_update(None, "busy_timeout", 5000_i64)
            .map_err(|e| AppError::Database(format!("set busy_timeout: {e}")))?;
        schema::migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// 取连接锁。锁中毒不致命：into_inner 继续用（数据正确性由事务保证）。
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn hash_cache(&self) -> hash_cache::HashCache {
        hash_cache::HashCache::new(self.clone())
    }

    pub fn gps_cache(&self) -> gps_cache::GpsCache {
        gps_cache::GpsCache::new(self.clone())
    }

    pub fn journal(&self) -> jobs::JobJournal {
        jobs::JobJournal::new(self.clone())
    }
}

pub(crate) fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates_on_disk() {
        let tmp = tempfile::tempdir().expect("tmp");
        let db = Database::open(&tmp.path().join("sub/archimages.db")).expect("open");
        let version: u32 = db
            .lock()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .expect("version");
        assert_eq!(version, 1);
    }
}
