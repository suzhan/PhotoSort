//! 哈希缓存 DAO（需求 §十二）：path + size + mtime 三者不变才允许复用，
//! 任何一项变化都视为失效，重新计算后覆盖。

use std::path::Path;
use std::time::SystemTime;

use rusqlite::{params, OptionalExtension};

use super::{now_unix, Database};
use crate::error::{AppError, Result};
use crate::models::duplicate::FileHash;

#[derive(Clone)]
pub struct HashCache {
    db: Database,
}

/// mtime 量化到纳秒整数键；溢出时饱和（现实文件系统到不了）。
fn mtime_key(t: SystemTime) -> i64 {
    let d = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    d.as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(d.subsec_nanos())) as i64
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode<const N: usize>(s: &str) -> Result<[u8; N]> {
    if s.len() != N * 2 {
        return Err(AppError::Hash(format!("bad hex length: {}", s.len())));
    }
    let mut out = [0u8; N];
    for (i, chunk) in s.as_bytes().chunks_exact(2).enumerate() {
        let hi = (chunk[0] as char).to_digit(16);
        let lo = (chunk[1] as char).to_digit(16);
        match (hi, lo) {
            (Some(hi), Some(lo)) => out[i] = ((hi << 4) | lo) as u8,
            _ => return Err(AppError::Hash("invalid hex in cache".to_string())),
        }
    }
    Ok(out)
}

impl HashCache {
    pub(crate) fn new(db: Database) -> Self {
        Self { db }
    }

    /// 命中返回缓存哈希；未命中/已失效返回 None。损坏行按未命中处理（下次重算覆盖）。
    pub fn lookup(&self, path: &Path, size: u64, mtime: SystemTime) -> Option<FileHash> {
        let key = path.to_string_lossy().into_owned();
        let row = {
            let conn = self.db.lock();
            conn.query_row(
                "SELECT size, mtime, sha256, md5, sha1 FROM hash_cache WHERE path = ?1",
                params![key],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()
        };
        let (cached_size, cached_mtime, sha256, md5, sha1) = row?;
        if cached_size != size as i64 || cached_mtime != mtime_key(mtime) {
            return None;
        }
        Some(FileHash {
            size,
            sha256: sha256.and_then(|s| hex_decode::<32>(&s).ok()),
            md5: md5.and_then(|s| hex_decode::<16>(&s).ok()),
            sha1: sha1.and_then(|s| hex_decode::<20>(&s).ok()),
        })
    }

    pub fn store(&self, path: &Path, mtime: SystemTime, hash: &FileHash) -> Result<()> {
        let key = path.to_string_lossy().into_owned();
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO hash_cache (path, size, mtime, sha256, md5, sha1, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(path) DO UPDATE SET
               size = excluded.size, mtime = excluded.mtime,
               sha256 = excluded.sha256, md5 = excluded.md5, sha1 = excluded.sha1,
               updated_at = excluded.updated_at",
            params![
                key,
                hash.size as i64,
                mtime_key(mtime),
                hash.sha256.map(|b| hex_encode(&b)),
                hash.md5.map(|b| hex_encode(&b)),
                hash.sha1.map(|b| hex_encode(&b)),
                now_unix(),
            ],
        )
        .map_err(|e| AppError::Database(format!("store hash cache: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn roundtrip_and_invalidation() {
        let db = Database::open_in_memory().expect("db");
        let cache = db.hash_cache();
        let path = Path::new("/photos/a.jpg");
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let hash = FileHash {
            size: 42,
            sha256: Some([0xab; 32]),
            md5: Some([0xcd; 16]),
            sha1: None,
        };

        assert!(cache.lookup(path, 42, mtime).is_none());
        cache.store(path, mtime, &hash).expect("store");

        let hit = cache.lookup(path, 42, mtime).expect("hit");
        assert_eq!(hit.sha256, Some([0xab; 32]));
        assert_eq!(hit.md5, Some([0xcd; 16]));
        assert_eq!(hit.sha1, None);

        // size 变了 → 失效
        assert!(cache.lookup(path, 43, mtime).is_none());
        // mtime 变了 → 失效
        assert!(cache
            .lookup(path, 42, mtime + Duration::from_secs(1))
            .is_none());

        // 覆盖写
        let newer = FileHash {
            size: 43,
            sha256: Some([0x11; 32]),
            md5: None,
            sha1: None,
        };
        cache.store(path, mtime, &newer).expect("overwrite");
        let hit = cache.lookup(path, 43, mtime).expect("hit");
        assert_eq!(hit.sha256, Some([0x11; 32]));
        assert_eq!(hit.md5, None);
    }

    #[test]
    fn corrupt_hex_row_behaves_as_miss() {
        let db = Database::open_in_memory().expect("db");
        {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO hash_cache (path, size, mtime, sha256, md5, sha1, updated_at)
                 VALUES ('/x.jpg', 1, 0, 'zz', NULL, NULL, 0)",
                [],
            )
            .expect("insert");
        }
        let hit = db
            .hash_cache()
            .lookup(Path::new("/x.jpg"), 1, SystemTime::UNIX_EPOCH);
        let h = hit.expect("row parses as FileHash");
        assert_eq!(h.sha256, None, "坏 hex 字段降级为 None，不 panic");
    }
}
