//! GPS 反向地理编码（需求 §七）。
//!
//! 设计要点：
//! - Google Maps Reverse Geocoding API 是**可选**功能；无 Key 仍可整理照片；
//! - API Key 绝不进 settings.json，存 OS 凭据存储（keyring）；
//! - 网络错误**绝不终止整批任务**：单点失败降级为坐标/未知地点；
//! - 坐标按 `gps_round_precision` 归一化后作缓存键，避免浮点抖动导致缓存击穿；
//! - 调用走 reqwest，配置 timeout；并发由调用方 Semaphore 控制（§二十）。

use std::time::Duration;

use serde::Deserialize;

use crate::error::{AppError, Result};
use crate::models::metadata::{GpsCoordinate, LocationSource, ResolvedLocation};

const ENDPOINT: &str = "https://maps.googleapis.com/maps/api/geocode/json";
const TIMEOUT: Duration = Duration::from_secs(8);

/// 归一化坐标到指定小数位，作缓存键。负零统一为正零避免键分裂。
pub fn normalize_coord(value: f64, precision: u8) -> (String, f64) {
    let p = precision as usize;
    let rounded = format!("{:.p$}", value, p = p)
        .parse::<f64>()
        .unwrap_or(0.0);
    // -0.0 + 0.0 == 0.0（消除负零符号位）
    let rounded = rounded + 0.0;
    let key = format!("{:.p$}", rounded, p = p);
    (key, rounded)
}

/// Google API 响应中我们关心的字段（其余忽略，避免结构耦合）。
#[derive(Debug, Deserialize)]
struct GeocodeResponse {
    results: Vec<GeocodeResult>,
    #[serde(default)]
    error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeocodeResult {
    #[serde(default)]
    address_components: Vec<AddressComponent>,
    formatted_address: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AddressComponent {
    long_name: Option<String>,
    #[serde(default)]
    types: Vec<String>,
}

/// 从 address_components 按类型取第一个 long_name。
fn pick(components: &[AddressComponent], wanted: &str) -> Option<String> {
    components
        .iter()
        .find(|c| c.types.iter().any(|t| t == wanted))
        .and_then(|c| c.long_name.clone())
        .filter(|s| !s.trim().is_empty())
}

/// 反查客户端。`Client` 内部已连接池化，复用即可。
pub struct Geocoder {
    client: reqwest::blocking::Client,
    api_key: String,
}

impl Geocoder {
    pub fn new(api_key: String) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(TIMEOUT)
            .build()
            .map_err(|e| AppError::Network(format!("build http client: {e}")))?;
        Ok(Self { client, api_key })
    }

    /// 调用 Google API 反查。失败时返回 Err，由调用方降级。
    pub fn reverse(&self, gps: GpsCoordinate) -> Result<ResolvedLocation> {
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[
                ("latlng", format!("{},{}", gps.latitude, gps.longitude)),
                ("key", self.api_key.clone()),
            ])
            .send()
            .map_err(|e| AppError::Network(format!("geocode request: {e}")))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| AppError::Network(format!("read geocode body: {e}")))?;
        if !status.is_success() {
            return Err(AppError::Network(format!("geocode HTTP {status}")));
        }
        let parsed: GeocodeResponse = serde_json::from_str(&body)
            .map_err(|e| AppError::Network(format!("parse geocode response: {e}")))?;
        if let Some(msg) = parsed.error_message {
            return Err(AppError::Network(format!("geocode API error: {msg}")));
        }
        let first = parsed
            .results
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Network("geocode returned no results".to_string()))?;
        let comps = first.address_components;
        Ok(ResolvedLocation {
            country: pick(&comps, "country"),
            province: pick(&comps, "administrative_area_level_1"),
            city: pick(&comps, "administrative_area_level_2").or_else(|| pick(&comps, "locality")),
            district: pick(&comps, "sublocality")
                .or_else(|| pick(&comps, "administrative_area_level_3")),
            formatted_address: first.formatted_address.filter(|s| !s.trim().is_empty()),
            source: LocationSource::Google,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_rounds_and_keys() {
        let (key, v) = normalize_coord(22.319345, 4);
        assert_eq!(key, "22.3193");
        assert!((v - 22.3193).abs() < 1e-9);
    }

    #[test]
    fn normalize_negative_zero_collapses() {
        let (key, v) = normalize_coord(-0.00001, 4);
        assert_eq!(key, "0.0000");
        assert_eq!(v, 0.0);
    }

    #[test]
    fn pick_finds_country() {
        let comps = vec![AddressComponent {
            long_name: Some("中国".to_string()),
            types: vec!["country".to_string()],
        }];
        assert_eq!(pick(&comps, "country").as_deref(), Some("中国"));
        assert!(pick(&comps, "locality").is_none());
    }

    #[test]
    fn pick_skips_blank_long_name() {
        let comps = vec![AddressComponent {
            long_name: Some("  ".to_string()),
            types: vec!["country".to_string()],
        }];
        assert!(pick(&comps, "country").is_none());
    }

    #[test]
    fn city_falls_back_to_locality() {
        let comps = vec![AddressComponent {
            long_name: Some("Hong Kong".to_string()),
            types: vec!["locality".to_string()],
        }];
        assert_eq!(pick(&comps, "administrative_area_level_2").as_deref(), None);
        // 调用方在 reverse() 里做 .or_else(locality) 兜底
        assert_eq!(pick(&comps, "locality").as_deref(), Some("Hong Kong"));
    }
}
