//! 统一错误类型。
//!
//! 每条错误携带两类信息：技术细节（`Display`，进日志）与前端 i18n 键
//! （`user_key`，进 UI）。业务代码禁止 `unwrap` / `expect`，一律走 `Result`。

use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("exif parse error: {0}")]
    Exif(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("template error: {0}")]
    Template(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("hash error: {0}")]
    Hash(String),
    #[error("permission denied: {0}")]
    Permission(String),
    /// 目标已存在：执行器据此触发冲突改名重试，绝不覆盖。
    #[error("target already exists: {0}")]
    TargetExists(String),
    #[error("cancelled")]
    Cancelled,
    #[error("config error: {0}")]
    Config(String),
    #[error("background task error: {0}")]
    Task(String),
}

impl AppError {
    /// 前端 i18n 键，见 `src/i18n/locales/*` 的 `error` 命名空间。
    pub fn user_key(&self) -> &'static str {
        match self {
            Self::Io(_) => "error.io",
            Self::Exif(_) => "error.exif",
            Self::Database(_) => "error.database",
            Self::Network(_) => "error.network",
            Self::Template(_) => "error.template",
            Self::InvalidPath(_) => "error.invalidPath",
            Self::Hash(_) => "error.hash",
            Self::Permission(_) => "error.permission",
            Self::TargetExists(_) => "error.invalidPath",
            Self::Cancelled => "error.cancelled",
            Self::Config(_) => "error.config",
            Self::Task(_) => "error.task",
        }
    }
}

/// 通过 IPC 返回给前端的错误形态。`message` 仅供开发调试展示，
/// 正式 UI 文案以 `key` 为准。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorDto {
    pub key: String,
    pub message: String,
}

impl From<AppError> for ErrorDto {
    fn from(err: AppError) -> Self {
        Self {
            key: err.user_key().to_string(),
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_key_is_stable_and_mapped() {
        let err = AppError::Template("bad variable".to_string());
        assert_eq!(err.user_key(), "error.template");
        assert!(err.to_string().contains("bad variable"));
    }

    #[test]
    fn cancelled_maps_to_own_key() {
        assert_eq!(AppError::Cancelled.user_key(), "error.cancelled");
    }

    #[test]
    fn dto_carries_key_and_technical_message() {
        let dto = ErrorDto::from(AppError::InvalidPath(".. escape".to_string()));
        assert_eq!(dto.key, "error.invalidPath");
        assert!(dto.message.contains(".. escape"));
        let json = serde_json::to_string(&dto).expect("dto serializes");
        assert!(json.contains("\"key\":\"error.invalidPath\""));
    }

    #[test]
    fn io_error_converts() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: AppError = io.into();
        assert_eq!(err.user_key(), "error.io");
    }
}
