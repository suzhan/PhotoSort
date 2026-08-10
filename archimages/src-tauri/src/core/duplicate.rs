//! 查重裁决与目标目录索引。
//!
//! 保守原则：哈希缺失或读取失败一律视为「不确定」，绝不判等
//!（需求 §十五：Hash 无法读取时绝对不能当重复处理，更不能删源文件）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tracing::warn;

use super::hash::hash_file;
use super::scanner::{self, ScanOptions};
use crate::error::Result;
use crate::models::duplicate::{DuplicateMode, FileHash};

/// 按模式比较两份哈希。size 是快速预筛，内容哈希是裁决依据。
pub fn hashes_equal(a: &FileHash, b: &FileHash, mode: DuplicateMode) -> bool {
    if a.size != b.size {
        return false;
    }
    match mode {
        DuplicateMode::Modern => match (a.sha256, b.sha256) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        DuplicateMode::LegacyStrict => match (a.md5, a.sha1, b.md5, b.sha1) {
            (Some(m1), Some(s1), Some(m2), Some(s2)) => m1 == m2 && s1 == s2,
            _ => false,
        },
    }
}

/// 目标目录哈希索引：构建期只收集 (size → paths)，哈希惰性计算并记忆化。
/// 可选挂 SQLite hash_cache（§十二）：path/size/mtime 不变时跨任务复用哈希。
/// 校验用途的哈希（copy_verify_delete 等）不走缓存，永远新鲜读盘。
pub struct DestinationIndex {
    mode: DuplicateMode,
    by_size: HashMap<u64, Vec<PathBuf>>,
    hashed: Mutex<HashMap<PathBuf, FileHash>>,
    cache: Option<crate::db::hash_cache::HashCache>,
}

impl DestinationIndex {
    /// 遍历目标树（复用 scanner 的忽略规则与白名单），只 stat 不读内容。
    pub fn build(root: &Path, mode: DuplicateMode) -> Result<Self> {
        let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
        let options = ScanOptions {
            include_subfolders: true,
        };
        // 索引构建不可取消：它在任务启动前一次性完成
        let stats = scanner::scan(root, &options, &|| false, |photo| {
            by_size.entry(photo.size).or_default().push(photo.path);
        })?;
        if stats.errors > 0 {
            warn!(
                errors = stats.errors,
                root = %root.to_string_lossy(),
                "destination index built with unreadable entries"
            );
        }
        Ok(Self {
            mode,
            by_size,
            hashed: Mutex::new(HashMap::new()),
            cache: None,
        })
    }

    /// 挂 SQLite 哈希缓存（跨任务复用）。
    pub fn with_cache(mut self, cache: crate::db::hash_cache::HashCache) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn len(&self) -> usize {
        self.by_size.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.by_size.is_empty()
    }

    /// 在目标树中找与源文件内容一致的文件。无同尺寸候选时不读源文件内容。
    /// 源文件自身在目标树内（原地整理）时跳过自身。
    pub fn find_duplicate(&self, source_path: &Path, source_size: u64) -> Result<Option<PathBuf>> {
        let candidates = match self.by_size.get(&source_size) {
            Some(c) => c,
            None => return Ok(None),
        };
        let real_candidates: Vec<&PathBuf> = candidates
            .iter()
            .filter(|p| p.as_path() != source_path)
            .collect();
        if real_candidates.is_empty() {
            return Ok(None);
        }
        let source_hash = hash_file(source_path, self.mode)?;
        for candidate in real_candidates {
            match self.hash_of(candidate) {
                Ok(h) if hashes_equal(&source_hash, &h, self.mode) => {
                    return Ok(Some(candidate.clone()));
                }
                Ok(_) => {}
                Err(e) => {
                    // 读不了的目标不参与判等（保守），但记录日志
                    warn!(path = %candidate.to_string_lossy(), error = %e, "hash candidate failed");
                }
            }
        }
        Ok(None)
    }

    /// 内容级冲突裁决：源文件与已占位的目标是否完全一致。
    pub fn equals_file(&self, source_path: &Path, target: &Path) -> Result<bool> {
        let source_hash = hash_file(source_path, self.mode)?;
        let target_hash = self.hash_of(target)?;
        Ok(hashes_equal(&source_hash, &target_hash, self.mode))
    }

