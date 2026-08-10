//! 整理执行 IPC：scan → metadata → plan → execute 一条后台流水线。
//!
//! 与预览共用同一 Scanner/MetadataReader/Planner（需求 §四十六：
//! Preview 与正式执行必须使用完全相同的逻辑）。执行期间：
//! - organize-progress：每文件完成计数，节流推送；
//! - job-complete：随命令返回汇总（事件同步推一份，便于日后多窗口）；
//! - 取消：cancel_job 置令牌，在途文件完成到安全状态后停止领取新任务；
//! - 任务日志（§十六）：job 全程入库，文件终态批量事务写，崩溃可恢复。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc::sync_channel, Arc};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tracing::{info, warn};

use crate::core::duplicate::DestinationIndex;
use crate::core::file_ops::sweep_stale_temps;
use crate::core::metadata::{MetadataOptions, MetadataReader};
use crate::core::organizer::{execute_plans, executor_config, FileOutcomeKind, OrganizeReport};
use crate::core::planner::Planner;
use crate::core::scanner::{self, ScanOptions};
use crate::error::{AppError, ErrorDto};
use crate::models::job::{JobFileStatus, JobKind, JobStatus};
use crate::models::plan::PhotoPlan;
use crate::models::progress::{JobPhase, ProgressEvent};
use crate::state::AppState;

/// 进度事件节流：每 N 个完成文件推一次。
const PROGRESS_EVERY: u64 = 25;
/// 日志批量：Planned 行每 500 条一个事务。
const PLANNED_BATCH: usize = 500;
/// 日志批量：终态行每 200 条一个事务。
const OUTCOME_BATCH: usize = 200;

