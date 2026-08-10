//! 流式文件哈希。红线（需求 §十一）：
//! - 单次顺序读取同时更新所有启用的 hasher，绝不为每个算法重读文件；
//! - 定长缓冲，绝不把整张照片加载进内存；
//! - Modern 只算 SHA-256，LegacyStrict 只算 MD5+SHA1，不算用不上的算法。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha2::Digest;

use crate::error::Result;
use crate::models::duplicate::{DuplicateMode, FileHash};

/// 需求建议值：8MB。
pub const HASH_BUFFER_SIZE: usize = 8 * 1024 * 1024;

pub fn hash_file(path: &Path, mode: DuplicateMode) -> Result<FileHash> {
    let file = File::open(path)?;
    hash_reader(file, mode)
}

pub fn hash_reader<R: Read>(mut reader: R, mode: DuplicateMode) -> Result<FileHash> {
    let mut sha256 = matches!(mode, DuplicateMode::Modern).then(sha2::Sha256::new);
    let mut md5 = matches!(mode, DuplicateMode::LegacyStrict).then(md5::Md5::new);
    let mut sha1 = matches!(mode, DuplicateMode::LegacyStrict).then(sha1::Sha1::new);

    let mut buffer = vec![0u8; HASH_BUFFER_SIZE];
    let mut size: u64 = 0;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        let chunk = &buffer[..n];
        if let Some(h) = &mut sha256 {
            h.update(chunk);
        }
        if let Some(h) = &mut md5 {
            h.update(chunk);
        }
        if let Some(h) = &mut sha1 {
            h.update(chunk);
        }
    }

    Ok(FileHash {
        size,
        sha256: sha256.map(|h| h.finalize().into()),
        md5: md5.map(|h| h.finalize().into()),
        sha1: sha1.map(|h| h.finalize().into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn modern_matches_known_sha256_vector() {
        let h = hash_reader(Cursor::new(b"abc"), DuplicateMode::Modern).expect("hash");
        assert_eq!(h.size, 3);
        assert_eq!(
            hex(&h.sha256.expect("sha256")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(h.md5.is_none() && h.sha1.is_none());
    }

    #[test]
    fn legacy_matches_known_md5_sha1_vectors() {
        let h = hash_reader(Cursor::new(b"abc"), DuplicateMode::LegacyStrict).expect("hash");
        assert_eq!(
            hex(&h.md5.expect("md5")),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            hex(&h.sha1.expect("sha1")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert!(h.sha256.is_none());
    }

    #[test]
    fn multi_chunk_streaming_matches_one_shot_digest() {
        // 9MB 数据跨多次 read，结果必须与一次性 digest 一致
        let data = vec![0xABu8; 9 * 1024 * 1024 + 7];
        let streamed = hash_reader(Cursor::new(&data), DuplicateMode::Modern).expect("hash");
        let one_shot: [u8; 32] = sha2::Sha256::digest(&data).into();
        assert_eq!(streamed.sha256.expect("sha256"), one_shot);
        assert_eq!(streamed.size, data.len() as u64);

        let streamed_legacy =
            hash_reader(Cursor::new(&data), DuplicateMode::LegacyStrict).expect("hash");
        let md5_one: [u8; 16] = md5::Md5::digest(&data).into();
        let sha1_one: [u8; 20] = sha1::Sha1::digest(&data).into();
        assert_eq!(streamed_legacy.md5.expect("md5"), md5_one);
        assert_eq!(streamed_legacy.sha1.expect("sha1"), sha1_one);
    }

    #[test]
    fn empty_file_hashes() {
        let h = hash_reader(Cursor::new(b""), DuplicateMode::Modern).expect("hash");
        assert_eq!(h.size, 0);
        // 空内容 SHA-256 是已知常量
        assert_eq!(
            hex(&h.sha256.expect("sha256")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn missing_file_is_io_error_not_panic() {
        let result = hash_file(Path::new("/nonexistent/nope.jpg"), DuplicateMode::Modern);
        assert!(result.is_err());
    }
}
