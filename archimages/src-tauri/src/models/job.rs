//! 事务日志模型：jobs / job_files 对应 SQLite 表（DAO 在 Phase 10）。
//! 崩溃恢复只依赖这些状态，绝不自动删除任何文件。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    Scan,
    Organize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Running,
    Completed,
    CompletedWithErrors,
    Cancelled,
    Failed,
    /// 应用异常退出后，启动时将 Running 标记为此状态。
    Interrupted,
    /// 用户在恢复提示中选择放弃（只改标记，绝不动文件）。
    Abandoned,
}

impl JobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Organize => "organize",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "scan" => Some(Self::Scan),
            "organize" => Some(Self::Organize),
            _ => None,
        }
    }
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::CompletedWithErrors => "completedWithErrors",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "completedWithErrors" => Some(Self::CompletedWithErrors),
            "cancelled" => Some(Self::Cancelled),
            "failed" => Some(Self::Failed),
            "interrupted" => Some(Self::Interrupted),
            "abandoned" => Some(Self::Abandoned),
            _ => None,
        }
    }
}

impl JobFileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::MetadataRead => "metadataRead",
            Self::Planned => "planned",
            Self::Copying => "copying",
            Self::Copied => "copied",
            Self::Verified => "verified",
            Self::SourceDeleted => "sourceDeleted",
            Self::Duplicate => "duplicate",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobFileStatus {
    Pending,
    MetadataRead,
    Planned,
    Copying,
    Copied,
    Verified,
    SourceDeleted,
    Duplicate,
    Skipped,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub status: JobStatus,
    pub source_root: Option<PathBuf>,
    pub destination_root: Option<PathBuf>,
    pub settings_json: Option<String>,
    /// Unix 秒。
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct JobFile {
    pub id: i64,
    pub job_id: String,
    pub source_path: PathBuf,
    pub target_path: Option<PathBuf>,
    pub status: JobFileStatus,
    pub source_sha256: Option<String>,
    pub target_sha256: Option<String>,
    pub error: Option<String>,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_roundtrip_via_serde() {
        let s = serde_json::to_string(&JobFileStatus::SourceDeleted).expect("serialize");
        assert_eq!(s, "\"sourceDeleted\"");
        let back: JobFileStatus = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back, JobFileStatus::SourceDeleted);
    }
}
