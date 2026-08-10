//! 跨平台路径与文件名安全。
//!
//! 两条红线：
//! 1. 任何进入文件系统的路径组件（模板渲染值、GPS 地址、相机型号）
//!    必须先过 `sanitize_path_component`；
//! 2. 最终目标路径必须过 `ensure_within`，证明仍在 destination root 之内。

use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, Result};

/// 清洗后的兜底名。
pub const FALLBACK_COMPONENT: &str = "Unknown";
/// 单组件长度上限（字符数）。APFS/NTFS 单组件 255 字节，留足余量给序号后缀。
pub const MAX_COMPONENT_CHARS: usize = 100;

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 清洗单个路径组件（不允许含分隔符；分隔符会被替换）。
///
/// 规则按最严格平台（Windows）取交集：非法字符与控制字符替换为 `_`，
/// 保留名（不区分大小写、含带扩展名形式）追加 `_`，
/// 末尾 `.`/空格裁剪，空串 / `.` / `..` 兜底为 `Unknown`。
pub fn sanitize_path_component(input: &str) -> String {
    let cleaned: String = input
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    // Windows 不允许以 . 或空格结尾；开头同理不利于兼容性，一并裁剪。
    let trimmed = cleaned
        .trim_matches(|c: char| c == '.' || c == ' ')
        .to_string();

    let candidate = if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        FALLBACK_COMPONENT.to_string()
    } else {
        trimmed
    };

    // 保留名按主名判断："CON" 与 "CON.jpg" 在 Windows 同属保留。
    let stem = candidate.split('.').next().unwrap_or_default();
    let candidate = if WINDOWS_RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
        format!("{candidate}_")
    } else {
        candidate
    };

    truncate_component(&candidate)
}

fn truncate_component(input: &str) -> String {
    if input.chars().count() <= MAX_COMPONENT_CHARS {
        return input.to_string();
    }
    let truncated: String = input.chars().take(MAX_COMPONENT_CHARS).collect();
    // 截断后可能又以 . / 空格结尾，再裁一次；全被裁掉则兜底。
    let trimmed = truncated.trim_matches(|c: char| c == '.' || c == ' ');
    if trimmed.is_empty() {
        FALLBACK_COMPONENT.to_string()
    } else {
        trimmed.to_string()
    }
}

/// 纯词法规范化：处理 `.` / `..`，不访问文件系统、不解析符号链接。
pub fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // 越出根部的 .. 直接丢弃：最终判定由 ensure_within 兜底。
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 校验 target 规范化后仍位于 root 之内，返回规范化后的路径。
pub fn ensure_within(root: &Path, target: &Path) -> Result<PathBuf> {
    let root_n = normalize_lexically(root);
    let target_n = normalize_lexically(target);
    if target_n.starts_with(&root_n) {
        Ok(target_n)
    } else {
        Err(AppError::InvalidPath(format!(
            "target escapes destination root: {}",
            target_n.to_string_lossy()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_windows_forbidden_chars() {
        assert_eq!(
            sanitize_path_component("a<b>c:d\"e/f\\g|h?i*j"),
            "a_b_c_d_e_f_g_h_i_j"
        );
    }

    #[test]
    fn replaces_control_chars() {
        assert_eq!(sanitize_path_component("a\u{0}\u{1f}b"), "a__b");
    }

    #[test]
    fn handles_windows_reserved_names_case_insensitive() {
        assert_eq!(sanitize_path_component("CON"), "CON_");
        assert_eq!(sanitize_path_component("con"), "con_");
        assert_eq!(sanitize_path_component("NUL"), "NUL_");
        assert_eq!(sanitize_path_component("com1"), "com1_");
        assert_eq!(sanitize_path_component("LPT9"), "LPT9_");
        // 带扩展名的保留名同样处理
        assert_eq!(sanitize_path_component("CON.jpg"), "CON.jpg_");
        // 普通名不受影响
        assert_eq!(sanitize_path_component("CONNECT"), "CONNECT");
    }

    #[test]
    fn trims_trailing_dot_and_space() {
        assert_eq!(sanitize_path_component("abc."), "abc");
        assert_eq!(sanitize_path_component("abc "), "abc");
        assert_eq!(sanitize_path_component("abc. "), "abc");
        assert_eq!(sanitize_path_component(" abc"), "abc");
    }

    #[test]
    fn empty_and_dots_fall_back() {
        assert_eq!(sanitize_path_component(""), FALLBACK_COMPONENT);
        assert_eq!(sanitize_path_component("   "), FALLBACK_COMPONENT);
        assert_eq!(sanitize_path_component("."), FALLBACK_COMPONENT);
        assert_eq!(sanitize_path_component(".."), FALLBACK_COMPONENT);
        assert_eq!(sanitize_path_component("..."), FALLBACK_COMPONENT);
    }

    #[test]
    fn unicode_passes_through() {
        assert_eq!(sanitize_path_component("香港"), "香港");
        assert_eq!(sanitize_path_component("東京"), "東京");
        assert_eq!(sanitize_path_component("München"), "München");
        assert_eq!(sanitize_path_component("é"), "é");
        assert_eq!(sanitize_path_component("中文文件名"), "中文文件名");
    }

    #[test]
    fn typical_camera_and_gps_values() {
        assert_eq!(sanitize_path_component("NIKON D80"), "NIKON D80");
        assert_eq!(
            sanitize_path_component("Hong Kong, Kowloon: TST"),
            "Hong Kong, Kowloon_ TST"
        );
    }

    #[test]
    fn truncates_overlong_component() {
        let long = "a".repeat(500);
        let out = sanitize_path_component(&long);
        assert_eq!(out.chars().count(), MAX_COMPONENT_CHARS);

        // 截断后结尾若是 . / 空格也要裁掉
        let mut almost = "a".repeat(MAX_COMPONENT_CHARS);
        almost.push_str("   ...");
        let out = sanitize_path_component(&almost);
        assert_eq!(out, "a".repeat(MAX_COMPONENT_CHARS));
    }

    #[test]
    fn normalize_lexically_resolves_dots() {
        assert_eq!(
            normalize_lexically(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(
            normalize_lexically(Path::new("/a/../../x")),
            PathBuf::from("/x")
        );
    }

    #[test]
    fn ensure_within_accepts_descendant() {
        let root = Path::new("/dest");
        let target = Path::new("/dest/2017/D80/a.jpg");
        let out = ensure_within(root, target).expect("within");
        assert_eq!(out, PathBuf::from("/dest/2017/D80/a.jpg"));
    }

    #[test]
    fn ensure_within_rejects_escape() {
        let root = Path::new("/dest");
        for target in [
            "/dest/../../etc/passwd",
            "/dest/../evil.jpg",
            "/other/a.jpg",
        ] {
            let err = ensure_within(root, Path::new(target)).expect_err("must reject");
            assert_eq!(err.user_key(), "error.invalidPath");
        }
    }

    #[test]
    fn ensure_within_root_itself_ok() {
        let out = ensure_within(Path::new("/dest"), Path::new("/dest/./")).expect("ok");
        assert_eq!(out, PathBuf::from("/dest"));
    }
}
