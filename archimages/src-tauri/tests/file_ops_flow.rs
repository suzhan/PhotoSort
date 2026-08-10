//! 集成测试（需求 §三十二）：程序生成 fixture，走完
//! scan → plan → execute 全链路，验证 Copy / CopyVerifyDelete / Duplicate 语义。

use std::path::{Path, PathBuf};

use archimages_lib::core::duplicate::{hashes_equal, DestinationIndex};
use archimages_lib::core::file_ops::{
    atomic_copy, copy_verify_delete, delete_verified_duplicate, sweep_stale_temps,
};
use archimages_lib::core::hash::hash_file;
use archimages_lib::core::planner::Planner;
use archimages_lib::core::scanner::{self, ScanOptions};
use archimages_lib::models::duplicate::{DuplicateMode, DuplicateResult};
use archimages_lib::models::photo::PhotoFile;
use archimages_lib::models::plan::{PhotoPlan, PlanStatus};
use archimages_lib::models::settings::AppSettings;

const MODE: DuplicateMode = DuplicateMode::Modern;

fn write_photo(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, content).expect("write");
}

fn collect_photos(root: &Path) -> Vec<PhotoFile> {
    let mut photos = Vec::new();
    scanner::scan(
        root,
        &ScanOptions {
            include_subfolders: true,
        },
        &|| false,
        |p| photos.push(p),
    )
    .expect("scan");
    photos.sort_by(|a, b| a.path.cmp(&b.path));
    photos
}

fn planned_targets(plans: &[PhotoPlan]) -> Vec<PathBuf> {
    plans.iter().map(|p| p.target_path.clone()).collect()
}

/// 搭一个三张照片的现场：A、B 为新照片，C 与档案库已有文件内容一致。
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().expect("tmp");
    let src = tmp.path().join("source");
    let dest = tmp.path().join("destination");
    write_photo(&src.join("A.jpg"), b"content-a");
    write_photo(&src.join("sub").join("B.jpg"), b"content-b");
    write_photo(&src.join("C.jpg"), b"content-c");
    write_photo(&dest.join("2017/NIKON D80/C.jpg"), b"content-c");
    (tmp, src, dest)
}

fn plan_all(src: &Path, dest: &Path) -> Vec<PhotoPlan> {
    let settings = AppSettings {
        source_directory: Some(src.to_path_buf()),
        destination_directory: Some(dest.to_path_buf()),
        ..Default::default()
    };
    let index = DestinationIndex::build(dest, MODE).expect("index");
    let planner = Planner::new(&settings).expect("planner").with_index(index);
    collect_photos(src)
        .iter()
        .map(|p| planner.plan(p, None))
        .collect()
}

#[test]
fn plan_generates_expected_targets() {
    let (_tmp, src, dest) = fixture();
    let plans = plan_all(&src, &dest);
    assert_eq!(plans.len(), 3);

    let targets = planned_targets(&plans);
    // 无 EXIF 的测试照片落入 fallback 目录
    assert!(targets.contains(&dest.join("UnknownDate/UnknownCamera/A.jpg")));
    assert!(targets.contains(&dest.join("UnknownDate/UnknownCamera/B.jpg")));

    let c = plans
        .iter()
        .find(|p| p.source.path.ends_with("C.jpg"))
        .expect("C plan");
    assert_eq!(c.status, PlanStatus::Duplicate);
    assert!(matches!(
        c.duplicate,
        DuplicateResult::ExactDuplicate { ref existing_path }
            if existing_path.ends_with("2017/NIKON D80/C.jpg")
    ));

    let a = plans
        .iter()
        .find(|p| p.source.path.ends_with("A.jpg"))
        .expect("A plan");
    // 默认目录模板含 {yyyy}：无拍摄日期 → MissingDate（分级先于 MissingExif）
    assert_eq!(a.status, PlanStatus::MissingDate);
    assert!(a.executable(), "MissingDate 可执行（fallback 兜底）");
}

#[test]
fn execute_copy_keeps_source_and_verifies_hash() {
    let (_tmp, src, dest) = fixture();
    let plans = plan_all(&src, &dest);
    assert_eq!(sweep_stale_temps(&dest), 0);

    for plan in &plans {
        match plan.status {
            PlanStatus::Duplicate => {} // Copy 模式：重复文件跳过复制，源保留
            s if plan.executable() => {
                atomic_copy(&plan.source.path, &plan.target_path, Some(MODE))
                    .unwrap_or_else(|e| panic!("copy {:?} failed: {e}", s));
            }
            s => panic!("unexpected non-executable plan: {s:?}"),
        }
    }

    let a_src = src.join("A.jpg");
    let a_dst = dest.join("UnknownDate/UnknownCamera/A.jpg");
    assert!(a_src.exists(), "Copy 模式源文件保留");
    assert!(a_dst.exists());
    let hs = hash_file(&a_src, MODE).expect("hash src");
    let hd = hash_file(&a_dst, MODE).expect("hash dst");
    assert!(hashes_equal(&hs, &hd, MODE), "copy 后哈希一致");
    assert!(src.join("C.jpg").exists(), "重复文件源保留（Copy 模式）");
    assert_eq!(sweep_stale_temps(&dest), 0, "执行后无临时文件残留");
}

#[test]
fn execute_copy_verify_delete_removes_sources_safely() {
    let (_tmp, src, dest) = fixture();
    let plans = plan_all(&src, &dest);

    for plan in &plans {
        match plan.status {
            PlanStatus::Duplicate => {
                // §十五：核验通过的重复文件，CVD 模式直接删源不复制
                let DuplicateResult::ExactDuplicate { existing_path } = &plan.duplicate else {
                    panic!("duplicate must carry evidence");
                };
                let deleted = delete_verified_duplicate(&plan.source.path, existing_path, MODE)
                    .expect("delete check");
                assert!(deleted);
            }
            s if plan.executable() => {
                copy_verify_delete(&plan.source.path, &plan.target_path, MODE)
                    .unwrap_or_else(|e| panic!("cvd {:?} failed: {e}", s));
            }
            s => panic!("unexpected non-executable plan: {s:?}"),
        }
    }

    assert!(!src.join("A.jpg").exists(), "A 源已删");
    assert!(!src.join("sub/B.jpg").exists(), "B 源已删");
    assert!(!src.join("C.jpg").exists(), "重复 C 源已删");

    let a_dst = dest.join("UnknownDate/UnknownCamera/A.jpg");
    assert_eq!(std::fs::read(&a_dst).expect("read"), b"content-a");
    assert_eq!(
        std::fs::read(dest.join("UnknownDate/UnknownCamera/B.jpg")).expect("read"),
        b"content-b"
    );
    // 目标库里 C 只有原有那一份
    assert_eq!(
        std::fs::read(dest.join("2017/NIKON D80/C.jpg")).expect("read"),
        b"content-c"
    );
    assert!(!dest.join("UnknownDate/UnknownCamera/C.jpg").exists());
    assert_eq!(sweep_stale_temps(&dest), 0);
}