/// 终态缓冲行：(source, status, final_target, error)。
type OutcomeRow = (PathBuf, JobFileStatus, Option<PathBuf>, Option<String>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeSummaryDto {
    pub job_id: String,
    #[serde(flatten)]
    pub report: OrganizeReport,
}

fn outcome_status(kind: FileOutcomeKind) -> JobFileStatus {
    match kind {
        FileOutcomeKind::Verified => JobFileStatus::Verified,
        FileOutcomeKind::SourceDeleted => JobFileStatus::SourceDeleted,
        FileOutcomeKind::Duplicate => JobFileStatus::Duplicate,
        FileOutcomeKind::Skipped => JobFileStatus::Skipped,
        FileOutcomeKind::Failed => JobFileStatus::Failed,
    }
}

#[tauri::command]
pub async fn organize_photos(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<OrganizeSummaryDto, ErrorDto> {
    let settings = state.snapshot().map_err(ErrorDto::from)?;
    let source = settings
        .source_directory
        .clone()
        .ok_or_else(|| AppError::Config("source directory not set".to_string()))
        .map_err(ErrorDto::from)?;
    let destination = settings
        .destination_directory
        .clone()
        .ok_or_else(|| AppError::Config("destination directory not set".to_string()))
        .map_err(ErrorDto::from)?;
    // 模板预检：语法错误在动任何文件之前报出
    Planner::new(&settings).map_err(ErrorDto::from)?;

    let job_id = uuid::Uuid::new_v4().to_string();
    let token = state.task_manager().register(&job_id);
    let journal = state.db().journal();
    let hash_cache = state.db().hash_cache();
    let gps_cache = state.db().gps_cache();
    let metadata_options = MetadataOptions {
        use_modified_time_fallback: settings.metadata_fallback.use_modified_time,
    };
    let include_subfolders = settings.include_subfolders;
    let hash_mode = settings.duplicate_mode;
    let workers = (settings.max_copy_workers as usize).max(1);
    let config = executor_config(&settings, token.clone());
    let settings_json = serde_json::to_string(&settings).ok();

    // 任务登记：从这里开始崩溃也能被下次启动发现
    journal
        .begin_job(
            &job_id,
            JobKind::Organize,
            Some(&source),
            Some(&destination),
            settings_json.as_deref(),
        )
        .map_err(ErrorDto::from)?;

    info!(job_id = %job_id, "organize started");
    let app2 = app.clone();
    let job2 = job_id.clone();
    let journal2 = journal.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        // 上次崩溃残留的临时文件先清掉
        let swept = sweep_stale_temps(&destination);
        if swept > 0 {
            warn!(swept, "swept stale temp files before organize");
        }

        let planner = match DestinationIndex::build(&destination, hash_mode) {
            Ok(index) => Planner::new(&settings)?.with_index(index.with_cache(hash_cache)),
            Err(e) => {
                warn!(error = %e, "destination index unavailable; dedupe degraded");
                Planner::new(&settings)?
            }
        };
        let mut reader = MetadataReader::new(metadata_options);

        // GPS 反查准备：仅当启用且有 API Key 时装配 Geocoder
        let geocoder: Option<crate::core::geocode::Geocoder> = if settings.gps_enabled {
            match crate::core::api_key::ApiKeyStore::get() {
                Ok(Some(key)) => match crate::core::geocode::Geocoder::new(key) {
                    Ok(g) => Some(g),
                    Err(e) => {
                        warn!(error = %e, "geocoder init failed; gps degraded to no-api mode");
                        None
                    }
                },
                Ok(None) => None,
                Err(e) => {
                    warn!(error = %e, "api key read failed; gps degraded");
                    None
                }
            }
        } else {
            None
        };
        let gps_cache = gps_cache.clone();
        let precision = settings.gps_round_precision;

        // 有界 channel：背压保证 10 万张照片不整批进内存
        let (tx, rx) = sync_channel::<PhotoPlan>(workers * 4);
        let produced = Arc::new(AtomicU64::new(0));
        let send_failed = Arc::new(AtomicBool::new(false));

        let report = std::thread::scope(|scope| {
            // 生产者：扫描 + 元数据 + 规划（只读）+ Planned 行批量入库
            let token_producer = token.clone();
            let produced2 = produced.clone();
            let send_failed2 = send_failed.clone();
            let journal3 = journal2.clone();
            let job4 = job2.clone();
            scope.spawn(move || {
                let stop = send_failed2.clone();
                let mut planned_buf: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
                let result = scanner::scan(
                    &source,
                    &ScanOptions { include_subfolders },
                    &|| token_producer.is_cancelled() || stop.load(Ordering::Relaxed),
                    |photo| {
                        let outcome = reader.read(&photo);
                        let md_ref = if outcome.parse_failed {
                            None
                        } else {
                            Some(&outcome.metadata)
                        };
                        // GPS 反查：缓存优先，未命中调 API，失败降级（不阻塞整批）
                        let pre_resolved = geocoder.as_ref().and_then(|g| {
                            let gps = md_ref.and_then(|m| m.gps)?;
                            let (lat_key, _) =
                                crate::core::geocode::normalize_coord(gps.latitude, precision);
                            let (lng_key, _) =
                                crate::core::geocode::normalize_coord(gps.longitude, precision);
                            if let Ok(Some(cached)) = gps_cache.lookup(&lat_key, &lng_key) {
                                return Some(cached.to_resolved());
                            }
                            match g.reverse(gps) {
                                Ok(loc) => {
                                    if let Err(e) = gps_cache.store(
                                        &lat_key,
                                        &lng_key,
                                        &crate::db::gps_cache::CachedLocation::from(&loc),
                                    ) {
                                        warn!(error = %e, "gps cache store failed");
                                    }
                                    Some(loc)
                                }
                                Err(e) => {
                                    warn!(error = %e, "geocode failed; degrading");
                                    None
                                }
                            }
                        });
                        let plan =
                            planner.plan_with_location(&photo, md_ref, pre_resolved.as_ref());
                        planned_buf
                            .push((plan.source.path.clone(), Some(plan.target_path.clone())));
                        if planned_buf.len() >= PLANNED_BATCH {
                            if let Err(e) = journal3.record_planned(&job4, &planned_buf) {
                                warn!(error = %e, "journal planned batch failed");
                            }
                            planned_buf.clear();
                        }
                        if tx.send(plan).is_err() {
                            send_failed2.store(true, Ordering::Relaxed);
                            return;
                        }
                        produced2.fetch_add(1, Ordering::Relaxed);
                    },
                );
                if !planned_buf.is_empty() {
                    if let Err(e) = journal3.record_planned(&job4, &planned_buf) {
                        warn!(error = %e, "journal planned flush failed");
                    }
                }
                // tx 随作用域结束自动 drop → 执行器收完队列后退出
                if let Err(e) = result {
                    warn!(error = %e, "producer scan failed mid-run");
                }
            });

            let app3 = app2.clone();
            let job5 = job2.clone();
            let produced3 = produced.clone();
            let journal5 = journal2.clone();
            let job6 = job2.clone();
            // 回调 move 语义：缓冲用 Arc<Mutex> 共享，执行完后统一收尾
            let outcomes_buf: Arc<std::sync::Mutex<Vec<OutcomeRow>>> =
                Arc::new(std::sync::Mutex::new(Vec::new()));
            let outcomes_buf2 = outcomes_buf.clone();
            let report = execute_plans(rx, config, move |p| {
                if let Some(last) = p.last {
                    let mut buf = outcomes_buf2.lock().unwrap_or_else(|e| e.into_inner());
                    buf.push((
                        last.source,
                        outcome_status(last.kind),
                        last.target,
                        last.error,
                    ));
                    if buf.len() >= OUTCOME_BATCH {
                        if let Err(e) = journal5.record_outcomes(&job6, &buf) {
                            warn!(error = %e, "journal outcomes batch failed");
                        }
                        buf.clear();
                    }
                }
                if p.current.is_multiple_of(PROGRESS_EVERY) {
                    let mut event = ProgressEvent::new(&job5, JobPhase::Executing, 0);
                    event.current = p.current;
                    event.total = produced3.load(Ordering::Relaxed);
                    event.current_file = p.current_file;
                    event.success = p.success;
                    event.duplicate = p.duplicate;
                    event.skipped = p.skipped;
                    event.failed = p.failed;
                    event.recompute_percent();
                    let _ = app3.emit("organize-progress", &event);
                }
            });
            {
                let buf = outcomes_buf.lock().unwrap_or_else(|e| e.into_inner());
                if !buf.is_empty() {
                    if let Err(e) = journal2.record_outcomes(&job2, &buf) {
                        warn!(error = %e, "journal outcomes flush failed");
                    }
                }
            }
            report
        });
        Ok::<_, AppError>(report)
    })
    .await
    .map_err(|e| AppError::Task(format!("organize task join: {e}")));

    state.task_manager().finish(&job_id);

    match result {
        Ok(Ok(report)) => {
            info!(
                job_id = %job_id,
                total = report.total,
                success = report.success,
                duplicate = report.duplicate,
                skipped = report.skipped,
                failed = report.failed,
                cancelled = report.cancelled,
                "organize finished"
            );
            let final_status = if report.cancelled {
                JobStatus::Cancelled
            } else if report.failed > 0 {
                JobStatus::CompletedWithErrors
            } else {
                JobStatus::Completed
            };
            if let Err(e) = journal.finish_job(&job_id, final_status) {
                warn!(error = %e, "finish job journal failed");
            }
            let summary = OrganizeSummaryDto {
                job_id: job_id.clone(),
                report,
            };
            let _ = app.emit("job-complete", &summary);
            Ok(summary)
        }
        Ok(Err(e)) => {
            let _ = journal.finish_job(&job_id, JobStatus::Failed);
            let dto = ErrorDto::from(e);
            let _ = app.emit("job-error", dto.clone());
            Err(dto)
        }
        Err(e) => {
            let _ = journal.finish_job(&job_id, JobStatus::Failed);
            let dto = ErrorDto::from(e);
            let _ = app.emit("job-error", dto.clone());
            Err(dto)
        }
    }
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job_id: String) -> bool {
    let found = state.task_manager().cancel(&job_id);
    info!(job_id = %job_id, found, "cancel requested");
    found
}
