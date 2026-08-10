//! 安全文件操作原语（需求 §十四 / §十五）。
//!
//! 不可违背的红线：
//! 1. 目标文件永远经 `.archimages-*.tmp` 临时文件 + rename 落位，
//!    崩溃不会留下半个「最终文件名」；
//! 2. rename 前目标已存在 → 报错，绝不静默覆盖；
//! 3. 源文件只在「目标已复制 + 存在 + 大小一致 + 内容哈希一致」之后删除；
//! 4. 哈希读不出来 → 一律不删源文件；
//! 5. io 错误向 Permission 变体归类，单文件失败不拖垮整批。

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use super::duplicate::hashes_equal;
use super::hash::hash_file;
use crate::error::{AppError, Result};
use crate::models::duplicate::DuplicateMode;

pub const TEMP_PREFIX: &str = ".archimages-";
pub const TEMP_SUFFIX: &str = ".tmp";
/// 复制缓冲：1MB，兼顾吞吐与内存（哈希另有 8MB 缓冲，各司其职）。
const COPY_BUFFER_SIZE: usize = 1024 * 1024;
/// 冲突改名上限：filename_1 … filename_9999。
const MAX_COLLISION_RENAME: u32 = 9999;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveOutcome {
    /// 同文件系统直接 rename（原子、零拷贝）。
    Renamed,
    /// 跨文件系统回退为 copy + verify + delete。
    CopiedAcrossDevices,
}

/// io 错误归类：权限问题单独成变体，供 UI 显示 PermissionDenied 并继续。
fn map_io(err: io::Error) -> AppError {
    if err.kind() == io::ErrorKind::PermissionDenied {
        AppError::Permission(err.to_string())
    } else {
        AppError::Io(err)
    }
}

pub fn is_temp_name(name: &str) -> bool {
    name.starts_with(TEMP_PREFIX) && name.ends_with(TEMP_SUFFIX)
}

fn temp_path_for(target: &Path) -> PathBuf {
    let name = format!(
        "{TEMP_PREFIX}{}{TEMP_SUFFIX}",
        uuid::Uuid::new_v4().simple()
    );
    match target.parent() {
        Some(dir) => dir.join(name),
        None => PathBuf::from(name),
    }
}

fn cleanup_temp(temp: &Path) {
    if let Err(e) = fs::remove_file(temp) {
        if e.kind() != io::ErrorKind::NotFound {
            warn!(temp = %temp.to_string_lossy(), error = %e, "failed to remove temp file");
        }
    }
}

/// 尽力而为的目录 fsync：保证 rename 的目录项落盘。不支持的文件系统记 debug。
fn sync_parent_dir(path: &Path) {
    let Some(dir) = path.parent() else { return };
    match File::open(dir).and_then(|f| f.sync_all()) {
        Ok(()) => {}
        Err(e) => debug!(dir = %dir.to_string_lossy(), error = %e, "dir fsync unsupported"),
    }
}

/// 保留源文件 mtime：照片归档依赖文件时间排查问题。失败仅告警。
fn preserve_mtime(target: &Path, source_mtime: std::time::SystemTime) {
    let times = std::fs::FileTimes::new().set_modified(source_mtime);
    let result = OpenOptions::new()
        .write(true)
        .open(target)
        .and_then(|f| f.set_times(times));
    if let Err(e) = result {
        warn!(path = %target.to_string_lossy(), error = %e, "preserve mtime failed");
    }
}

