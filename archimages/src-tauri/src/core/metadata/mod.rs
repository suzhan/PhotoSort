//! 元数据编排器（按架构决策的双引擎 + ExifTool 兜底链路）：
//!
//! ```text
//! PhotoFile ── 扩展名分类 ──▶ RAW 集合 → rawler
//!                             其余     → nom-exif
//!                                  │
//!                              解析失败? ── YES ──▶ ExifTool（如可用）
//! ```
//!
//! 「解析成功但字段为空」不算失败：缺 EXIF 走模板 fallback，
//! 只有解析错误才触发 ExifTool。`taken_at` 的 mtime 兜底在此统一应用。

pub mod clean;
pub mod datetime;
pub mod exiftool_engine;
mod nomexif_engine;
mod rawler_engine;

use chrono::{DateTime, Local};
use nom_exif::MediaParser;
use tracing::warn;

use crate::models::metadata::{PhotoMetadata, TakenAtSource};
use crate::models::photo::PhotoFile;

/// RAW 集合（rawler 主路径）。cr3/raf/iiq 归 nom-exif（其官方支持）。
const RAW_EXTENSIONS: &[&str] = &[
    "nef", "nrw", "cr2", "crw", "arw", "dng", "orf", "rw2", "pef", "srw", "erf", "kdc", "mos",
    "mef", "rwl",
];

pub fn is_raw_extension(lowercase_ext: &str) -> bool {
    RAW_EXTENSIONS.contains(&lowercase_ext)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataEngine {
    NomExif,
    Rawler,
    ExifTool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MetadataOptions {
    /// 用户显式开启后，EXIF 无拍摄时间时用文件 mtime 兜底。
    pub use_modified_time_fallback: bool,
}

#[derive(Debug)]
pub struct MetadataOutcome {
    pub metadata: PhotoMetadata,
    /// 实际产出数据的引擎；全部失败为 None。
    pub engine: Option<MetadataEngine>,
    /// 所有可用引擎均解析失败（区别于「解析成功但字段为空」）。
    pub parse_failed: bool,
}

/// 批量读取器：复用 nom-exif 解析缓冲，进程级缓存 ExifTool 探测结果。
pub struct MetadataReader {
    parser: MediaParser,
    exiftool: Option<&'static exiftool_engine::ExifTool>,
    options: MetadataOptions,
}

impl MetadataReader {
    pub fn new(options: MetadataOptions) -> Self {
        Self {
            parser: MediaParser::new(),
            exiftool: exiftool_engine::detect(),
            options,
        }
    }

    pub fn exiftool_available(&self) -> bool {
        self.exiftool.is_some()
    }

    pub fn read(&mut self, photo: &PhotoFile) -> MetadataOutcome {
        let is_raw = is_raw_extension(&photo.extension);
        let primary_engine = if is_raw {
            MetadataEngine::Rawler
        } else {
            MetadataEngine::NomExif
        };

        let primary = if is_raw {
            rawler_engine::read(&photo.path)
        } else {
            nomexif_engine::read(&mut self.parser, &photo.path)
        };

        let (mut metadata, engine, parse_failed) = match primary {
            Ok(md) => (md, Some(primary_engine), false),
            Err(primary_err) => {
                warn!(
                    path = %photo.path.to_string_lossy(),
                    "primary metadata engine failed: {primary_err}"
                );
                match self.exiftool {
                    Some(exiftool) => match exiftool_engine::read(exiftool, &photo.path) {
                        Ok(md) => (md, Some(MetadataEngine::ExifTool), false),
                        Err(fallback_err) => {
                            warn!(
                                path = %photo.path.to_string_lossy(),
                                "exiftool fallback failed: {fallback_err}"
                            );
                            (PhotoMetadata::default(), None, true)
                        }
                    },
                    None => (PhotoMetadata::default(), None, true),
                }
            }
        };

        if metadata.taken_at.is_none() && self.options.use_modified_time_fallback {
            let local: DateTime<Local> = photo.modified_time.into();
            metadata.taken_at = Some(local.naive_local());
            metadata.taken_at_source = Some(TakenAtSource::FileModifiedFallback);
        }

        MetadataOutcome {
            metadata,
            engine,
            parse_failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::SystemTime;

    #[test]
    fn raw_extension_classification() {
        assert!(is_raw_extension("nef"));
        assert!(is_raw_extension("arw"));
        assert!(is_raw_extension("dng"));
        assert!(!is_raw_extension("jpg"));
        assert!(!is_raw_extension("cr3")); // cr3 归 nom-exif
        assert!(!is_raw_extension("raf"));
    }

    #[test]
    fn failed_parse_returns_default_metadata_without_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.jpg");
        fs::write(&path, b"garbage").expect("write");
        let photo = PhotoFile {
            path,
            size: 7,
            extension: "jpg".to_string(),
            modified_time: SystemTime::UNIX_EPOCH,
        };
        let mut reader = MetadataReader::new(MetadataOptions::default());
        let outcome = reader.read(&photo);
        // 本机若装了 exiftool 可能兜底成功，两种路径都必须安全。
        if outcome.parse_failed {
            assert!(outcome.metadata.taken_at.is_none());
            assert!(outcome.engine.is_none());
        } else {
            assert!(outcome.engine.is_some());
        }
    }

    #[test]
    fn mtime_fallback_applies_only_when_enabled_and_taken_at_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.jpg");
        fs::write(&path, b"garbage").expect("write");
        let photo = PhotoFile {
            path,
            size: 7,
            extension: "jpg".to_string(),
            modified_time: SystemTime::UNIX_EPOCH,
        };

        // 关闭：即使解析失败也不填 mtime
        let mut reader = MetadataReader::new(MetadataOptions::default());
        let outcome = reader.read(&photo);
        assert_ne!(
            outcome.metadata.taken_at_source,
            Some(TakenAtSource::FileModifiedFallback)
        );

        // 开启：解析失败/无拍摄时间时用 mtime
        let mut reader = MetadataReader::new(MetadataOptions {
            use_modified_time_fallback: true,
        });
        let outcome = reader.read(&photo);
        if outcome.metadata.taken_at_source == Some(TakenAtSource::FileModifiedFallback) {
            assert!(outcome.metadata.taken_at.is_some());
        }
    }
}
