//! 模板引擎：目录模板与文件名模板共用同一解析/渲染管线。
//!
//! 设计红线：
//! - 词法 token 化，禁止用连续 String.replace 解析变量；
//! - 渲染输出的每个路径组件都过 `sanitize_path_component`（含字面值），
//!   `..` 之类逃逸在清洗阶段即被消灭，最终再由 `ensure_within` 兜底；
//! - `{seq}` 只允许出现在文件名模板（目录序号无意义且会破坏确定性）。

mod context;
mod render;

pub use context::TemplateContext;
pub use render::{render_directory, render_filename};

use crate::error::{AppError, Result};

/// 序号宽度上限：{seq:10} 足够 100 亿，再大无意义。
pub const MAX_SEQ_WIDTH: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateToken {
    Literal(String),
    Variable(Variable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variable {
    Yyyy,
    Mm,
    Dd,
    YyyyMmDd,
    YyyyMmDdDash,
    Hh,
    Mi,
    Ss,
    /// 需求示例 `{yyyyMMdd}_{HHmmss}` 里的复合变量。
    HhMmSs,
    CameraMake,
    CameraModel,
    LensMake,
    LensModel,
    GpsCountry,
    GpsProvince,
    GpsCity,
    GpsDistrict,
    OriginalName,
    Extension,
    /// None = 不补零；Some(w) = 按宽度补零。
    Seq {
        width: Option<usize>,
    },
}

#[derive(Debug, Clone)]
pub struct Template {
    tokens: Vec<TemplateToken>,
    source: String,
}

impl Template {
    pub fn parse(input: &str) -> Result<Self> {
        let mut tokens = Vec::new();
        let mut literal = String::new();
        let mut chars = input.chars().peekable();

        macro_rules! flush_literal {
            () => {
                if !literal.is_empty() {
                    tokens.push(TemplateToken::Literal(std::mem::take(&mut literal)));
                }
            };
        }

        while let Some(c) = chars.next() {
            match c {
                '{' => {
                    if chars.peek() == Some(&'{') {
                        chars.next();
                        literal.push('{');
                        continue;
                    }
                    flush_literal!();
                    let mut name = String::new();
                    let mut closed = false;
                    for c2 in chars.by_ref() {
                        match c2 {
                            '}' => {
                                closed = true;
                                break;
                            }
                            '{' => {
                                return Err(AppError::Template(
                                    "nested '{' inside variable".to_string(),
                                ));
                            }
                            _ => name.push(c2),
                        }
                    }
                    if !closed {
                        return Err(AppError::Template(format!(
                            "unclosed '{{' in template: {input}"
                        )));
                    }
                    let variable = parse_variable(name.trim())?;
                    tokens.push(TemplateToken::Variable(variable));
                }
                '}' => {
                    if chars.peek() == Some(&'}') {
                        chars.next();
                        literal.push('}');
                    } else {
                        return Err(AppError::Template(format!(
                            "unmatched '}}' in template: {input}"
                        )));
                    }
                }
                _ => literal.push(c),
            }
        }
        flush_literal!();

        Ok(Self {
            tokens,
            source: input.to_string(),
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn tokens(&self) -> &[TemplateToken] {
        &self.tokens
    }

    pub fn uses_seq(&self) -> bool {
        self.tokens
            .iter()
            .any(|t| matches!(t, TemplateToken::Variable(Variable::Seq { .. })))
    }

    pub fn uses_date(&self) -> bool {
        self.uses_any(|v| {
            matches!(
                v,
                Variable::Yyyy
                    | Variable::Mm
                    | Variable::Dd
                    | Variable::YyyyMmDd
                    | Variable::YyyyMmDdDash
                    | Variable::Hh
                    | Variable::Mi
                    | Variable::Ss
                    | Variable::HhMmSs
            )
        })
    }

    pub fn uses_gps(&self) -> bool {
        self.uses_any(|v| {
            matches!(
                v,
                Variable::GpsCountry
                    | Variable::GpsProvince
                    | Variable::GpsCity
                    | Variable::GpsDistrict
            )
        })
    }

    pub fn uses_camera(&self) -> bool {
        self.uses_any(|v| matches!(v, Variable::CameraMake | Variable::CameraModel))
    }

    pub fn uses_lens(&self) -> bool {
        self.uses_any(|v| matches!(v, Variable::LensMake | Variable::LensModel))
    }

    pub fn uses_any(&self, pred: impl Fn(&Variable) -> bool) -> bool {
        self.tokens.iter().any(|t| match t {
            TemplateToken::Variable(v) => pred(v),
            TemplateToken::Literal(_) => false,
        })
    }

    /// 任何元数据相关变量（决定 MissingExif 是否有意义）。
    pub fn uses_metadata(&self) -> bool {
        self.uses_date() || self.uses_gps() || self.uses_camera() || self.uses_lens()
    }

    /// 目录模板专用校验：禁止 {seq}。
    pub fn validate_directory(input: &str) -> Result<Self> {
        let t = Self::parse(input)?;
        if t.uses_seq() {
            return Err(AppError::Template(
                "{seq} is only allowed in filename template".to_string(),
            ));
        }
        Ok(t)
    }

    /// 文件名模板专用校验：非空。
    pub fn validate_filename(input: &str) -> Result<Self> {
        let t = Self::parse(input)?;
        if t.tokens.is_empty() {
            return Err(AppError::Template("filename template is empty".to_string()));
        }
        Ok(t)
    }
}

fn parse_variable(name: &str) -> Result<Variable> {
    let (base, arg) = match name.split_once(':') {
        Some((b, a)) => (b, Some(a)),
        None => (name, None),
    };
    let variable = match base {
        "yyyy" => Variable::Yyyy,
        "MM" => Variable::Mm,
        "dd" => Variable::Dd,
        "yyyyMMdd" => Variable::YyyyMmDd,
        "yyyy-MM-dd" => Variable::YyyyMmDdDash,
        "HH" => Variable::Hh,
        "mm" => Variable::Mi,
        "ss" => Variable::Ss,
        "HHmmss" => Variable::HhMmSs,
        "camera_make" => Variable::CameraMake,
        "camera_model" => Variable::CameraModel,
        "lens_make" => Variable::LensMake,
        "lens_model" => Variable::LensModel,
        "gps_country" => Variable::GpsCountry,
        "gps_province" => Variable::GpsProvince,
        "gps_city" => Variable::GpsCity,
        "gps_district" => Variable::GpsDistrict,
        "original_name" => Variable::OriginalName,
        "extension" => Variable::Extension,
        "seq" => {
            let width = match arg {
                None => None,
                Some(a) => {
                    let w: usize = a
                        .parse()
                        .map_err(|_| AppError::Template(format!("invalid seq width: {a}")))?;
                    if w == 0 || w > MAX_SEQ_WIDTH {
                        return Err(AppError::Template(format!(
                            "seq width out of range 1..={MAX_SEQ_WIDTH}: {w}"
                        )));
                    }
                    Some(w)
                }
            };
            Variable::Seq { width }
        }
        _ => {
            return Err(AppError::Template(format!(
                "unknown template variable: {{{name}}}"
            )));
        }
    };
    if arg.is_some() && !matches!(variable, Variable::Seq { .. }) {
        return Err(AppError::Template(format!(
            "variable {{{base}}} does not take an argument"
        )));
    }
    Ok(variable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(t: &Template) -> Vec<Variable> {
        t.tokens
            .iter()
            .filter_map(|t| match t {
                TemplateToken::Variable(v) => Some(*v),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parses_mixed_template() {
        let t = Template::parse("{yyyy}/{camera_model}").expect("parse");
        assert_eq!(vars(&t), vec![Variable::Yyyy, Variable::CameraModel]);
        match &t.tokens()[1] {
            TemplateToken::Literal(s) => assert_eq!(s, "/"),
            _ => panic!("expected literal"),
        }
    }

    #[test]
    fn parses_all_documented_variables() {
        for (name, expect) in [
            ("yyyy", Variable::Yyyy),
            ("MM", Variable::Mm),
            ("dd", Variable::Dd),
            ("yyyyMMdd", Variable::YyyyMmDd),
            ("yyyy-MM-dd", Variable::YyyyMmDdDash),
            ("HH", Variable::Hh),
            ("mm", Variable::Mi),
            ("ss", Variable::Ss),
            ("HHmmss", Variable::HhMmSs),
            ("camera_make", Variable::CameraMake),
            ("camera_model", Variable::CameraModel),
            ("lens_make", Variable::LensMake),
            ("lens_model", Variable::LensModel),
            ("gps_country", Variable::GpsCountry),
            ("gps_province", Variable::GpsProvince),
            ("gps_city", Variable::GpsCity),
            ("gps_district", Variable::GpsDistrict),
            ("original_name", Variable::OriginalName),
            ("extension", Variable::Extension),
            ("seq", Variable::Seq { width: None }),
            ("seq:2", Variable::Seq { width: Some(2) }),
            ("seq:5", Variable::Seq { width: Some(5) }),
        ] {
            let t = Template::parse(&format!("{{{name}}}")).expect(name);
            assert_eq!(vars(&t), vec![expect], "variable {name}");
        }
    }

    #[test]
    fn escapes_double_braces() {
        let t = Template::parse("{{literal}}/{{yyyy}}").expect("parse");
        let rendered_literals: String = t
            .tokens()
            .iter()
            .filter_map(|t| match t {
                TemplateToken::Literal(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(rendered_literals, "{literal}/{yyyy}");
    }

    #[test]
    fn rejects_unknown_variable() {
        let err = Template::parse("{iso}").expect_err("unknown");
        assert_eq!(err.user_key(), "error.template");
        assert!(err.to_string().contains("iso"));
    }

    #[test]
    fn rejects_unclosed_and_unmatched_braces() {
        assert!(Template::parse("{yyyy").is_err());
        assert!(Template::parse("yyyy}").is_err());
        assert!(Template::parse("{yy{yy}}").is_err());
        assert!(Template::parse("{}").is_err());
        // 转义后的 { 不构成嵌套：{{nested{yyyy}}} = 字面值 + 变量
        assert!(Template::parse("{{nested{yyyy}}}").is_ok());
    }

    #[test]
    fn rejects_bad_seq_width() {
        assert!(Template::parse("{seq:0}").is_err());
        assert!(Template::parse("{seq:11}").is_err());
        assert!(Template::parse("{seq:x}").is_err());
        // 参数不允许挂在别的变量上
        assert!(Template::parse("{yyyy:4}").is_err());
    }

    #[test]
    fn seq_forbidden_in_directory_template() {
        assert!(Template::validate_directory("{yyyy}/{seq:4}").is_err());
        assert!(Template::validate_directory("{yyyy}/{camera_model}").is_ok());
        assert!(Template::validate_filename("{yyyyMMdd}_{seq:4}").is_ok());
    }

    #[test]
    fn variable_classification() {
        let t = Template::parse("{yyyy}/{gps_city}").expect("t");
        assert!(t.uses_date() && t.uses_gps());
        assert!(!t.uses_camera() && !t.uses_lens());
        assert!(t.uses_metadata());

        let t = Template::parse("{original_name}.{extension}").expect("t");
        assert!(!t.uses_metadata());
        assert!(!t.uses_seq());
    }
}