    fn hash_of(&self, path: &Path) -> Result<FileHash> {
        {
            let cache = self.hashed.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(h) = cache.get(path) {
                return Ok(h.clone());
            }
        }
        // SQLite 缓存：size+mtime 校验通过且当前模式所需摘要齐全才复用
        if let Some(cache) = &self.cache {
            let meta = std::fs::metadata(path)?;
            let mtime = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            if let Some(hit) = cache.lookup(path, meta.len(), mtime) {
                if digest_complete(&hit, self.mode) {
                    let mut guard = self.hashed.lock().unwrap_or_else(|e| e.into_inner());
                    return Ok(guard.entry(path.to_path_buf()).or_insert(hit).clone());
                }
            }
            let hash = hash_file(path, self.mode)?;
            if let Err(e) = cache.store(path, mtime, &hash) {
                // 缓存失败只降级性能，不阻断任务
                warn!(path = %path.to_string_lossy(), error = %e, "hash cache store failed");
            }
            let mut guard = self.hashed.lock().unwrap_or_else(|e| e.into_inner());
            return Ok(guard.entry(path.to_path_buf()).or_insert(hash).clone());
        }
        let hash = hash_file(path, self.mode)?;
        let mut cache = self.hashed.lock().unwrap_or_else(|e| e.into_inner());
        Ok(cache.entry(path.to_path_buf()).or_insert(hash).clone())
    }
}

