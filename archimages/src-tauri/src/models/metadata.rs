//! EXIF 元数据与地理位置模型。
//!
//! 关键约束：EXIF 拍摄时间是「相机本地朴素时间」，没有时区。
//! 一律用 `NaiveDateTime` 建模，绝不当 UTC 处理，否则跨时区整理会错位目录日期。

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GpsCoordinate {
    pub latitude: f64,
    pub longitude: f64,
}

/// 拍摄时间来源，fallback 必须可审计。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TakenAtSource {
    ExifDateTimeOriginal,
    ExifCreateDate,
    FileModifiedFallback,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoMetadata {
    pub taken_at: Option<NaiveDateTime>,
    pub taken_at_source: Option<TakenAtSource>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub gps: Option<GpsCoordinate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocationSource {
    Google,
    /// 无 API 时退化为坐标字符串（如 22.3193_114.1694）。
    Coordinates,
    #[default]
    Fallback,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLocation {
    pub country: Option<String>,
    pub province: Option<String>,
    pub city: Option<String>,
    pub district: Option<String>,
    pub formatted_address: Option<String>,
    pub source: LocationSource,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_defaults_are_empty() {
        let m = PhotoMetadata::default();
        assert!(m.taken_at.is_none());
        assert!(m.camera_model.is_none());
        assert!(m.gps.is_none());
    }

    #[test]
    fn taken_at_source_serializes_camel_case() {
        let json = serde_json::to_string(&TakenAtSource::FileModifiedFallback).expect("serialize");
        assert_eq!(json, "\"fileModifiedFallback\"");
    }

    #[test]
    fn location_default_source_is_fallback() {
        let loc = ResolvedLocation::default();
        assert_eq!(loc.source, LocationSource::Fallback);
    }
}
