//! 模板预览 IPC：用固定示例上下文渲染，供规则编辑器实时预览。

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};

use crate::core::template::{render_directory, render_filename, Template, TemplateContext};
use crate::error::ErrorDto;
use crate::models::metadata::{LocationSource, ResolvedLocation};
use crate::models::settings::MetadataFallback;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreviewRequest {
    pub directory_template: String,
    pub filename_template: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePreviewDto {
    /// 逐段清洗后的目录组件。
    pub directory_components: Vec<String>,
    pub filename: String,
    /// 仅供展示的拼接串（真实路径用 PathBuf::push 构建）。
    pub example: String,
}

/// 预览用示例上下文（对应需求文档里的 D80 / 香港样例）。
fn sample_context() -> TemplateContext {
    TemplateContext {
        taken_at: Some(NaiveDateTime::new(
            // 常量初始化：日期合法是编译期已知事实
            NaiveDate::from_ymd_opt(2017, 11, 30).expect("valid sample date"),
            NaiveTime::from_hms_opt(15, 22, 31).expect("valid sample time"),
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
        seq: Some(1),
    }
}

#[tauri::command]
pub fn template_preview(request: TemplatePreviewRequest) -> Result<TemplatePreviewDto, ErrorDto> {
    let dir_template =
        Template::validate_directory(&request.directory_template).map_err(ErrorDto::from)?;
    let file_template =
        Template::validate_filename(&request.filename_template).map_err(ErrorDto::from)?;
    let ctx = sample_context();
    let components = render_directory(&dir_template, &ctx).map_err(ErrorDto::from)?;
    let filename = render_filename(&file_template, &ctx).map_err(ErrorDto::from)?;

    let mut parts = components.clone();
    parts.push(filename.clone());
    Ok(TemplatePreviewDto {
        directory_components: components,
        filename,
        example: parts.join("/"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_matches_requirement_example() {
        let dto = template_preview(TemplatePreviewRequest {
            directory_template: "{yyyy}/{gps_city}/{camera_model}".to_string(),
            filename_template: "{yyyyMMdd}_{HHmmss}_{seq:4}.{extension}".to_string(),
        })
        .expect("preview");
        assert_eq!(
            dto.example,
            "2017/Hong Kong/NIKON D80/20171130_152231_0001.JPG"
        );
    }

    #[test]
    fn invalid_template_returns_typed_error() {
        let err = template_preview(TemplatePreviewRequest {
            directory_template: "{bogus}".to_string(),
            filename_template: "{original_name}.{extension}".to_string(),
        })
        .expect_err("must fail");
        assert_eq!(err.key, "error.template");
    }
}
