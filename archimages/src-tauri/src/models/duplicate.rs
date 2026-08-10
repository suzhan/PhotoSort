//! 查重模型。判定结果必须携带证据路径，禁止只返回 true/false。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Modern = 文件大小 + SHA-256（默认）；LegacyStrict = 大小 + MD5 + SHA1（对齐 v2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DuplicateMode {
    #[default]
    Modern,
    LegacyStrict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DuplicateResult {
    NotDuplicate,
    /// 目标位置已存在内容完全一致的文件。
    ExactDuplicate {
        existing_path: PathBuf,
    },
    /// 同名但内容不同：必须改名，禁止覆盖。
    Collision {
        existing_path: PathBuf,
    },
}

impl DuplicateResult {
    pub fn is_duplicate(&self) -> bool {
        matches!(self, Self::ExactDuplicate { .. })
    }
}

/// 流式计算出的哈希集合。Legacy 模式下 MD5/SHA1 必须单次读取同时算出。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileHash {
    pub size: u64,
    pub sha256: Option<[u8; 32]>,
    pub md5: Option<[u8; 16]>,
    pub sha1: Option<[u8; 20]>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_result_semantics() {
        assert!(!DuplicateResult::NotDuplicate.is_duplicate());
        assert!(DuplicateResult::ExactDuplicate {
            existing_path: PathBuf::from("/p/a.jpg")
        }
        .is_duplicate());
        assert!(!DuplicateResult::Collision {
            existing_path: PathBuf::from("/p/a.jpg")
        }
        .is_duplicate());
    }

    #[test]
    fn duplicate_mode_default_is_modern() {
        assert_eq!(DuplicateMode::default(), DuplicateMode::Modern);
        let json = serde_json::to_string(&DuplicateMode::LegacyStrict).expect("serialize");
        assert_eq!(json, "\"legacyStrict\"");
    }
}
