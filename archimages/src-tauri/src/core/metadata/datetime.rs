//! EXIF 日期时间解析。
//!
//! EXIF 标准格式是 "2017:11:30 15:22:31"（朴素本地时间，无时区）。
//! 也容忍 ISO 分隔符、亚秒与全零无效值。解析结果一律 NaiveDateTime。

use chrono::NaiveDateTime;

const FORMATS: &[&str] = &[
    "%Y:%m:%d %H:%M:%S%.f",
    "%Y:%m:%d %H:%M:%S",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S",
];

/// 解析 EXIF 日期字符串；无效值（如全零、非法月份）返回 None。
pub fn parse_exif_datetime(raw: &str) -> Option<NaiveDateTime> {
    let s = raw.trim().trim_end_matches('\0');
    if s.is_empty() || s.chars().all(|c| c == '0' || c == ':' || c == ' ') {
        return None;
    }
    FORMATS
        .iter()
        .find_map(|fmt| NaiveDateTime::parse_from_str(s, fmt).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveTime, Timelike};

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDateTime::new(
            NaiveDate::from_ymd_opt(y, mo, d).expect("date"),
            NaiveTime::from_hms_opt(h, mi, s).expect("time"),
        )
    }

    #[test]
    fn parses_standard_exif_format() {
        assert_eq!(
            parse_exif_datetime("2017:11:30 15:22:31"),
            Some(dt(2017, 11, 30, 15, 22, 31))
        );
    }

    #[test]
    fn parses_subseconds_and_iso_separator() {
        // 亚秒被保留（模板格式化时只用到整秒）
        let with_frac = parse_exif_datetime("2017:11:30 15:22:31.12").expect("parse");
        let expected = dt(2017, 11, 30, 15, 22, 31)
            .with_nanosecond(120_000_000)
            .expect("nanos");
        assert_eq!(with_frac, expected);
        assert_eq!(
            parse_exif_datetime("2017-11-30 15:22:31"),
            Some(dt(2017, 11, 30, 15, 22, 31))
        );
        assert_eq!(
            parse_exif_datetime("2017-11-30T15:22:31"),
            Some(dt(2017, 11, 30, 15, 22, 31))
        );
    }

    #[test]
    fn rejects_zero_and_garbage() {
        assert_eq!(parse_exif_datetime("0000:00:00 00:00:00"), None);
        assert_eq!(parse_exif_datetime("2017:13:40 99:99:99"), None);
        assert_eq!(parse_exif_datetime("not a date"), None);
        assert_eq!(parse_exif_datetime(""), None);
    }

    #[test]
    fn tolerates_trailing_nul_and_space() {
        assert_eq!(
            parse_exif_datetime("2017:11:30 15:22:31\u{0} "),
            Some(dt(2017, 11, 30, 15, 22, 31))
        );
    }
}
