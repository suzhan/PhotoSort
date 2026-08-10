//! ArchImages v3：照片归档、整理、重命名与查重工具。
//!
//! 分层约定：commands（IPC 薄层）→ core（业务核心）→ db / models / utils。
//! 数据安全优先于性能，任何文件变更必须可校验、可审计、可恢复。

pub mod commands;
pub mod config;
pub mod core;
pub mod db;
pub mod error;
pub mod models;
pub mod state;
pub mod utils;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;

            let log_dir = app.path().app_log_dir()?;
            let log_guard = utils::logging::init(&log_dir, app.handle().clone())?;

            let config_dir = app.path().app_config_dir()?;
            let store = config::JsonSettingsStore::new(config_dir.join("settings.json"));

            // SQLite 放 AppData（§二十一），启动即迁移 + 崩溃任务标记
            let data_dir = app.path().app_data_dir()?;
            let database = db::Database::open(&data_dir.join("archimages.db"))?;
            let interrupted = database.journal().mark_interrupted()?;
            if !interrupted.is_empty() {
                tracing::warn!(
                    count = interrupted.len(),
                    "found interrupted jobs from previous run"
                );
            }

            let app_state = state::AppState::new(store, log_guard, database)?;
            app.manage(app_state);

            tracing::info!(version = env!("CARGO_PKG_VERSION"), "ArchImages started");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::scan::scan_photos,
            commands::template::template_preview,
            commands::organize::organize_photos,
            commands::organize::cancel_job,
            commands::jobs::pending_recovery_jobs,
            commands::jobs::abandon_job,
            commands::geocode::set_google_api_key,
            commands::geocode::clear_google_api_key,
            commands::geocode::has_google_api_key,
            commands::geocode::test_geocode,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start ArchImages");
}
