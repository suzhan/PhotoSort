//! rawler 引擎：RAW 格式（NEF/CR2/ARW/DNG 等）的主路径。
//!
//! 纪律（Phase 0 架构决策）：只走 `raw_metadata` 元数据路径，
//! 绝不触发像素解码；rawler 不遵循 SemVer，版本在 Cargo.toml 锁死。

use std::path::Path;

use rawler::decoders::RawDecodeParams;
use rawler::exif::{Exif, ExifGPS};
use rawler::formats::tiff::Rational;
use rawler::rawsource::RawSource;

use super::clean::clean_exif_string;
use super::datetime::parse_exif_datetime;
use crate::error::{AppError, Result};
use crate::models::metadata::{GpsCoordinate, PhotoMetadata, TakenAtSource};

fn rational_to_f64(r: Rational) -> Option<f64> {
    if r.d == 0 {
        None
    } else {
        Some(r.n as f64 / r.d as f64)
    }
}

/// [度, 分, 秒] + 半球参考 → 带符号十进制度数。
fn dms_to_decimal(dms: [Rational; 3], reference: Option<&str>) -> Option<f64> {
    let d = rational_to_f64(dms[0])?;
    let m = rational_to_f64(dms[1])?;
    let s = rational_to_f64(dms[2])?;
    let mut value = d + m / 60.0 + s / 3600.0;
    if matches!(reference, Some(r) if r.eq_ignore_ascii_case("S") || r.eq_ignore_ascii_case("W")) {
        value = -value;
    }
    Some(value)
}

fn gps(raw: Option<&ExifGPS>) -> Option<GpsCoordinate> {
    let gps = raw?;
    let latitude = dms_to_decimal(gps.gps_latitude?, gps.gps_latitude_ref.as_deref())?;
    let longitude = dms_to_decimal(gps.gps_longitude?, gps.gps_longitude_ref.as_deref())?;
    Some(GpsCoordinate {
        latitude,
        longitude,
    })
}

fn pick_datetime(exif: &Exif) -> (Option<chrono::NaiveDateTime>, Option<TakenAtSource>) {
    if let Some(raw) = &exif.date_time_original {
        if let Some(dt) = parse_exif_datetime(raw) {
            return (Some(dt), Some(TakenAtSource::ExifDateTimeOriginal));
        }
    }
    if let Some(raw) = &exif.create_date {
        if let Some(dt) = parse_exif_datetime(raw) {
            return (Some(dt), Some(TakenAtSource::ExifCreateDate));
        }
    }
    (None, None)
}

pub fn read(path: &Path) -> Result<PhotoMetadata> {
    let rawfile = RawSource::new(path).map_err(|e| AppError::Exif(format!("rawler open: {e}")))?;
    let decoder =
        rawler::get_decoder(&rawfile).map_err(|e| AppError::Exif(format!("rawler decode: {e}")))?;
    let metadata = decoder
        .raw_metadata(&rawfile, &RawDecodeParams::default())
        .map_err(|e| AppError::Exif(format!("rawler metadata: {e}")))?;

    let exif = &metadata.exif;
    let (taken_at, taken_at_source) = pick_datetime(exif);

    // 镜头信息优先用 rawler 的镜头表解析结果，其次 EXIF 原文。
    let (lens_make, lens_model) = match &metadata.lens {
        Some(lens) => (
            clean_exif_string(&lens.lens_make),
            clean_exif_string(&lens.lens_model),
        ),
        None => (
            exif.lens_make.as_deref().and_then(clean_exif_string),
            exif.lens_model.as_deref().and_then(clean_exif_string),
        ),
    };

    Ok(PhotoMetadata {
        taken_at,
        taken_at_source,
        camera_make: clean_exif_string(&metadata.make),
        camera_model: clean_exif_string(&metadata.model),
        lens_make,
        lens_model,
        gps: gps(exif.gps.as_ref()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metadata::datetime::parse_exif_datetime;

    #[test]
    fn dms_conversion_with_hemisphere_sign() {
        let lat = dms_to_decimal(
            [
                Rational { n: 22, d: 1 },
                Rational { n: 19158, d: 1000 },
                Rational { n: 0, d: 1 },
            ],
            Some("N"),
        )
        .expect("lat");
        assert!((lat - 22.3193).abs() < 1e-6);

        let south = dms_to_decimal(
            [
                Rational { n: 33, d: 1 },
                Rational { n: 51, d: 1 },
                Rational { n: 0, d: 1 },
            ],
            Some("s"),
        )
        .expect("south");
        assert!((south + 33.85).abs() < 1e-6);
    }

    #[test]
    fn zero_denominator_yields_none() {
        assert!(dms_to_decimal(
            [
                Rational { n: 1, d: 0 },
                Rational { n: 0, d: 1 },
                Rational { n: 0, d: 1 }
            ],
            Some("N"),
        )
        .is_none());
    }

    #[test]
    fn garbage_raw_is_error_not_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bad.nef");
        std::fs::write(&path, b"definitely not a raw file").expect("write");
        assert!(read(&path).is_err());
    }

    #[test]
    fn rawler_datetime_uses_shared_parser() {
        assert!(parse_exif_datetime("2017:11:30 15:22:31").is_some());
    }
}
