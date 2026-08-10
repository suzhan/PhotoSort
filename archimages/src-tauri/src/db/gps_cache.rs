//! GPS 缓存 DAO（需求 §七）：归一化坐标作主键，跨任务复用。
//! Google API 调用昂贵且有配额，缓存命中是性能与成本的双重保障。

use rusqlite::params;

use super::{now_unix, Database};
use crate::error::{AppError, Result};
use crate::models::metadata::ResolvedLocation;

#[derive(Clone)]
pub struct GpsCache {
    db: Database,
}

/// 缓存行：与 ResolvedLocation 字段一一对应 + raw_provider（始终 "google"）。
#[derive(Debug, Clone)]
pub struct CachedLocation {
    pub country: Option<String>,
    pub province: Option<String>,
    pub city: Option<String>,
    pub district: Option<String>,
    pub formatted_address: Option<String>,
}

impl From<&ResolvedLocation> for CachedLocation {
    fn from(l: &ResolvedLocation) -> Self {
        Self {
            country: l.country.clone(),
            province: l.province.clone(),
            city: l.city.clone(),
            district: l.district.clone(),
            formatted_address: l.formatted_address.clone(),
        }
    }
}

impl CachedLocation {
    pub fn to_resolved(&self) -> ResolvedLocation {
        ResolvedLocation {
            country: self.country.clone(),
            province: self.province.clone(),
            city: self.city.clone(),
            district: self.district.clone(),
            formatted_address: self.formatted_address.clone(),
            source: crate::models::metadata::LocationSource::Google,
        }
    }
}

impl GpsCache {
    pub(crate) fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn lookup(&self, lat_key: &str, lng_key: &str) -> Result<Option<CachedLocation>> {
        let conn = self.db.lock();
        let row = conn
            .query_row(
                "SELECT country, province, city, district, formatted_address
                 FROM gps_cache WHERE latitude_key = ?1 AND longitude_key = ?2",
                params![lat_key, lng_key],
                |r| {
                    Ok(CachedLocation {
                        country: r.get(0)?,
                        province: r.get(1)?,
                        city: r.get(2)?,
                        district: r.get(3)?,
                        formatted_address: r.get(4)?,
                    })
                },
            )
            .ok();
        Ok(row)
    }

    pub fn store(&self, lat_key: &str, lng_key: &str, loc: &CachedLocation) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO gps_cache (latitude_key, longitude_key, country, province, city, district, formatted_address, raw_provider, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(latitude_key, longitude_key) DO UPDATE SET
               country = excluded.country, province = excluded.province,
               city = excluded.city, district = excluded.district,
               formatted_address = excluded.formatted_address,
               raw_provider = excluded.raw_provider,
               updated_at = excluded.updated_at",
            params![
                lat_key,
                lng_key,
                loc.country,
                loc.province,
                loc.city,
                loc.district,
                loc.formatted_address,
                "google",
                now_unix(),
            ],
        )
        .map_err(|e| AppError::Database(format!("store gps cache: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let db = Database::open_in_memory().expect("db");
        let cache = db.gps_cache();
        assert!(cache.lookup("22.3193", "114.1694").unwrap().is_none());

        let loc = CachedLocation {
            country: Some("中国".to_string()),
            province: Some("香港".to_string()),
            city: Some("香港".to_string()),
            district: None,
            formatted_address: Some("Hong Kong".to_string()),
        };
        cache.store("22.3193", "114.1694", &loc).unwrap();

        let hit = cache.lookup("22.3193", "114.1694").unwrap().unwrap();
        assert_eq!(hit.country.as_deref(), Some("中国"));
        assert_eq!(hit.city.as_deref(), Some("香港"));
        assert!(hit.district.is_none());
    }

    #[test]
    fn overwrite_on_conflict() {
        let db = Database::open_in_memory().expect("db");
        let cache = db.gps_cache();
        let mut loc = CachedLocation {
            country: Some("A".to_string()),
            province: None,
            city: None,
            district: None,
            formatted_address: None,
        };
        cache.store("1", "2", &loc).unwrap();
        loc.country = Some("B".to_string());
        cache.store("1", "2", &loc).unwrap();
        let hit = cache.lookup("1", "2").unwrap().unwrap();
        assert_eq!(hit.country.as_deref(), Some("B"));
    }
}