/// 原子复制：temp → flush → fsync → （可选）哈希校验 → rename。
/// verify 为 Some(mode) 时按查重模式对 temp 做内容级校验。
pub fn atomic_copy(source: &Path, target: &Path, verify: Option<DuplicateMode>) -> Result<()> {
    let source_meta = fs::metadata(source).map_err(map_io)?;
    let parent = target
        .parent()
        .ok_or_else(|| AppError::InvalidPath("target has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(map_io)?;

    if target.exists() {
        return Err(AppError::TargetExists(
            target.to_string_lossy().into_owned(),
        ));
    }

    let temp = temp_path_for(target);
    let result = copy_stream(source, &temp, source_meta.len())
        .and_then(|_| verify_temp(source, &temp, verify))
        .and_then(|_| rename_final(&temp, target));

    if let Err(e) = result {
        cleanup_temp(&temp);
        return Err(e);
    }

    preserve_mtime(
        target,
        source_meta
            .modified()
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
    );
    sync_parent_dir(target);
    Ok(())
}

fn copy_stream(source: &Path, temp: &Path, expected_size: u64) -> Result<()> {
    let mut reader = File::open(source).map_err(map_io)?;
    let mut writer = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temp)
        .map_err(map_io)?;

    let mut buffer = vec![0u8; COPY_BUFFER_SIZE];
    let mut written: u64 = 0;
    loop {
        let n = reader.read(&mut buffer).map_err(map_io)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n]).map_err(map_io)?;
        written += n as u64;
    }
    writer.flush().map_err(map_io)?;
    writer.sync_all().map_err(map_io)?;
    drop(writer);

    if written != expected_size {
        return Err(AppError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "copied {written} bytes, expected {expected_size} (source changed during copy?)"
            ),
        )));
    }
    let temp_size = fs::metadata(temp).map_err(map_io)?.len();
    if temp_size != expected_size {
        return Err(AppError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("temp file size {temp_size} != expected {expected_size}"),
        )));
    }
    Ok(())
}

fn verify_temp(source: &Path, temp: &Path, verify: Option<DuplicateMode>) -> Result<()> {
    let Some(mode) = verify else { return Ok(()) };
    let source_hash = hash_file(source, mode)?;
    let temp_hash = hash_file(temp, mode)?;
    if hashes_equal(&source_hash, &temp_hash, mode) {
        Ok(())
    } else {
        Err(AppError::Hash(format!(
            "post-copy verification failed: {}",
            temp.to_string_lossy()
        )))
    }
}

/// rename 落位前的最后一道防线：目标若在此期间出现则拒绝覆盖。
fn rename_final(temp: &Path, target: &Path) -> Result<()> {
    if target.exists() {
        return Err(AppError::TargetExists(
            target.to_string_lossy().into_owned(),
        ));
    }
    fs::rename(temp, target).map_err(map_io)
}

/// CopyVerifyDelete（需求 §十四）：复制校验通过后，最终文件复核存在与大小，再删源。
pub fn copy_verify_delete(source: &Path, target: &Path, mode: DuplicateMode) -> Result<()> {
    let source_size = fs::metadata(source).map_err(map_io)?.len();
    atomic_copy(source, target, Some(mode))?;

    // 删源前的最终复核：最终文件必须真实存在且大小一致
    let final_meta = fs::metadata(target).map_err(map_io)?;
    if final_meta.len() != source_size {
        return Err(AppError::Hash(format!(
            "final file size mismatch, source NOT deleted: {}",
            target.to_string_lossy()
        )));
    }
    fs::remove_file(source).map_err(map_io)?;
    info!(
        event = "SourceDeleted",
        source = %source.to_string_lossy(),
        target = %target.to_string_lossy(),
        "source removed after verified copy"
    );
    Ok(())
}

/// Move：同盘原子 rename；跨盘自动回退 copy + verify + delete。
pub fn safe_move(source: &Path, target: &Path, mode: DuplicateMode) -> Result<MoveOutcome> {
    let parent = target
        .parent()
        .ok_or_else(|| AppError::InvalidPath("target has no parent".to_string()))?;
    fs::create_dir_all(parent).map_err(map_io)?;
    if target.exists() {
        return Err(AppError::TargetExists(
            target.to_string_lossy().into_owned(),
        ));
    }
    match fs::rename(source, target) {
        Ok(()) => {
            sync_parent_dir(target);
            Ok(MoveOutcome::Renamed)
        }
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            copy_verify_delete(source, target, mode)?;
            Ok(MoveOutcome::CopiedAcrossDevices)
        }
        Err(e) => Err(map_io(e)),
    }
}

