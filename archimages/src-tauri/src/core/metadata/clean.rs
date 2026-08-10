//! EXIF 字符串清洗：trim、去 NUL、折叠异常空白。
//! 相机固件经常写出带 NUL 填充或多余空格的字段，进模板前必须干净。

/// 清洗单个 EXIF 字符串。全部为空/NULL 时返回 None（交给 fallback 逻辑）。
pub fn clean_exif_string(raw: &str) -> Option<String> {
    let no_nul: String = raw.chars().filter(|&c| c != '\0').collect();
    // 折叠所有连续空白（含 tab / 不换行空格）为单个空格。
    let mut out = String::with_capacity(no_nul.len());
    let mut last_was_space = false;
    for c in no_nul.trim().chars() {
        if c.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(c);
            last_was_space = false;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_strips_nul() {
        assert_eq!(
            clean_exif_string("  NIKON D80 \u{0}\u{0} "),
            Some("NIKON D80".to_string())
        );
    }

    #[test]
    fn collapses_inner_whitespace() {
        assert_eq!(
            clean_exif_string("NIKON\u{a0}  D80"),
            Some("NIKON D80".to_string())
        );
    }

    #[test]
    fn all_blank_becomes_none() {
        assert_eq!(clean_exif_string("\u{0}\u{0}  \t"), None);
        assert_eq!(clean_exif_string(""), None);
    }
}