/// 当前查重模式需要的摘要是否齐全。
fn digest_complete(hash: &FileHash, mode: DuplicateMode) -> bool {
    match mode {
        DuplicateMode::Modern => hash.sha256.is_some(),
        DuplicateMode::LegacyStrict => hash.md5.is_some() && hash.sha1.is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_reuses_sqlite_cache_across_builds() {
        let tmp = tempfile::tempdir().expect("tmp");
        let db = crate::db::Database::open_in_memory().expect("db");
        let cache = db.hash_cache();

        let dest_dir = tmp.path().join("dest");
        std::fs::create_dir_all(&dest_dir).expect("mkdir");
        let archived = dest_dir.join("archived.jpg");
        std::fs::write(&archived, b"same-bytes").expect("write");
        let source = tmp.path().join("incoming.jpg");
        std::fs::write(&source, b"same-bytes").expect("write");

        // 第一次：惰性哈希并落缓存
        let index1 = DestinationIndex::build(&dest_dir, DuplicateMode::Modern)
            .expect("build")
            .with_cache(cache.clone());
        let found = index1
            .find_duplicate(&source, 10)
            .expect("find")
            .expect("duplicate found");
        assert_eq!(found, archived);
        let size = std::fs::metadata(&archived).expect("stat").len();
        let mtime = std::fs::metadata(&archived)
            .expect("stat")
            .modified()
            .expect("mtime");
        assert!(
            cache.lookup(&archived, size, mtime).is_some(),
            "首次运行后缓存应有记录"
        );

        // 第二次（新索引 = 模拟新任务）：同样能找到重复（走缓存路径）
        let index2 = DestinationIndex::build(&dest_dir, DuplicateMode::Modern)
            .expect("build")
            .with_cache(cache);
        let found2 = index2
            .find_duplicate(&source, 10)
            .expect("find")
            .expect("duplicate found via cache");
        assert_eq!(found2, archived);
    }

    #[test]
    fn cache_with_missing_digest_is_not_reused_for_mode() {
        // LegacyStrict 缓存行只有 md5+sha1，Modern 模式需要 sha256 → 不得复用
        let tmp = tempfile::tempdir().expect("tmp");
        let db = crate::db::Database::open_in_memory().expect("db");
        let cache = db.hash_cache();
        let dest_dir = tmp.path().join("dest");
        std::fs::create_dir_all(&dest_dir).expect("mkdir");
        let archived = dest_dir.join("a.jpg");
        std::fs::write(&archived, b"xyz").expect("write");
        let meta = std::fs::metadata(&archived).expect("stat");
        cache
            .store(
                &archived,
                meta.modified().expect("mtime"),
                &FileHash {
                    size: 3,
                    sha256: None,
                    md5: Some([1; 16]),
                    sha1: Some([2; 20]),
                },
            )
            .expect("store");
        let index = DestinationIndex::build(&dest_dir, DuplicateMode::Modern)
            .expect("build")
            .with_cache(cache.clone());
        index.equals_file(&archived, &archived).expect("equals");
        let hit = cache
            .lookup(&archived, 3, meta.modified().expect("mtime"))
            .expect("row");
        assert!(hit.sha256.is_some(), "Modern 模式补算后缓存应含 sha256");
    }

    fn hash(
        size: u64,
        sha256: Option<[u8; 32]>,
        md5: Option<[u8; 16]>,
        sha1: Option<[u8; 20]>,
    ) -> FileHash {
        FileHash {
            size,
            sha256,
            md5,
            sha1,
        }
    }

    #[test]
    fn different_size_never_equal() {
        let a = hash(1, Some([1; 32]), None, None);
        let b = hash(2, Some([1; 32]), None, None);
        assert!(!hashes_equal(&a, &b, DuplicateMode::Modern));
    }

    #[test]
    fn modern_requires_sha256_only() {
        let a = hash(1, Some([1; 32]), Some([9; 16]), None);
        let b = hash(1, Some([1; 32]), None, None);
        assert!(hashes_equal(&a, &b, DuplicateMode::Modern));
        let c = hash(1, Some([2; 32]), None, None);
        assert!(!hashes_equal(&a, &c, DuplicateMode::Modern));
    }

    #[test]
    fn legacy_requires_both_md5_and_sha1() {
        let a = hash(1, None, Some([1; 16]), Some([1; 20]));
        let same = hash(1, None, Some([1; 16]), Some([1; 20]));
        assert!(hashes_equal(&a, &same, DuplicateMode::LegacyStrict));
        // MD5 撞但 SHA1 不同 → 不算（双重校验的意义）
        let md5_only = hash(1, None, Some([1; 16]), Some([2; 20]));
        assert!(!hashes_equal(&a, &md5_only, DuplicateMode::LegacyStrict));
    }

    #[test]
    fn missing_hash_is_never_equal() {
        let a = hash(1, None, None, None);
        let b = hash(1, None, None, None);
        assert!(!hashes_equal(&a, &b, DuplicateMode::Modern));
        assert!(!hashes_equal(&a, &b, DuplicateMode::LegacyStrict));
    }

    fn write(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).expect("write fixture");
        p
    }

    #[test]
    fn index_finds_duplicate_and_memoizes() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(dest.join("sub")).expect("mkdir");
        write(&dest, "a.jpg", b"content-x");
        write(&dest.join("sub"), "b.jpg", b"content-x");
        write(&dest, "c.jpg", b"different!");
        let source = write(tmp.path(), "source.jpg", b"content-x");

        let index = DestinationIndex::build(&dest, DuplicateMode::Modern).expect("build");
        assert_eq!(index.len(), 3);

        let found = index
            .find_duplicate(&source, 9)
            .expect("find")
            .expect("duplicate exists");
        assert!(found.ends_with("a.jpg") || found.ends_with("b.jpg"));

        // 尺寸无候选：不读源文件也直接 None
        let big = write(tmp.path(), "big.jpg", b"0123456789abcdef");
        assert!(index.find_duplicate(&big, 16).expect("find").is_none());

        // 源在目标树内（原地整理）：不匹配自身
        let inside = dest.join("a.jpg");
        let found_inside = index
            .find_duplicate(&inside, 9)
            .expect("find")
            .expect("matches the other copy");
        assert!(found_inside.ends_with("b.jpg"));
    }

    #[test]
    fn equals_file_distinguishes_content() {
        let tmp = tempfile::tempdir().expect("tmp");
        let same = write(tmp.path(), "same.jpg", b"same-size!!");
        let diff = write(tmp.path(), "diff.jpg", b"same-size?!");
        let source = write(tmp.path(), "s.jpg", b"same-size!!");
        let index = DestinationIndex::build(tmp.path(), DuplicateMode::Modern).expect("build");
        assert!(index.equals_file(&source, &same).expect("eq"));
        assert!(!index.equals_file(&source, &diff).expect("neq"));
    }

    #[test]
    fn legacy_mode_index_works() {
        let tmp = tempfile::tempdir().expect("tmp");
        write(tmp.path(), "old.jpg", b"legacy-content");
        let source = write(tmp.path(), "new.jpg", b"legacy-content");
        let index =
            DestinationIndex::build(tmp.path(), DuplicateMode::LegacyStrict).expect("build");
        assert!(index.find_duplicate(&source, 14).expect("find").is_some());
    }
}