/// §十五：目标已有完全重复文件时，Move/CVD 模式允许删源——但必须重新核验。
/// 任何核验失败/读不出来都返回 Ok(false)，源文件原地保留。
pub fn delete_verified_duplicate(
    source: &Path,
    existing: &Path,
    mode: DuplicateMode,
) -> Result<bool> {
    if !existing.exists() {
        warn!(existing = %existing.to_string_lossy(), "duplicate target vanished; source kept");
        return Ok(false);
    }
    let source_size = fs::metadata(source).map_err(map_io)?.len();
    let existing_size = match fs::metadata(existing) {
        Ok(m) => m.len(),
        Err(e) => {
            warn!(error = %e, "cannot stat duplicate target; source kept");
            return Ok(false);
        }
    };
    if source_size != existing_size {
        warn!("duplicate size mismatch; source kept");
        return Ok(false);
    }
    let verified = match (hash_file(source, mode), hash_file(existing, mode)) {
        (Ok(a), Ok(b)) => hashes_equal(&a, &b, mode),
        _ => false, // 哈希读不出来 → 绝不删
    };
    if !verified {
        warn!(source = %source.to_string_lossy(), "duplicate verification failed; source kept");
        return Ok(false);
    }
    fs::remove_file(source).map_err(map_io)?;
    info!(
        event = "DuplicateVerified",
        source = %source.to_string_lossy(),
        existing = %existing.to_string_lossy(),
        "verified duplicate"
    );
    info!(
        event = "SourceDeleted",
        source = %source.to_string_lossy(),
        "duplicate source removed"
    );
    Ok(true)
}

/// 无序号模板的冲突改名：name.ext → name_1.ext（需求 §十三 默认策略）。
pub fn collision_free_name(target: &Path, exists: impl Fn(&Path) -> bool) -> Result<PathBuf> {
    if !exists(target) {
        return Ok(target.to_path_buf());
    }
    let parent = target.parent().map(Path::to_path_buf).unwrap_or_default();
    let stem = target
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let ext = target.extension().map(|e| e.to_string_lossy().into_owned());
    for i in 1..=MAX_COLLISION_RENAME {
        let name = match &ext {
            Some(e) => format!("{stem}_{i}.{e}"),
            None => format!("{stem}_{i}"),
        };
        let candidate = parent.join(name);
        if !exists(&candidate) {
            return Ok(candidate);
        }
    }
    Err(AppError::Task(format!(
        "collision rename exhausted for {}",
        target.to_string_lossy()
    )))
}

