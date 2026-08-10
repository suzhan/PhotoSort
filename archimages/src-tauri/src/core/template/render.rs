//! 渲染器：token → 字符串 → 按组件清洗。
//!
//! 字面值同样清洗：用户在模板里写的任何内容（包括 "../"）都不例外。

use chrono::NaiveDateTime;

use super::{Template, TemplateContext, TemplateToken, Variable};
use crate::error::{AppError, Result};
use crate::utils::path::sanitize_path_component;

fn format_date(taken_at: NaiveDateTime, fmt: &str) -> String {
    taken_at.format(fmt).to_string()
}

fn render_variable(variable: &Variable, ctx: &TemplateContext) -> Result<String> {
    let fallback = &ctx.fallback;
    let value = match variable {
        Variable::Yyyy => ctx
            .taken_at
            .map(|t| format_date(t, "%Y"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::Mm => ctx
            .taken_at
            .map(|t| format_date(t, "%m"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::Dd => ctx
            .taken_at
            .map(|t| format_date(t, "%d"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::YyyyMmDd => ctx
            .taken_at
            .map(|t| format_date(t, "%Y%m%d"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::YyyyMmDdDash => ctx
            .taken_at
            .map(|t| format_date(t, "%Y-%m-%d"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::Hh => ctx
            .taken_at
            .map(|t| format_date(t, "%H"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::Mi => ctx
            .taken_at
            .map(|t| format_date(t, "%M"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::Ss => ctx
            .taken_at
            .map(|t| format_date(t, "%S"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::HhMmSs => ctx
            .taken_at
            .map(|t| format_date(t, "%H%M%S"))
            .unwrap_or_else(|| fallback.unknown_date.clone()),
        Variable::CameraMake => ctx
            .camera_make
            .clone()
            .unwrap_or_else(|| fallback.unknown_camera.clone()),
        Variable::CameraModel => ctx
            .camera_model
            .clone()
            .unwrap_or_else(|| fallback.unknown_camera.clone()),
        Variable::LensMake => ctx
            .lens_make
            .clone()
            .unwrap_or_else(|| fallback.unknown_camera.clone()),
        Variable::LensModel => ctx
            .lens_model
            .clone()
            .unwrap_or_else(|| fallback.unknown_camera.clone()),
        Variable::GpsCountry => ctx
            .location
            .as_ref()
            .and_then(|l| l.country.clone())
            .unwrap_or_else(|| fallback.unknown_location.clone()),
        Variable::GpsProvince => ctx
            .location
            .as_ref()
            .and_then(|l| l.province.clone())
            .unwrap_or_else(|| fallback.unknown_location.clone()),
        Variable::GpsCity => ctx
            .location
            .as_ref()
            .and_then(|l| l.city.clone())
            .unwrap_or_else(|| fallback.unknown_location.clone()),
        Variable::GpsDistrict => ctx
            .location
            .as_ref()
            .and_then(|l| l.district.clone())
            .unwrap_or_else(|| fallback.unknown_location.clone()),
        Variable::OriginalName => ctx.original_name.clone(),
        Variable::Extension => ctx.extension.clone(),
        Variable::Seq { width } => {
            let seq = ctx
                .seq
                .ok_or_else(|| AppError::Template("seq used but not assigned".to_string()))?;
            match width {
                Some(w) => format!("{seq:0w$}", w = w),
                None => seq.to_string(),
            }
        }
    };
    Ok(value)
}

/// 原始渲染（未清洗）。仅内部使用；对外走 directory/filename 入口。
fn render_raw(template: &Template, ctx: &TemplateContext) -> Result<String> {
    let mut out = String::new();
    for token in template.tokens() {
        match token {
            TemplateToken::Literal(s) => out.push_str(s),
            TemplateToken::Variable(v) => out.push_str(&render_variable(v, ctx)?),
        }
    }
    Ok(out)
}

/// 渲染目录模板：按 `/` 分段、丢弃空段、逐段清洗。
/// 返回的 Vec 交给调用方用 PathBuf::push 逐段构建真实路径。
pub fn render_directory(template: &Template, ctx: &TemplateContext) -> Result<Vec<String>> {
    let raw = render_raw(template, ctx)?;
    let components: Vec<String> = raw
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(sanitize_path_component)
        .collect();
    Ok(components)
}

/// 渲染文件名模板：整体作为一个路径组件清洗。
pub fn render_filename(template: &Template, ctx: &TemplateContext) -> Result<String> {
    let raw = render_raw(template, ctx)?;
    let cleaned = sanitize_path_component(raw.trim());
    Ok(cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::metadata::{LocationSource, ResolvedLocation};
    use crate::models::settings::MetadataFallback;
    use chrono::{NaiveDate, NaiveTime};

    fn ctx() -> TemplateContext {
        TemplateContext {
            taken_at: Some(NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2017, 11, 30).expect("date"),
                NaiveTime::from_hms_opt(15, 22, 31).expect("time"),
            )),
            camera_make: Some("NIKON CORPORATION".to_string()),
            camera_model: Some("NIKON D80".to_string()),
            lens_make: Some("NIKON".to_string()),
            lens_model: Some("18-135mm F3.5-5.6".to_string()),
            location: Some(ResolvedLocation {
                country: Some("China".to_string()),
                province: None,
                city: Some("Hong Kong".to_string()),
                district: None,
                formatted_address: None,
                source: LocationSource::Google,
            }),
            original_name: "DSC_1231".to_string(),
            extension: "JPG".to_string(),
            fallback: MetadataFallback::default(),
            seq: None,
        }
    }

    #[test]
    fn renders_documented_directory_examples() {
        let c = ctx();
        let t = Template::validate_directory("{yyyy}/{camera_model}").expect("t");
        assert_eq!(
            render_directory(&t, &c).expect("r"),
            vec!["2017", "NIKON D80"]
        );

        let t = Template::validate_directory("{yyyy}/{gps_city}/{camera_model}").expect("t");
        assert_eq!(
            render_directory(&t, &c).expect("r"),
            vec!["2017", "Hong Kong", "NIKON D80"]
        );

        let t = Template::validate_directory("{yyyy}/{yyyyMMdd}/{camera_model}/{lens_model}")
            .expect("t");
        assert_eq!(
            render_directory(&t, &c).expect("r"),
            vec!["2017", "20171130", "NIKON D80", "18-135mm F3.5-5.6"]
        );
    }

    #[test]
    fn renders_documented_filename_examples() {
        let mut c = ctx();
        let t = Template::validate_filename("{original_name}.{extension}").expect("t");
        assert_eq!(render_filename(&t, &c).expect("r"), "DSC_1231.JPG");

        let t = Template::validate_filename("{yyyyMMdd}_{HHmmss}.{extension}").expect("t");
        assert_eq!(render_filename(&t, &c).expect("r"), "20171130_152231.JPG");

        c.seq = Some(1);
        let t = Template::validate_filename("{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}").expect("t");
        assert_eq!(
            render_filename(&t, &c).expect("r"),
            "20171130_152231_0001.JPG"
        );

        let t = Template::validate_filename("{original_name}_{yyyyMMdd}.{extension}").expect("t");
        assert_eq!(render_filename(&t, &c).expect("r"), "DSC_1231_20171130.JPG");
    }

    #[test]
    fn seq_widths() {
        let mut c = ctx();
        for (tpl, seq, expect) in [
            ("{seq}", 1, "1"),
            ("{seq:2}", 1, "01"),
            ("{seq:3}", 7, "007"),
            ("{seq:4}", 1, "0001"),
            ("{seq:5}", 42, "00042"),
            // 超过宽度的序号按自然宽度输出，不截断
            ("{seq:2}", 1234, "1234"),
        ] {
            c.seq = Some(seq);
            let t = Template::validate_filename(tpl).expect("parse");
            assert_eq!(render_filename(&t, &c).expect("render"), expect, "{tpl}");
        }
    }

    #[test]
    fn seq_without_assignment_is_error() {
        let c = ctx();
        let t = Template::validate_filename("{seq:4}.{extension}").expect("t");
        assert!(render_filename(&t, &c).is_err());
    }

    #[test]
    fn fallbacks_when_metadata_missing() {
        let mut c = ctx();
        c.taken_at = None;
        c.camera_model = None;
        c.location = None;
        let t = Template::validate_directory("{yyyy}/{camera_model}/{gps_city}").expect("t");
        assert_eq!(
            render_directory(&t, &c).expect("r"),
            vec!["UnknownDate", "UnknownCamera", "UnknownLocation"]
        );
    }

    #[test]
    fn values_are_sanitized_per_component() {
        let mut c = ctx();
        c.camera_model = Some("CON".to_string());
        c.location = Some(ResolvedLocation {
            city: Some("Kowloon: TST?".to_string()),
            ..Default::default()
        });
        let t = Template::validate_directory("{camera_model}/{gps_city}").expect("t");
        assert_eq!(
            render_directory(&t, &c).expect("r"),
            vec!["CON_", "Kowloon_ TST_"]
        );
    }

    #[test]
    fn escape_attempt_in_template_is_neutralized() {
        let c = ctx();
        // 字面值里的 ../ 同样被清洗，变成无害的 Unknown
        let t = Template::validate_directory("../../{yyyy}").expect("t");
        let components = render_directory(&t, &c).expect("r");
        assert_eq!(components, vec!["Unknown", "Unknown", "2017"]);
        assert!(!components.iter().any(|s| s == ".." || s.contains('/')));
    }

    #[test]
    fn unicode_components_pass_through() {
        let mut c = ctx();
        c.location = Some(ResolvedLocation {
            city: Some("香港".to_string()),
            ..Default::default()
        });
        let t = Template::validate_directory("{yyyy}/{gps_city}").expect("t");
        assert_eq!(render_directory(&t, &c).expect("r"), vec!["2017", "香港"]);
    }

    #[test]
    fn collapsed_slashes_and_spaces_are_dropped() {
        let c = ctx();
        let t = Template::validate_directory("photos//  /{yyyy}").expect("t");
        assert_eq!(render_directory(&t, &c).expect("r"), vec!["photos", "2017"]);
    }
}
