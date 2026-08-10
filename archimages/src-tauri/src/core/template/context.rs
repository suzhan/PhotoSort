//! 模板渲染上下文：把领域对象翻译成模板可消费的扁平变量集。

use chrono::NaiveDateTime;

use crate::models::metadata::{PhotoMetadata, ResolvedLocation};
use crate::models::photo::PhotoFile;
use crate::models::settings::MetadataFallback;

#[derive(Debug, Clone)]
pub struct TemplateContext {
    pub taken_at: Option<NaiveDateTime>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    /// GPS 反解结果（或坐标退化、或 None）。由 Planner 按设置构造。
    pub location: Option<ResolvedLocation>,
    /// 原始文件名主体（不含扩展名）。
    pub original_name: String,
    /// 原始扩展名（保留原始大小写，不含点）。
    pub extension: String,
    pub fallback: MetadataFallback,
    /// 执行期由 SequenceCoordinator 分配；预览期可给示例值。
    pub seq: Option<u64>,
}

impl TemplateContext {
    /// 从扫描 + 元数据构造。GPS 位置由调用方按设置解析后塞入。
    pub fn from_parts(
        photo: &PhotoFile,
        metadata: Option<&PhotoMetadata>,
        location: Option<ResolvedLocation>,
        fallback: MetadataFallback,
        seq: Option<u64>,
    ) -> Self {
        let original_name = photo
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = photo
            .path
            .extension()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| photo.extension.clone());
        let empty = PhotoMetadata::default();
        let md = metadata.unwrap_or(&empty);
        Self {
            taken_at: md.taken_at,
            camera_make: md.camera_make.clone(),
            camera_model: md.camera_model.clone(),
            lens_make: md.lens_make.clone(),
            lens_model: md.lens_model.clone(),
            location,
            original_name,
            extension,
            fallback,
            seq,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::SystemTime;

    #[test]
    fn from_parts_extracts_name_and_extension_preserving_case() {
        let photo = PhotoFile {
            path: PathBuf::from("/src/DSC_1231.JPG"),
            size: 1,
            extension: "jpg".to_string(),
            modified_time: SystemTime::UNIX_EPOCH,
        };
        let ctx =
            TemplateContext::from_parts(&photo, None, None, MetadataFallback::default(), None);
        assert_eq!(ctx.original_name, "DSC_1231");
        assert_eq!(ctx.extension, "JPG"); // 保留原始大小写
        assert!(ctx.camera_model.is_none());
    }
}
