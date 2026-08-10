//! 规划模型：Planner 纯函数的输出，Preview 与正式执行共用同一份 Plan。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::duplicate::DuplicateResult;
use super::metadata::{PhotoMetadata, ResolvedLocation};
use super::photo::PhotoFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanStatus {
    Ready,
    Duplicate,
    MissingExif,
    MissingDate,
    MissingGps,
    Collision,
    Unsupported,
    Error,
}

/// 非致命问题，供预览表警示列展示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanWarning {
    MissingCameraModel,
    MissingLensModel,
    MissingGps,
    /// 用户开启 fallback 后使用了文件修改时间，必须明示。
    UsedModifiedTimeFallback,
    /// 模板值含非法字符被清洗。
    SanitizedComponent,
    /// 组件超长被截断。
    NameTruncated,
}

#[derive(Debug, Clone)]
pub struct PhotoPlan {
    pub source: PhotoFile,
    pub metadata: Option<PhotoMetadata>,
    pub location: Option<ResolvedLocation>,
    pub target_path: PathBuf,
    pub status: PlanStatus,
    pub duplicate: DuplicateResult,
    pub warnings: Vec<PlanWarning>,
}

impl PlanStatus {
    /// 与前端 ScanRow.status 对应的 camelCase 字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanStatus::Ready => "ready",
            PlanStatus::Duplicate => "duplicate",
            PlanStatus::MissingExif => "missingExif",
            PlanStatus::MissingDate => "missingDate",
            PlanStatus::MissingGps => "missingGps",
            PlanStatus::Collision => "collision",
            PlanStatus::Unsupported => "unsupported",
            PlanStatus::Error => "error",
        }
    }
}

impl PhotoPlan {
    /// 是否可进入执行阶段：元数据缺失不阻塞（fallback 名称兜底且状态已明示），
    /// Collision / Error / Unsupported 才阻塞。
    pub fn executable(&self) -> bool {
        matches!(
            self.status,
            PlanStatus::Ready
                | PlanStatus::Duplicate
                | PlanStatus::MissingExif
                | PlanStatus::MissingDate
                | PlanStatus::MissingGps
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn plan_with(status: PlanStatus) -> PhotoPlan {
        PhotoPlan {
            source: PhotoFile {
                path: PathBuf::from("/src/a.jpg"),
                size: 1,
                extension: "jpg".to_string(),
                modified_time: SystemTime::UNIX_EPOCH,
            },
            metadata: None,
            location: None,
            target_path: PathBuf::from("/dst/a.jpg"),
            status,
            duplicate: DuplicateResult::NotDuplicate,
            warnings: vec![],
        }
    }

    #[test]
    fn executable_matrix() {
        assert!(plan_with(PlanStatus::Ready).executable());
        assert!(plan_with(PlanStatus::Duplicate).executable());
        assert!(plan_with(PlanStatus::MissingExif).executable());
        assert!(plan_with(PlanStatus::MissingDate).executable());
        assert!(plan_with(PlanStatus::MissingGps).executable());
        assert!(!plan_with(PlanStatus::Collision).executable());
        assert!(!plan_with(PlanStatus::Unsupported).executable());
        assert!(!plan_with(PlanStatus::Error).executable());
    }

    #[test]
    fn status_strings_are_camel_case() {
        assert_eq!(PlanStatus::MissingDate.as_str(), "missingDate");
        assert_eq!(PlanStatus::Ready.as_str(), "ready");
    }
}