/// 清理上次崩溃残留的临时文件。返回清理数量。
/// 注意：临时文件名以 `.` 开头，扫描器的隐藏文件过滤反而会漏掉它们，这里全量遍历。
pub fn sweep_stale_temps(root: &Path) -> u64 {
    let mut removed = 0u64;
    for entry in walkdir::WalkDir::new(root).into_iter() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "sweep walk error");
                continue;
            }
        };
        if entry.file_type().is_file()
            && is_temp_name(&entry.file_name().to_string_lossy())
            && fs::remove_file(entry.path()).is_ok()
        {
            removed += 1;
            info!(path = %entry.path().to_string_lossy(), "swept stale temp file");
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }

    #[test]
    fn atomic_copy_copies_bytes_and_leaves_no_temp() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("a.jpg");
        write(&src, b"photo-bytes");
        let target = tmp.path().join("dest/2017/a.jpg");

        atomic_copy(&src, &target, Some(DuplicateMode::Modern)).expect("copy");
        assert_eq!(std::fs::read(&target).expect("read"), b"photo-bytes");
        assert!(src.exists(), "copy keeps source");
        let leftovers: Vec<_> = std::fs::read_dir(target.parent().expect("p"))
            .expect("readdir")
            .filter_map(|e| e.ok())
            .filter(|e| is_temp_name(&e.file_name().to_string_lossy()))
            .collect();
        assert!(leftovers.is_empty(), "no temp file remains");
    }

    #[test]
    fn atomic_copy_refuses_existing_target() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("a.jpg");
        write(&src, b"new");
        let target = tmp.path().join("dest/a.jpg");
        write(&target, b"old");
        let err = atomic_copy(&src, &target, None).expect_err("must refuse");
        assert_eq!(err.user_key(), "error.invalidPath");
        assert_eq!(std::fs::read(&target).expect("read"), b"old");
    }

    #[test]
    fn atomic_copy_missing_source_is_io_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let target = tmp.path().join("dest/a.jpg");
        assert!(atomic_copy(Path::new("/nope.jpg"), &target, None).is_err());
        assert!(!target.exists());
    }

    #[test]
    fn copy_verify_delete_removes_source_after_verification() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("a.jpg");
        write(&src, b"verified-content");
        let target = tmp.path().join("dest/a.jpg");

        copy_verify_delete(&src, &target, DuplicateMode::Modern).expect("cvd");
        assert!(!src.exists(), "source removed");
        assert_eq!(std::fs::read(&target).expect("read"), b"verified-content");
    }

    #[test]
    fn safe_move_renames_on_same_device() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("a.jpg");
        write(&src, b"move-me");
        let target = tmp.path().join("dest/a.jpg");
        let outcome = safe_move(&src, &target, DuplicateMode::Modern).expect("move");
        assert_eq!(outcome, MoveOutcome::Renamed);
        assert!(!src.exists());
        assert_eq!(std::fs::read(&target).expect("read"), b"move-me");
    }

    #[test]
    fn safe_move_refuses_existing_target_without_touching_source() {
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("a.jpg");
        write(&src, b"keep");
        let target = tmp.path().join("dest/a.jpg");
        write(&target, b"occupied");
        assert!(safe_move(&src, &target, DuplicateMode::Modern).is_err());
        assert!(src.exists());
        assert_eq!(std::fs::read(&target).expect("read"), b"occupied");
    }

    #[test]
    fn delete_verified_duplicate_only_when_content_verified() {
        let tmp = tempfile::tempdir().expect("tmp");
        // 内容一致 → 删
        let src = tmp.path().join("s1.jpg");
        write(&src, b"dup");
        let existing = tmp.path().join("dest/e1.jpg");
        write(&existing, b"dup");
        assert!(delete_verified_duplicate(&src, &existing, DuplicateMode::Modern).expect("ok"));
        assert!(!src.exists());

        // 内容不同 → 不删
        let src2 = tmp.path().join("s2.jpg");
        write(&src2, b"aaa");
        let existing2 = tmp.path().join("dest/e2.jpg");
        write(&existing2, b"bbb");
        assert!(!delete_verified_duplicate(&src2, &existing2, DuplicateMode::Modern).expect("ok"));
        assert!(src2.exists(), "different content: source kept");

        // 目标消失 → 不删
        let src3 = tmp.path().join("s3.jpg");
        write(&src3, b"ccc");
        let ghost = tmp.path().join("dest/ghost.jpg");
        assert!(!delete_verified_duplicate(&src3, &ghost, DuplicateMode::Modern).expect("ok"));
        assert!(src3.exists());
    }

    #[test]
    fn collision_free_name_increments() {
        let tmp = tempfile::tempdir().expect("tmp");
        let occupied = tmp.path().join("20171130_0001.jpg");
        write(&occupied, b"x");
        let also = tmp.path().join("20171130_0001_1.jpg");
        write(&also, b"x");
        let free = collision_free_name(&occupied, |p| p.exists()).expect("name");
        assert_eq!(free, tmp.path().join("20171130_0001_2.jpg"));

        let untouched = tmp.path().join("fresh.jpg");
        assert_eq!(
            collision_free_name(&untouched, |p| p.exists()).expect("free"),
            untouched
        );
    }

    #[test]
    fn sweep_removes_only_temp_files() {
        let tmp = tempfile::tempdir().expect("tmp");
        write(&tmp.path().join(".archimages-deadbeef.tmp"), b"partial");
        write(&tmp.path().join("sub/.archimages-cafe.tmp"), b"partial");
        write(&tmp.path().join("real.jpg"), b"photo");
        let removed = sweep_stale_temps(tmp.path());
        assert_eq!(removed, 2);
        assert!(tmp.path().join("real.jpg").exists());
    }

    #[cfg(unix)]
    #[test]
    fn permission_denied_maps_to_permission_error() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().expect("tmp");
        let src = tmp.path().join("a.jpg");
        write(&src, b"x");
        let locked = tmp.path().join("locked");
        std::fs::create_dir_all(&locked).expect("mkdir");
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        let target = locked.join("a.jpg");
        let err = atomic_copy(&src, &target, None).expect_err("permission");
        // 恢复权限以便 tempdir 清理
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("restore");
        assert_eq!(err.user_key(), "error.permission");
    }
}
