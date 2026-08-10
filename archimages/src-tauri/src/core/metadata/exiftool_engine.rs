//! ExifTool 兜底引擎（可选增强，非硬依赖）。
//!
//! 安全纪律：
//! - 仅当前两个纯 Rust 引擎解析失败时调用；
//! - argv 直传路径 + `--` 分隔，不经过 shell，杜绝注入；
//! - 每次调用带超时，防畸形文件挂死；
//! - 运行时探测（PATH / ARCHIMAGES_EXIFTOOL），探测不到就静默跳过，
//!   应用其余功能不受影响（原需求：最终用户不得依赖本地安装 ExifTool）。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use tracing::debug;
use wait_timeout::ChildExt;

use super::clean::clean_exif_string;
use super::datetime::parse_exif_datetime;
use crate::error::{AppError, Result};
use crate::models::metadata::{GpsCoordinate, PhotoMetadata, TakenAtSource};

const READ_TIMEOUT: Duration = Duration::from_secs(10);
const ENV_OVERRIDE: &str = "ARCHIMAGES_EXIFTOOL";

pub struct ExifTool {
    path: PathBuf,
    version: String,
}

impl ExifTool {
    pub fn version(&self) -> &str {
        &self.version
    }
}

/// 进程级探测（只跑一次）。未安装返回 None，属正常情况。
pub fn detect() -> Option<&'static ExifTool> {
    static DETECTED: OnceLock<Option<ExifTool>> = OnceLock::new();
    DETECTED.get_or_init(detect_impl).as_ref()
}

fn detect_impl() -> Option<ExifTool> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(custom) = std::env::var(ENV_OVERRIDE) {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }
    candidates.push(PathBuf::from("exiftool"));

    for candidate in candidates {
        match probe(&candidate) {
            Some(version) => {
                debug!(path = %candidate.display(), version = %version, "exiftool detected");
                return Some(ExifTool {
                    path: candidate,
                    version,
                });
            }
            None => continue,
        }
    }
    None
}

fn probe(path: &Path) -> Option<String> {
    let output = Command::new(path)
        .arg("-ver")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        None
    } else {
        Some(version)
    }
}

pub fn read(exiftool: &ExifTool, path: &Path) -> Result<PhotoMetadata> {
    let mut child = Command::new(&exiftool.path)
        .args([
            "-j",
            "-n",
            "-fast",
            "-DateTimeOriginal",
            "-CreateDate",
            "-Make",
            "-Model",
            "-LensMake",
            "-LensModel",
            "-GPSLatitude",
            "-GPSLongitude",
            "--",
        ])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Exif(format!("exiftool spawn: {e}")))?;

    let status = match child
        .wait_timeout(READ_TIMEOUT)
        .map_err(|e| AppError::Exif(format!("exiftool wait: {e}")))?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AppError::Exif(format!(
                "exiftool timeout after {}s: {}",
                READ_TIMEOUT.as_secs(),
                path.to_string_lossy()
            )));
        }
    };

    let mut stdout = Vec::new();
    if let Some(mut pipe) = child.stdout.take() {
        pipe.read_to_end(&mut stdout)
            .map_err(|e| AppError::Exif(format!("exiftool read stdout: {e}")))?;
    }
    if !status.success() {
        return Err(AppError::Exif(format!(
            "exiftool exited with {status}: {}",
            path.to_string_lossy()
        )));
    }

    parse_exiftool_json(&stdout)
}

