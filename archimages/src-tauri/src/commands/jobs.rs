//! 崩溃恢复 IPC（需求 §十六）：启动后查询未完成任务；放弃只改标记，绝不动文件。

use tauri::State;

use crate::db::jobs::PendingJobSummary;
use crate::error::ErrorDto;
use crate::state::AppState;

/// 上次异常退出留下的 Interrupted 任务列表。
#[tauri::command]
pub fn pending_recovery_jobs(
    state: State<'_, AppState>,
) -> Result<Vec<PendingJobSummary>, ErrorDto> {
    state
        .db()
        .journal()
        .pending_recovery()
        .map_err(ErrorDto::from)
}

/// 放弃某个未完成任务：Interrupted → Abandoned。
/// “继续”不需要单独命令——重新点开始整理即可，流水线对既有文件天然安全。
#[tauri::command]
pub fn abandon_job(state: State<'_, AppState>, job_id: String) -> Result<(), ErrorDto> {
    state
        .db()
        .journal()
        .abandon(&job_id)
        .map_err(ErrorDto::from)
}
