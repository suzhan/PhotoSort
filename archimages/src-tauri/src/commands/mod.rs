//! Tauri 命令层：只做参数校验与 DTO 映射，业务逻辑一律在 core/。

pub mod geocode;
pub mod jobs;
pub mod organize;
pub mod scan;
pub mod settings;
pub mod template;

/// 前后端连通性自检。
#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_pongs() {
        assert_eq!(ping(), "pong");
    }
}
