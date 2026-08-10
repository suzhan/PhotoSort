//! 设置相关 IPC：读取 / 保存。校验在 AppSettings::validate 内集中完成。

use tauri::State;
use tracing::info;

use crate::error::ErrorDto;
use crate::models::settings::AppSettings;
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, ErrorDto> {
    state.snapshot().map_err(ErrorDto::from)
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<(), ErrorDto> {
    state.save_settings(settings).map_err(ErrorDto::from)?;
    info!("settings saved");
    Ok(())
}
