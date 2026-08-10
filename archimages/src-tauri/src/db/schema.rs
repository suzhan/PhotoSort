//! Schema 迁移：版本号存 PRAGMA user_version，迁移脚本集中在 MIGRATIONS。
//! 业务代码里禁止散写 CREATE TABLE（需求 §二十一）。

use rusqlite::Connection;

use crate::error::{AppError, Result};

/// (版本号, SQL)。追加式：新版本一律往后加，绝不修改历史迁移。
const MIGRATIONS: &[(u32, &str)] = &[(
    1,
    "
    CREATE TABLE settings (
        key        TEXT PRIMARY KEY,
        value_json TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE gps_cache (
        latitude_key      TEXT NOT NULL,
        longitude_key     TEXT NOT NULL,
        country           TEXT,
        province          TEXT,
        city              TEXT,
        district          TEXT,
        formatted_address TEXT,
        raw_provider      TEXT NOT NULL,
        updated_at        INTEGER NOT NULL,
        PRIMARY KEY (latitude_key, longitude_key)
    );

    CREATE TABLE hash_cache (
        path       TEXT PRIMARY KEY,
        size       INTEGER NOT NULL,
        mtime      INTEGER NOT NULL,
        sha256     TEXT,
        md5        TEXT,
        sha1       TEXT,
        updated_at INTEGER NOT NULL
    );

    CREATE TABLE jobs (
        id               TEXT PRIMARY KEY,
        kind             TEXT NOT NULL,
        status           TEXT NOT NULL,
        source_root      TEXT,
        destination_root TEXT,
        settings_json    TEXT,
        started_at       INTEGER NOT NULL,
        finished_at      INTEGER
    );

    CREATE TABLE job_files (
        id            INTEGER PRIMARY KEY AUTOINCREMENT,
        job_id        TEXT NOT NULL REFERENCES jobs(id),
        source_path   TEXT NOT NULL,
        target_path   TEXT,
        status        TEXT NOT NULL,
        source_sha256 TEXT,
        target_sha256 TEXT,
        error         TEXT,
        updated_at    INTEGER NOT NULL,
        UNIQUE (job_id, source_path)
    );
    CREATE INDEX idx_job_files_job ON job_files(job_id);
    ",
)];

/// 把数据库迁移到最新版本；每个版本在独立事务中应用。
pub fn migrate(conn: &mut Connection) -> Result<()> {
    let current: u32 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|e| AppError::Database(format!("read user_version: {e}")))?;

    for (version, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("begin migration {version}: {e}")))?;
        tx.execute_batch(sql)
            .map_err(|e| AppError::Database(format!("migration {version}: {e}")))?;
        tx.pragma_update(None, "user_version", *version)
            .map_err(|e| AppError::Database(format!("set user_version {version}: {e}")))?;
        tx.commit()
            .map_err(|e| AppError::Database(format!("commit migration {version}: {e}")))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_database_migrates_to_latest() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate(&mut conn).expect("migrate");
        let version: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .expect("version");
        assert_eq!(version, MIGRATIONS.last().expect("last").0);

        // 五张表都应在
        for table in ["settings", "gps_cache", "hash_cache", "jobs", "job_files"] {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .expect("query");
            assert_eq!(count, 1, "table {table} missing");
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().expect("open");
        migrate(&mut conn).expect("first");
        migrate(&mut conn).expect("second");
    }
}
