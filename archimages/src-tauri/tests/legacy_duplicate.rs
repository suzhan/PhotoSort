//! Phase 14 端到端：LegacyStrict（MD5+SHA1）查重集成测试。
//!
//! 验证 P7 实现的 LegacyStrict 路径：
//! 1. 相同内容不同文件名 → 判为重复；
//! 2. 相同大小不同内容 → 不判为重复；
//! 3. LegacyStrict 与 Modern 在相同内容上结论一致；
//! 4. DestinationIndex 在 LegacyStrict 模式下正确查重；
//! 5. 单遍流式：MD5 与 SHA1 同时产出（已在 hash.rs 单测覆盖，这里验证端到端）。

use std::fs;
use std::path::Path;

use archimages_lib::core::duplicate::{hashes_equal, DestinationIndex};
use archimages_lib::core::hash::hash_file;
use archimages_lib::models::duplicate::{DuplicateMode, FileHash};
use sha2::Digest;

fn write(path: &Path, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(path, content).expect("write");
}

#[test]
fn legacy_identical_content_different_names_are_duplicates() {
    let dir = tempfile::tempdir().expect("tmp");
    let a = dir.path().join("a.jpg");
    let b = dir.path().join("b.jpg");
    write(&a, b"identical-photo-bytes");
    write(&b, b"identical-photo-bytes");

    let ha = hash_file(&a, DuplicateMode::LegacyStrict).expect("hash a");
    let hb = hash_file(&b, DuplicateMode::LegacyStrict).expect("hash b");

    assert!(ha.md5.is_some() && ha.sha1.is_some());
    assert!(ha.sha256.is_none(), "LegacyStrict 不应产出 sha256");
    assert!(hashes_equal(&ha, &hb, DuplicateMode::LegacyStrict));
}

#[test]
fn legacy_same_size_different_content_not_duplicate() {
    let dir = tempfile::tempdir().expect("tmp");
    let a = dir.path().join("a.jpg");
    let b = dir.path().join("b.jpg");
    write(&a, b"AAAAAAAAAAAAAAAA");
    write(&b, b"BBBBBBBBBBBBBBBB"); // 同长度不同内容

    let ha = hash_file(&a, DuplicateMode::LegacyStrict).expect("hash a");
    let hb = hash_file(&b, DuplicateMode::LegacyStrict).expect("hash b");

    assert_eq!(ha.size, hb.size);
    assert!(!hashes_equal(&ha, &hb, DuplicateMode::LegacyStrict));
    assert_ne!(ha.md5, hb.md5);
    assert_ne!(ha.sha1, hb.sha1);
}

#[test]
fn legacy_and_modern_agree_on_identical_content() {
    let dir = tempfile::tempdir().expect("tmp");
    let a = dir.path().join("a.jpg");
    let b = dir.path().join("b.jpg");
    write(&a, b"same-content");
    write(&b, b"same-content");

    let ha_legacy = hash_file(&a, DuplicateMode::LegacyStrict).expect("hash a legacy");
    let hb_legacy = hash_file(&b, DuplicateMode::LegacyStrict).expect("hash b legacy");
    let ha_modern = hash_file(&a, DuplicateMode::Modern).expect("hash a modern");
    let hb_modern = hash_file(&b, DuplicateMode::Modern).expect("hash b modern");

    assert!(hashes_equal(
        &ha_legacy,
        &hb_legacy,
        DuplicateMode::LegacyStrict
    ));
    assert!(hashes_equal(&ha_modern, &hb_modern, DuplicateMode::Modern));
}

#[test]
fn legacy_and_modern_agree_on_different_content() {
    let dir = tempfile::tempdir().expect("tmp");
    let a = dir.path().join("a.jpg");
    let b = dir.path().join("b.jpg");
    write(&a, b"content-one");
    write(&b, b"content-two");

    let ha_legacy = hash_file(&a, DuplicateMode::LegacyStrict).expect("hash a");
    let hb_legacy = hash_file(&b, DuplicateMode::LegacyStrict).expect("hash b");
    let ha_modern = hash_file(&a, DuplicateMode::Modern).expect("hash a modern");
    let hb_modern = hash_file(&b, DuplicateMode::Modern).expect("hash b modern");

    assert!(!hashes_equal(
        &ha_legacy,
        &hb_legacy,
        DuplicateMode::LegacyStrict
    ));
    assert!(!hashes_equal(&ha_modern, &hb_modern, DuplicateMode::Modern));
}

#[test]
fn destination_index_finds_duplicate_in_legacy_mode() {
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("dest");
    let archived = dest.join("archived.jpg");
    write(&archived, b"legacy-duplicate-content");

    let incoming = dir.path().join("incoming.jpg");
    write(&incoming, b"legacy-duplicate-content");

    let index = DestinationIndex::build(&dest, DuplicateMode::LegacyStrict).expect("build");
    assert!(!index.is_empty(), "index should contain archived file");
    let src_size = std::fs::metadata(&incoming).expect("stat").len();
    let found = index
        .find_duplicate(&incoming, src_size)
        .expect("find")
        .expect("should find duplicate");
    assert_eq!(found, archived);
}

#[test]
fn destination_index_no_false_positive_in_legacy_mode() {
    let dir = tempfile::tempdir().expect("tmp");
    let dest = dir.path().join("dest");
    let archived = dest.join("archived.jpg");
    write(&archived, b"legacy-unique-content-A");

    let incoming = dir.path().join("incoming.jpg");
    write(&incoming, b"legacy-unique-content-B"); // 同长度不同内容

    let index = DestinationIndex::build(&dest, DuplicateMode::LegacyStrict).expect("build");
    let src_size = std::fs::metadata(&incoming).expect("stat").len();
    let found = index.find_duplicate(&incoming, src_size).expect("find");
    assert!(found.is_none(), "同长度不同内容不得判为重复");
}

#[test]
fn legacy_hash_missing_digests_not_treated_as_equal() {
    // 模拟哈希缺失：保守原则，绝不判等
    let a = FileHash {
        size: 10,
        md5: None,
        sha1: Some([0; 20]),
        sha256: None,
    };
    let b = FileHash {
        size: 10,
        md5: Some([0; 16]),
        sha1: None,
        sha256: None,
    };
    assert!(!hashes_equal(&a, &b, DuplicateMode::LegacyStrict));
}

#[test]
fn legacy_single_pass_produces_both_digests() {
    // 验证单遍读取同时产出 MD5 + SHA1（不重读文件）
    let dir = tempfile::tempdir().expect("tmp");
    let path = dir.path().join("photo.jpg");
    let data = vec![0xCDu8; 9 * 1024 * 1024 + 13]; // 跨多次 8MB buffer
    write(&path, &data);

    let h = hash_file(&path, DuplicateMode::LegacyStrict).expect("hash");
    assert!(h.md5.is_some() && h.sha1.is_some());
    assert!(h.sha256.is_none());

    // 与独立一次性 digest 对照
    let md5_one: [u8; 16] = md5::Md5::digest(&data).into();
    let sha1_one: [u8; 20] = sha1::Sha1::digest(&data).into();
    assert_eq!(h.md5.unwrap(), md5_one);
    assert_eq!(h.sha1.unwrap(), sha1_one);
}
