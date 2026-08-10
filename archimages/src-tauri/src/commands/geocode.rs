//! GPS 反查 IPC：API Key 管理 + 手动反查测试。
//!
//! 真实反查在 organize 流水线内由 Planner 触发（命中缓存优先）；
//! 这里只暴露 Key 管理与连通性自检，供设置对话框使用。

use crate::core::api_key::ApiKeyStore;
use crate::core::geocode::Geocoder;
use crate::error::{AppError, ErrorDto};
use crate::models::metadata::GpsCoordinate;

#[tauri::command]
pub fn set_google_api_key(key: String) -> Result<(), ErrorDto> {
    if key.trim().is_empty() {
        return Err(ErrorDto::from(AppError::Config(
            "api key must not be empty".to_string(),
        )));
    }
    ApiKeyStore::set(key.trim()).map_err(ErrorDto::from)
}

#[tauri::command]
pub fn clear_google_api_key() -> Result<(), ErrorDto> {
    ApiKeyStore::clear().map_err(ErrorDto::from)
}

#[tauri::command]
pub fn has_google_api_key() -> bool {
    ApiKeyStore::get().map(|k| k.is_some()).unwrap_or(false)
}

/// 连通性自检：用给定坐标反查，返回格式化地址。失败返回错误（前端展示）。
#[tauri::command]
pub fn test_geocode(latitude: f64, longitude: f64) -> Result<String, ErrorDto> {
    let key = ApiKeyStore::get()
        .map_err(ErrorDto::from)?
        .ok_or_else(|| AppError::Config("no api key configured".to_string()))
        .map_err(ErrorDto::from)?;
    let geocoder = Geocoder::new(key).map_err(ErrorDto::from)?;
    let loc = geocoder
        .reverse(GpsCoordinate {
            latitude,
            longitude,
        })
        .map_err(ErrorDto::from)?;
    Ok(loc.formatted_address.unwrap_or_else(|| {
        loc.country
            .or(loc.province)
            .or(loc.city)
            .unwrap_or_else(|| "(no address)".to_string())
    }))
}
