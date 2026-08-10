//! 应用设置模型与校验。API Key 不属于本结构，单独存 OS 凭据存储（Phase 12）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::{AppError, Result};
use crate::models::duplicate::DuplicateMode;

/// 默认 Copy 而非 CopyVerifyDelete：数据安全工具的最保守默认。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationMode {
    #[default]
    Copy,
    Move,
    CopyVerifyDelete,
}

/// 有 GPS 坐标但无 Google API（或未配置）时，路径变量如何取值（需求 §七 三选一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpsNoApiMode {
    /// 忽略 GPS：不进入路径，也不产生 MissingGps 状态。
    Ignore,
    /// 默认：坐标字符串（如 22.3193_114.1694）。
    #[default]
    Coordinates,
    /// 一律当未知位置处理。
    UnknownLocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GpsPathLevel {
    Country,
    Province,
    #[default]
    City,
    District,
    FormattedAddress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// metadata 缺失时的 fallback 命名；这些名字进文件系统，不随界面语言变化。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataFallback {
    /// 默认 false：绝不默认把文件修改时间当拍摄时间。
    pub use_modified_time: bool,
    pub unknown_camera: String,
    pub unknown_location: String,
    pub unknown_date: String,
}

impl Default for MetadataFallback {
    fn default() -> Self {
        Self {
            use_modified_time: false,
            unknown_camera: "UnknownCamera".to_string(),
            unknown_location: "UnknownLocation".to_string(),
            unknown_date: "UnknownDate".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub source_directory: Option<PathBuf>,
    pub destination_directory: Option<PathBuf>,
    pub include_subfolders: bool,
    pub operation_mode: OperationMode,
    pub directory_template: String,
    pub filename_template: String,
    pub duplicate_mode: DuplicateMode,
    /// 默认关闭：GPS 坐标上传第三方属隐私敏感操作，必须显式开启。
    pub gps_enabled: bool,
    /// serde default：旧版本 settings.json 无此字段时按默认值加载。
    #[serde(default)]
    pub gps_no_api_mode: GpsNoApiMode,
    pub gps_path_level: GpsPathLevel,
    /// 经纬度缓存键的小数位精度，4 ≈ 11 米。
    pub gps_round_precision: u8,
    pub metadata_fallback: MetadataFallback,
    pub max_hash_workers: u16,
    pub max_copy_workers: u16,
    pub theme: Theme,
    pub language: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            source_directory: None,
            destination_directory: None,
            include_subfolders: true,
            operation_mode: OperationMode::default(),
            directory_template: "{yyyy}/{camera_model}".to_string(),
            // 默认保留原文件名：重命名是可选增强，不是默认行为。
            filename_template: "{original_name}.{extension}".to_string(),
            duplicate_mode: DuplicateMode::default(),
            gps_enabled: false,
            gps_no_api_mode: GpsNoApiMode::default(),
            gps_path_level: GpsPathLevel::default(),
            gps_round_precision: 4,
            metadata_fallback: MetadataFallback::default(),
            max_hash_workers: 4,
            max_copy_workers: 2,
            theme: Theme::default(),
            language: "zh-CN".to_string(),
        }
    }
}

impl AppSettings {
    pub const MAX_WORKERS: u16 = 16;
    pub const MAX_GPS_PRECISION: u8 = 6;

    pub fn validate(&self) -> Result<()> {
        if self.directory_template.trim().is_empty() {
            return Err(AppError::Config("directory template is empty".to_string()));
        }
        if self.filename_template.trim().is_empty() {
            return Err(AppError::Config("filename template is empty".to_string()));
        }
        if self.max_hash_workers == 0 || self.max_hash_workers > Self::MAX_WORKERS {
            return Err(AppError::Config(format!(
                "max_hash_workers out of range 1..={}: {}",
                Self::MAX_WORKERS,
                self.max_hash_workers
            )));
        }
        if self.max_copy_workers == 0 || self.max_copy_workers > Self::MAX_WORKERS {
            return Err(AppError::Config(format!(
                "max_copy_workers out of range 1..={}: {}",
                Self::MAX_WORKERS,
                self.max_copy_workers
            )));
        }
        if self.gps_round_precision == 0 || self.gps_round_precision > Self::MAX_GPS_PRECISION {
            return Err(AppError::Config(format!(
                "gps_round_precision out of range 1..={}: {}",
                Self::MAX_GPS_PRECISION,
                self.gps_round_precision
            )));
        }
        let fb = &self.metadata_fallback;
        for (field, value) in [
            ("unknown_camera", &fb.unknown_camera),
            ("unknown_location", &fb.unknown_location),
            ("unknown_date", &fb.unknown_date),
        ] {
            if value.trim().is_empty() {
                return Err(AppError::Config(format!(
                    "metadata fallback {field} is empty"
                )));
            }
        }
        if self.language.trim().is_empty() {
            return Err(AppError::Config("language is empty".to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        AppSettings::default().validate().expect("defaults valid");
    }

    #[test]
    fn default_is_copy_not_delete() {
        assert_eq!(AppSettings::default().operation_mode, OperationMode::Copy);
        assert!(!AppSettings::default().metadata_fallback.use_modified_time);
        assert!(!AppSettings::default().gps_enabled);
    }

    #[test]
    fn rejects_empty_template() {
        let s = AppSettings {
            directory_template: "  ".to_string(),
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn rejects_out_of_range_workers_and_precision() {
        let s = AppSettings {
            max_copy_workers: 0,
            ..Default::default()
        };
        assert!(s.validate().is_err());

        let s = AppSettings {
            max_hash_workers: 99,
            ..Default::default()
        };
        assert!(s.validate().is_err());

        let s = AppSettings {
            gps_round_precision: 7,
            ..Default::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn serde_roundtrip_keeps_camel_case_contract() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).expect("serialize");
        assert!(json.contains("\"includeSubfolders\":true"));
        assert!(json.contains("\"directoryTemplate\":\"{yyyy}/{camera_model}\""));
        let back: AppSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, s);
    }

    #[test]
    fn legacy_settings_without_gps_no_api_mode_still_load() {
        // Phase 2-5 写出的 settings.json 没有 gpsNoApiMode 字段
        let mut json = serde_json::to_string(&AppSettings::default()).expect("serialize");
        json = json.replace("\"gpsNoApiMode\":\"coordinates\",", "");
        let back: AppSettings = serde_json::from_str(&json).expect("legacy loads");
        assert_eq!(back.gps_no_api_mode, GpsNoApiMode::Coordinates);
    }
}
