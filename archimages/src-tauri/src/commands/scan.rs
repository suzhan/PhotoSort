//! 扫描 IPC：spawn_blocking 跑流式扫描，scan-items 分批推送行，
//! scan-progress 节流推送进度；结果摘要随命令返回。
//!
//! Phase 3 尚无取消 UI，取消钩子占位（Phase 9 接 CancellationToken）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tracing::{info, warn};

use crate::core::duplicate::DestinationIndex;
use crate::core::metadata::{MetadataOptions, MetadataReader};
use crate::core::planner::Planner;
use crate::core::scanner::{self, ScanOptions, ScanStats};
use crate::error::{AppError, ErrorDto};
use crate::models::duplicate::DuplicateResult;
use crate::models::plan::PlanStatus;
use crate::models::progress::{JobPhase, ProgressEvent};
use crate::state::AppState;

/// scan-items 单批行数：兼顾事件频率与 IPC 开销。
const ITEM_BATCH_SIZE: usize = 200;
/// 进度事件节流间隔（按 found 数）。
const PROGRESS_INTERVAL: u64 = 500;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRequest {
    pub root: String,
    pub include_subfolders: bool,
}

/// 与前端 `ScanRow` 一一对应。Phase 3 只有扫描列，其余为 null。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanRowDto {
    pub seq: u64,
    pub source_path: String,
    pub size: Option<u64>,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub gps: Option<String>,
    pub target_path: Option<String>,
    pub status: String,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanItemsBatch {
    pub job_id: String,
    pub rows: Vec<ScanRowDto>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummaryDto {
    pub job_id: String,
    pub root: String,
    pub found: u64,
    pub skipped_hidden: u64,
    pub skipped_unsupported: u64,
    pub skipped_non_file: u64,
    pub errors: u64,
    pub cancelled: bool,
    /// 解析成功但相机/时间/GPS 全缺的数量。
    pub metadata_missing: u64,
    /// 所有可用引擎均解析失败的数量。
    pub metadata_failed: u64,
    /// 已生成 Plan 的数量（设置了目标目录才 > 0）。
    pub planned: u64,
    /// 目标路径已存在且无法自动避让的数量（待内容查重裁决）。
    pub collisions: u64,
    /// 内容核实为完全重复的数量。
    pub duplicates: u64,
    /// 规划失败（路径逃逸 / 模板渲染错误等）的数量。
    pub plan_errors: u64,
}

impl From<ScanStats> for ScanSummaryDto {
    fn from(s: ScanStats) -> Self {
        Self {
            job_id: String::new(),
            root: String::new(),
            found: s.found,
            skipped_hidden: s.skipped_hidden,
            skipped_unsupported: s.skipped_unsupported,
            skipped_non_file: s.skipped_non_file,
            errors: s.errors,
            cancelled: s.cancelled,
            metadata_missing: 0,
            metadata_failed: 0,
            planned: 0,
            collisions: 0,
            duplicates: 0,
            plan_errors: 0,
        }
    }
}

#[tauri::command]
pub async fn scan_photos(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ScanRequest,
) -> Result<ScanSummaryDto, ErrorDto> {
    let trimmed = request.root.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidPath("source root is empty".to_string()).into());
    }
    let settings = state.snapshot().map_err(ErrorDto::from)?;
    let metadata_options = MetadataOptions {
        use_modified_time_fallback: settings.metadata_fallback.use_modified_time,
    };

    // 设置了目标目录才进入完整预览（扫描 + 规划 + 查重）；模板非法直接报错，让用户先修规则。
    // 模板语法错误在进后台线程前就报出。
    let want_plan = settings.destination_directory.is_some();
    if want_plan {
        Planner::new(&settings).map_err(ErrorDto::from)?;
    }
    let destination = settings.destination_directory.clone();
    let duplicate_mode = settings.duplicate_mode;

    let root = PathBuf::from(trimmed);
    let job_id = uuid::Uuid::new_v4().to_string();
    let options = ScanOptions {
        include_subfolders: request.include_subfolders,
    };

    info!(job_id = %job_id, root = %root.to_string_lossy(), "scan started");

    let app2 = app.clone();
    let job_id2 = job_id.clone();
    let (stats, metadata_missing, metadata_failed, planned, collisions, duplicates, plan_errors) =
        tauri::async_runtime::spawn_blocking(move || {
            // 目标索引：stat-only 遍历目标树；失败降级为不查重（不阻塞预览）。
            let planner = match &destination {
                Some(dest) => {
                    let index = match DestinationIndex::build(dest, duplicate_mode) {
                        Ok(i) => Some(i),
                        Err(e) => {
                            warn!(error = %e, "destination index unavailable; dedupe degraded");
                            None
                        }
                    };
                    let p = Planner::new(&settings)?;
                    Some(match index {
                        Some(i) => p.with_index(i),
                        None => p,
                    })
                }
                None => None,
            };
            let mut reader = MetadataReader::new(metadata_options);
            let mut progress = ProgressEvent::new(&job_id2, JobPhase::Scanning, 0);
            let mut batch: Vec<ScanRowDto> = Vec::with_capacity(ITEM_BATCH_SIZE);
            let mut seq: u64 = 0;
            let mut missing: u64 = 0;
            let mut failed: u64 = 0;
            let mut planned_count: u64 = 0;
            let mut collision_count: u64 = 0;
            let mut duplicate_count: u64 = 0;
            let mut plan_error_count: u64 = 0;

            let stats = scanner::scan(&root, &options, &|| false, |photo| {
                seq += 1;
                let outcome = reader.read(&photo);
                let md = outcome.metadata;
                let has_any = md.taken_at.is_some()
                    || md.camera_model.is_some()
                    || md.camera_make.is_some()
                    || md.gps.is_some();
                let warning = if outcome.parse_failed {
                    failed += 1;
                    Some("metadataReadFailed".to_string())
                } else if !has_any {
                    missing += 1;
                    Some("missingExif".to_string())
                } else {
                    None
                };
                let camera = md.camera_model.clone().or(md.camera_make.clone());
                let lens = md.lens_model.clone().or(md.lens_make.clone());

                let (target_path, status) = match &planner {
                    Some(p) => {
                        planned_count += 1;
                        // 解析失败的文件以 None 交给 Planner，状态自然降级
                        let md_ref = if outcome.parse_failed {
                            None
                        } else {
                            Some(&md)
                        };
                        let plan = p.plan(&photo, md_ref);
                        match plan.status {
                            PlanStatus::Collision => collision_count += 1,
                            PlanStatus::Duplicate => duplicate_count += 1,
                            PlanStatus::Error => plan_error_count += 1,
                            _ => {}
                        }
                        // Duplicate 时展示已存在的档案文件，比规划目标更有信息量
                        let shown = match &plan.duplicate {
                            DuplicateResult::ExactDuplicate { existing_path } => {
                                existing_path.clone()
                            }
                            _ => plan.target_path.clone(),
                        };
                        (
                            Some(shown.to_string_lossy().into_owned()),
                            plan.status.as_str().to_string(),
                        )
                    }
                    None => (None, "scanned".to_string()),
                };

                batch.push(ScanRowDto {
                    seq,
                    source_path: photo.path.to_string_lossy().into_owned(),
                    size: Some(photo.size),
                    taken_at: md
                        .taken_at
                        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()),
                    camera,
                    lens,
                    gps: md
                        .gps
                        .map(|g| format!("{:.4},{:.4}", g.latitude, g.longitude)),
                    target_path,
                    status,
                    warning,
                });
                if batch.len() >= ITEM_BATCH_SIZE {
                    emit_items(&app2, &job_id2, &mut batch);
                }
                progress.current = seq;
                if seq.is_multiple_of(PROGRESS_INTERVAL) {
                    progress.recompute_percent();
                    let _ = app2.emit("scan-progress", &progress);
                }
            })?;

            if !batch.is_empty() {
                emit_items(&app2, &job_id2, &mut batch);
            }
            progress.phase = JobPhase::Done;
            progress.recompute_percent();
            let _ = app2.emit("scan-progress", &progress);
            Ok::<_, AppError>((
                stats,
                missing,
                failed,
                planned_count,
                collision_count,
                duplicate_count,
                plan_error_count,
            ))
        })
        .await
        .map_err(|e| AppError::Task(format!("scan task join: {e}")))?
        .map_err(ErrorDto::from)?;

    info!(
        job_id = %job_id,
        found = stats.found,
        errors = stats.errors,
        cancelled = stats.cancelled,
        "scan finished"
    );

    let mut dto = ScanSummaryDto::from(stats);
    dto.job_id = job_id;
    dto.root = trimmed.to_string();
    dto.metadata_missing = metadata_missing;
    dto.metadata_failed = metadata_failed;
    dto.planned = planned;
    dto.collisions = collisions;
    dto.duplicates = duplicates;
    dto.plan_errors = plan_errors;
    Ok(dto)
}

fn emit_items(app: &AppHandle, job_id: &str, batch: &mut Vec<ScanRowDto>) {
    let payload = ScanItemsBatch {
        job_id: job_id.to_string(),
        rows: std::mem::take(batch),
    };
    let _ = app.emit("scan-items", payload);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_dto_serializes_camel_case() {
        let dto = ScanSummaryDto {
            job_id: "j1".to_string(),
            root: "/photos".to_string(),
            found: 3,
            skipped_hidden: 1,
            skipped_unsupported: 2,
            skipped_non_file: 0,
            errors: 0,
            cancelled: false,
            metadata_missing: 1,
            metadata_failed: 0,
            planned: 3,
            collisions: 1,
            duplicates: 2,
            plan_errors: 0,
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        assert!(json.contains("\"jobId\":\"j1\""));
        assert!(json.contains("\"skippedUnsupported\":2"));
        assert!(json.contains("\"metadataMissing\":1"));
        assert!(json.contains("\"planned\":3"));
        assert!(json.contains("\"collisions\":1"));
        assert!(json.contains("\"duplicates\":2"));
    }
}
