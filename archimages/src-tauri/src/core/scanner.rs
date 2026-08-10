//! 递归照片扫描。
//!
//! 原则：不读取文件内容，只收集 `PhotoFile` 元信息；结果通过回调流式
//! 产出，10 万级目录不会在内存里堆积 Vec。取消通过 `should_stop` 钩子，
//! Phase 9 接入 CancellationToken。

use std::path::Path;
use std::time::SystemTime;

use tracing::warn;
use walkdir::WalkDir;

use crate::error::{AppError, Result};
use crate::models::photo::PhotoFile;

/// 扩展名白名单（小写、不含点）。
///
/// 来源（2026-08-10 核实，见 Phase 0 架构文档）：
/// - nom-exif 3.6.2 官方支持：jpg/jpeg/png/heic/heif/avif/tif/tiff/cr3/raf/iiq
/// - rawler 0.7.2 补充 RAW：nef/nrw/cr2/crw/arw/dng/orf/rw2/pef/srw/erf/kdc/mos/mef/rwl
///
/// Sigma X3F 双引擎均不支持，不在列表内（已知限制）。
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "heic", "heif", "avif", "tif", "tiff", //
    "cr3", "raf", "iiq", //
    "nef", "nrw", "cr2", "crw", "arw", "dng", "orf", "rw2", "pef", "srw", "erf", "kdc", "mos",
    "mef", "rwl",
];

pub fn is_supported_extension(lowercase_ext: &str) -> bool {
    SUPPORTED_EXTENSIONS.contains(&lowercase_ext)
}

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    pub include_subfolders: bool,
}

/// 与需求默认一致：包含子目录。
impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            include_subfolders: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanStats {
    pub found: u64,
    pub skipped_hidden: u64,
    pub skipped_unsupported: u64,
    pub skipped_non_file: u64,
    pub errors: u64,
    pub cancelled: bool,
}

/// 文件名级别的忽略规则：点开头（隐藏文件、.DS_Store、应用临时文件
/// `.archimages-*.tmp`）以及 Windows 系统残留。SQLite 等其余类型由
/// 扩展名白名单统一拦截。
fn is_ignorable_name(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    lower == "thumbs.db" || lower == "desktop.ini"
}