/// 解析 `exiftool -j -n` 输出（单文件 JSON 数组）。纯函数，便于测试。
fn parse_exiftool_json(bytes: &[u8]) -> Result<PhotoMetadata> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| AppError::Exif(format!("exiftool json: {e}")))?;
    let obj = value
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::Exif("exiftool json: empty result".to_string()))?;

    let get_text = |key: &str| -> Option<String> {
        obj.get(key)
            .and_then(Value::as_str)
            .and_then(clean_exif_string)
    };
    let get_f64 = |key: &str| -> Option<f64> {
        obj.get(key).and_then(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
        })
    };

    let (taken_at, taken_at_source) = if let Some(dt) = obj
        .get("DateTimeOriginal")
        .and_then(Value::as_str)
        .and_then(parse_exif_datetime)
    {
        (Some(dt), Some(TakenAtSource::ExifDateTimeOriginal))
    } else if let Some(dt) = obj
        .get("CreateDate")
        .and_then(Value::as_str)
        .and_then(parse_exif_datetime)
    {
        (Some(dt), Some(TakenAtSource::ExifCreateDate))
    } else {
        (None, None)
    };

    let gps = match (get_f64("GPSLatitude"), get_f64("GPSLongitude")) {
        (Some(latitude), Some(longitude)) => Some(GpsCoordinate {
            latitude,
            longitude,
        }),
        _ => None,
    };

    Ok(PhotoMetadata {
        taken_at,
        taken_at_source,
        camera_make: get_text("Make"),
        camera_model: get_text("Model"),
        lens_make: get_text("LensMake"),
        lens_model: get_text("LensModel"),
        gps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[{
        "SourceFile": "/photos/DSC_1231.JPG",
        "DateTimeOriginal": "2017:11:30 15:22:31",
        "CreateDate": "2017:11:30 15:22:32",
        "Make": "NIKON CORPORATION\u0000",
        "Model": "NIKON D80",
        "LensMake": "NIKON",
        "LensModel": "18-135mm F3.5-5.6",
        "GPSLatitude": 22.3193,
        "GPSLongitude": 114.1694
    }]"#;

    #[test]
    fn parses_full_sample() {
        let md = parse_exiftool_json(SAMPLE.as_bytes()).expect("parse");
        assert_eq!(md.camera_make.as_deref(), Some("NIKON CORPORATION"));
        assert_eq!(md.camera_model.as_deref(), Some("NIKON D80"));
        assert_eq!(md.lens_model.as_deref(), Some("18-135mm F3.5-5.6"));
        assert_eq!(
            md.taken_at.map(|t| t.to_string()),
            Some("2017-11-30 15:22:31".to_string())
        );
        assert_eq!(
            md.taken_at_source,
            Some(TakenAtSource::ExifDateTimeOriginal)
        );
        let gps = md.gps.expect("gps");
        assert!((gps.latitude - 22.3193).abs() < 1e-9);
        assert!((gps.longitude - 114.1694).abs() < 1e-9);
    }

    #[test]
    fn falls_back_to_create_date() {
        let json = r#"[{"CreateDate": "2020:01:02 03:04:05"}]"#;
        let md = parse_exiftool_json(json.as_bytes()).expect("parse");
        assert_eq!(md.taken_at_source, Some(TakenAtSource::ExifCreateDate));
    }

    #[test]
    fn accepts_string_numeric_gps() {
        let json = r#"[{"GPSLatitude": "22.3193", "GPSLongitude": "114.1694"}]"#;
        let md = parse_exiftool_json(json.as_bytes()).expect("parse");
        assert!(md.gps.is_some());
    }

    #[test]
    fn rejects_empty_or_invalid_json() {
        assert!(parse_exiftool_json(b"[]").is_err());
        assert!(parse_exiftool_json(b"{}").is_err());
        assert!(parse_exiftool_json(b"not json").is_err());
    }

    #[test]
    fn detection_does_not_panic() {
        // 探测结果取决于环境（CI 通常无 exiftool），只验证不 panic。
        let _ = detect();
    }

    /// 真机回归：本机装有 exiftool 时手动跑 `cargo test -- --ignored`。
    #[test]
    #[ignore = "requires locally installed exiftool"]
    fn real_exiftool_roundtrip() {
        let Some(et) = detect() else {
            eprintln!("exiftool not installed; skip");
            return;
        };
        eprintln!("exiftool version: {}", et.version());
    }
}