/// 流式扫描 `root`。每发现一张合格照片调用一次 `on_photo`；
/// `should_stop` 返回 true 时尽快停止（最多延迟 CANCEL_CHECK_INTERVAL 个条目）。
pub fn scan(
    root: &Path,
    options: &ScanOptions,
    should_stop: &dyn Fn() -> bool,
    mut on_photo: impl FnMut(PhotoFile),
) -> Result<ScanStats> {
    const CANCEL_CHECK_INTERVAL: u64 = 256;

    if !root.is_dir() {
        return Err(AppError::InvalidPath(format!(
            "source root is not a directory: {}",
            root.to_string_lossy()
        )));
    }

    let max_depth = if options.include_subfolders {
        usize::MAX
    } else {
        1
    };

    let mut stats = ScanStats::default();
    let mut visited: u64 = 0;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .max_depth(max_depth)
        .into_iter()
        // 隐藏目录整棵剪掉，不向下遍历。
        .filter_entry(|entry| {
            let hidden = entry
                .file_name()
                .to_str()
                .map(|n| n.starts_with('.'))
                .unwrap_or(false);
            !(hidden && entry.depth() > 0)
        });

    for item in walker {
        visited += 1;
        if visited.is_multiple_of(CANCEL_CHECK_INTERVAL) && should_stop() {
            stats.cancelled = true;
            break;
        }

        let entry = match item {
            Ok(e) => e,
            Err(e) => {
                stats.errors += 1;
                warn!("scan entry error: {e}");
                continue;
            }
        };

        let file_name = entry.file_name().to_string_lossy();
        if is_ignorable_name(&file_name) {
            stats.skipped_hidden += 1;
            continue;
        }

        // 不跟随符号链接：symlink 的 file_type 非 is_file，自然跳过。
        if !entry.file_type().is_file() {
            stats.skipped_non_file += 1;
            continue;
        }

        let extension = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let Some(extension) = extension else {
            stats.skipped_unsupported += 1;
            continue;
        };
        if !is_supported_extension(&extension) {
            stats.skipped_unsupported += 1;
            continue;
        }

        let metadata = match entry.path().metadata() {
            Ok(m) => m,
            Err(e) => {
                stats.errors += 1;
                warn!(path = %entry.path().to_string_lossy(), "stat failed: {e}");
                continue;
            }
        };
        let modified_time = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        stats.found += 1;
        on_photo(PhotoFile {
            path: entry.path().to_path_buf(),
            size: metadata.len(),
            extension,
            modified_time,
        });
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let p = dir.join(name);
        fs::write(&p, b"x").expect("write fixture");
        p
    }

    fn collect(root: &Path, options: &ScanOptions) -> (ScanStats, Vec<PhotoFile>) {
        let mut out = Vec::new();
        let stats = scan(root, options, &|| false, |f| out.push(f)).expect("scan");
        (stats, out)
    }

    #[test]
    fn recursive_scan_filters_by_allowlist_and_ignores() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        touch(root, "a.jpg");
        touch(root, "b.JPEG"); // 大写扩展名
        touch(root, "c.nef");
        touch(root, "notes.txt");
        touch(root, ".DS_Store");
        touch(root, "Thumbs.db");
        touch(root, "archive.db");
        touch(root, ".archimages-9f3a2b1c.tmp");

        let sub = root.join("sub");
        fs::create_dir(&sub).expect("mkdir");
        touch(&sub, "d.png");
        let hidden = root.join(".hidden");
        fs::create_dir(&hidden).expect("mkdir hidden");
        touch(&hidden, "e.jpg");

        let (stats, files) = collect(root, &ScanOptions::default());
        assert_eq!(stats.found, 4);
        let mut names: Vec<String> = files.iter().map(|f| f.file_name_lossy()).collect();
        names.sort();
        assert_eq!(names, vec!["a.jpg", "b.JPEG", "c.nef", "d.png"]);
        assert!(stats.skipped_unsupported >= 2); // txt + db
        assert!(!stats.cancelled);
    }

    #[test]
    fn non_recursive_stops_at_top_level() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        touch(root, "a.jpg");
        let sub = root.join("sub");
        fs::create_dir(&sub).expect("mkdir");
        touch(&sub, "b.jpg");

        let (stats, files) = collect(
            root,
            &ScanOptions {
                include_subfolders: false,
            },
        );
        assert_eq!(stats.found, 1);
        assert_eq!(files[0].file_name_lossy(), "a.jpg");
    }

    #[test]
    fn missing_root_is_invalid_path_error() {
        let err = scan(
            Path::new("/definitely/not/exist"),
            &ScanOptions::default(),
            &|| false,
            |_| {},
        )
        .expect_err("must fail");
        assert_eq!(err.user_key(), "error.invalidPath");
    }

    #[test]
    fn cancellation_hook_stops_scan() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for i in 0..600 {
            touch(root, &format!("f{i:04}.jpg"));
        }
        let mut count = 0u64;
        let stats = scan(
            root,
            &ScanOptions::default(),
            &|| true, // 第一次检查点就取消
            |_| count += 1,
        )
        .expect("scan");
        assert!(stats.cancelled);
        assert!(count < 600);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let real = touch(root, "real.jpg");
        std::os::unix::fs::symlink(&real, root.join("link.jpg")).expect("symlink");

        let (stats, files) = collect(root, &ScanOptions::default());
        assert_eq!(stats.found, 1);
        assert_eq!(files[0].file_name_lossy(), "real.jpg");
    }

    #[test]
    fn handles_ten_thousand_files_streaming() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        for i in 0..10_000 {
            touch(root, &format!("f{i:05}.jpg"));
        }
        let mut count = 0u64;
        let stats = scan(root, &ScanOptions::default(), &|| false, |_| count += 1).expect("scan");
        assert_eq!(stats.found, 10_000);
        assert_eq!(count, 10_000);
    }
}
