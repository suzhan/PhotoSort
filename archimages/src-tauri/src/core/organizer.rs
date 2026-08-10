//! 执行器：消费 PhotoPlan 的有界 worker pool（需求 §十九/§二十/§四十六）。
//!
//! - 只执行 Planner 给出的 Plan，自己不做任何路径决策；
//! - 固定 N 个 worker + 有界 channel：并发有上限，内存有上限；
//! - 取消 = 令牌：worker 停止领取新任务，正在复制/校验的文件完成到安全状态；
//! - 预览与执行之间目标被别人占掉（TOCTOU）→ 按 §十三 自动改名重试一次；
//! - 单文件失败只记数 + 收集错误，绝不拖垮整批。

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Mutex, MutexGuard};

use serde::Serialize;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use super::file_ops::{
    atomic_copy, collision_free_name, copy_verify_delete, delete_verified_duplicate, safe_move,
};
use crate::error::AppError;
use crate::models::duplicate::{DuplicateMode, DuplicateResult};
use crate::models::plan::PhotoPlan;
use crate::models::settings::OperationMode;

/// 报告里保留的错误明细上限：全量进日志，报告只留样本。
const MAX_REPORTED_ERRORS: usize = 100;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileError {
    pub source: String,
    pub key: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizeReport {
    /// 收到的 Plan 总数。
    pub total: u64,
    pub success: u64,
    pub duplicate: u64,
    /// Collision / 不可执行 / 重复但删除被保守拒绝。
    pub skipped: u64,
    pub failed: u64,
    pub cancelled: bool,
    pub errors: Vec<FileError>,
}

/// 每个文件完成后的进度快照（由命令层节流后转成 ProgressEvent 发出）。
#[derive(Debug, Clone)]
pub struct ExecutorProgress {
    pub current: u64,
    pub current_file: Option<String>,
    pub success: u64,
    pub duplicate: u64,
    pub skipped: u64,
    pub failed: u64,
    /// 本次完成文件的终态，供任务日志记录。
    pub last: Option<FileOutcome>,
}

/// 文件终态：与 JobFileStatus 的写入侧子集一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOutcomeKind {
    Verified,
    SourceDeleted,
    Duplicate,
    Skipped,
    Failed,
}

#[derive(Debug, Clone)]
pub struct FileOutcome {
    pub source: PathBuf,
    /// 冲突改名重试后的最终目标；未发生改名时为计划目标。
    pub target: Option<PathBuf>,
    pub kind: FileOutcomeKind,
    pub error: Option<String>,
}

pub struct ExecutorConfig {
    pub operation: OperationMode,
    pub hash_mode: DuplicateMode,
    /// 来自 max_copy_workers，调用方已校验 1..=16。
    pub workers: usize,
    pub cancel: CancellationToken,
}

struct Shared {
    report: Mutex<OrganizeReport>,
}

impl Shared {
    fn lock_report(&self) -> MutexGuard<'_, OrganizeReport> {
        self.report.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn snapshot(
        &self,
        current_file: Option<String>,
        last: Option<FileOutcome>,
    ) -> ExecutorProgress {
        let r = self.lock_report();
        ExecutorProgress {
            current: r.success + r.duplicate + r.skipped + r.failed,
            current_file,
            success: r.success,
            duplicate: r.duplicate,
            skipped: r.skipped,
            failed: r.failed,
            last,
        }
    }

    fn record_error(&self, source: &Path, err: &AppError) {
        let mut r = self.lock_report();
        r.failed += 1;
        if r.errors.len() < MAX_REPORTED_ERRORS {
            r.errors.push(FileError {
                source: source.to_string_lossy().into_owned(),
                key: err.user_key().to_string(),
                message: err.to_string(),
            });
        }
    }
}

/// 执行单条 Plan，返回终态 outcome。
fn execute_one(plan: &PhotoPlan, config: &ExecutorConfig, shared: &Shared) -> FileOutcome {
    use OperationMode as Op;

    let source = plan.source.path.clone();
    if !plan.executable() {
        shared.lock_report().skipped += 1;
        return FileOutcome {
            source,
            target: None,
            kind: FileOutcomeKind::Skipped,
            error: None,
        };
    }

    if let DuplicateResult::ExactDuplicate { existing_path } = &plan.duplicate {
        match config.operation {
            Op::Copy => {
                // 复制模式：目标已有同内容文件，无需再复制，源保留
                shared.lock_report().duplicate += 1;
                FileOutcome {
                    source,
                    target: Some(existing_path.clone()),
                    kind: FileOutcomeKind::Duplicate,
                    error: None,
                }
            }
            Op::Move | Op::CopyVerifyDelete => {
                match delete_verified_duplicate(&plan.source.path, existing_path, config.hash_mode)
                {
                    Ok(true) => {
                        shared.lock_report().duplicate += 1;
                        FileOutcome {
                            source,
                            target: Some(existing_path.clone()),
                            kind: FileOutcomeKind::Duplicate,
                            error: None,
                        }
                    }
                    Ok(false) => {
                        // 核验不过 → 保守保留源
                        shared.lock_report().skipped += 1;
                        FileOutcome {
                            source,
                            target: None,
                            kind: FileOutcomeKind::Skipped,
                            error: None,
                        }
                    }
                    Err(e) => {
                        shared.record_error(&plan.source.path, &e);
                        FileOutcome {
                            source,
                            target: None,
                            kind: FileOutcomeKind::Failed,
                            error: Some(e.to_string()),
                        }
                    }
                }
            }
        }
    } else {
        match write_with_collision_retry(plan, config) {
            Ok(final_target) => {
                shared.lock_report().success += 1;
                FileOutcome {
                    source,
                    target: Some(final_target),
                    kind: match config.operation {
                        Op::Copy | Op::Move => FileOutcomeKind::Verified,
                        Op::CopyVerifyDelete => FileOutcomeKind::SourceDeleted,
                    },
                    error: None,
                }
            }
            Err(e) => {
                warn!(source = %plan.source.path.to_string_lossy(), error = %e, "file operation failed");
                shared.record_error(&plan.source.path, &e);
                FileOutcome {
                    source,
                    target: None,
                    kind: FileOutcomeKind::Failed,
                    error: Some(e.to_string()),
                }
            }
        }
    }
}

/// 执行写入类操作；冲突改名后返回最终目标路径。
fn write_with_collision_retry(
    plan: &PhotoPlan,
    config: &ExecutorConfig,
) -> crate::error::Result<PathBuf> {
    let source = &plan.source.path;
    let target = &plan.target_path;
    let result = match config.operation {
        OperationMode::Copy => atomic_copy(source, target, Some(config.hash_mode)),
        OperationMode::Move => safe_move(source, target, config.hash_mode).map(|_| ()),
        OperationMode::CopyVerifyDelete => copy_verify_delete(source, target, config.hash_mode),
    };
    match result {
        Err(AppError::TargetExists(_)) => {
            // 预览后目标被别人占掉：按 §十三 改名重试一次
            let renamed = collision_free_name(target, |p| p.exists())?;
            info!(
                from = %target.to_string_lossy(),
                to = %renamed.to_string_lossy(),
                "target occupied since preview; renamed"
            );
            match config.operation {
                OperationMode::Copy => {
                    atomic_copy(source, &renamed, Some(config.hash_mode)).map(|_| renamed)
                }
                OperationMode::Move => {
                    safe_move(source, &renamed, config.hash_mode).map(|_| renamed)
                }
                OperationMode::CopyVerifyDelete => {
                    copy_verify_delete(source, &renamed, config.hash_mode).map(|_| renamed)
                }
            }
        }
        other => other.map(|_| target.clone()),
    }
}

/// 消费 channel 中的 Plan 直到关闭或取消。阻塞调用：请放在 spawn_blocking / 专用线程里。
pub fn execute_plans(
    rx: Receiver<PhotoPlan>,
    config: ExecutorConfig,
    on_progress: impl FnMut(ExecutorProgress) + Send,
) -> OrganizeReport {
    let shared = Shared {
        report: Mutex::new(OrganizeReport::default()),
    };
    let rx = Mutex::new(rx);
    // FnMut 回调跨 worker 共享：Mutex 串行化回调（调用极轻，非瓶颈）
    let on_progress = Mutex::new(on_progress);

    std::thread::scope(|scope| {
        for _ in 0..config.workers.max(1) {
            scope.spawn(|| loop {
                // 停止领取新任务；在途文件完成后走到这里自然退出
                if config.cancel.is_cancelled() {
                    return;
                }
                let plan = {
                    let guard = rx.lock().unwrap_or_else(|e| e.into_inner());
                    guard.recv()
                };
                let plan = match plan {
                    Ok(p) => p,
                    Err(_) => return, // 生产者结束
                };
                {
                    let mut r = shared.lock_report();
                    r.total += 1;
                }
                let current_file = plan.source.path.to_string_lossy().into_owned();
                let outcome = execute_one(&plan, &config, &shared);
                let progress = shared.snapshot(Some(current_file), Some(outcome));
                let mut cb = on_progress.lock().unwrap_or_else(|e| e.into_inner());
                (cb)(progress);
            });
        }
    });

    let mut report = shared.lock_report().clone();
    report.cancelled = config.cancel.is_cancelled();
    report
}

/// 供命令层组装执行器配置；目标索引存在与否由调用方决定。
pub fn executor_config(
    settings: &crate::models::settings::AppSettings,
    cancel: CancellationToken,
) -> ExecutorConfig {
    ExecutorConfig {
        operation: settings.operation_mode,
        hash_mode: settings.duplicate_mode,
        workers: settings.max_copy_workers as usize,
        cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::metadata::PhotoMetadata;
    use crate::models::photo::PhotoFile;
    use crate::models::plan::PlanStatus;
    use std::sync::mpsc::sync_channel;
    use std::time::SystemTime;

    fn write(path: &Path, content: &[u8]) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, content).expect("write");
    }

    fn plan_for(source: &Path, target: &Path) -> PhotoPlan {
        PhotoPlan {
            source: PhotoFile {
                path: source.to_path_buf(),
                size: std::fs::metadata(source).expect("stat").len(),
                extension: "jpg".to_string(),
                modified_time: SystemTime::UNIX_EPOCH,
            },
            metadata: Some(PhotoMetadata::default()),
            location: None,
            target_path: target.to_path_buf(),
            status: PlanStatus::Ready,
            duplicate: DuplicateResult::NotDuplicate,
            warnings: vec![],
        }
    }

    fn config(cancel: CancellationToken, op: OperationMode, workers: usize) -> ExecutorConfig {
        ExecutorConfig {
            operation: op,
            hash_mode: DuplicateMode::Modern,
            workers,
            cancel,
        }
    }

    #[test]
    fn executes_all_plans_with_bounded_workers() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(8);
        let src_dir = tmp.path().join("src");
        let dst_dir = tmp.path().join("dst");
        let mut sources = Vec::new();
        for i in 0..50 {
            let s = src_dir.join(format!("{i}.jpg"));
            write(&s, format!("content-{i}").as_bytes());
            sources.push(s);
        }
        let producer = {
            let dst = dst_dir.clone();
            std::thread::spawn(move || {
                for s in sources {
                    let t = dst.join(s.file_name().expect("name"));
                    tx.send(plan_for(&s, &t)).expect("send");
                }
            })
        };
        let report = execute_plans(
            rx,
            config(CancellationToken::new(), OperationMode::Copy, 4),
            |_| {},
        );
        producer.join().expect("join");
        assert_eq!(report.total, 50);
        assert_eq!(report.success, 50);
        assert_eq!(report.failed, 0);
        assert_eq!(std::fs::read_dir(&dst_dir).expect("readdir").count(), 50);
    }

    #[test]
    fn pre_cancelled_token_executes_nothing() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(4);
        let s = tmp.path().join("src/a.jpg");
        write(&s, b"x");
        tx.send(plan_for(&s, &tmp.path().join("dst/a.jpg")))
            .expect("send");
        drop(tx);
        let token = CancellationToken::new();
        token.cancel();
        let report = execute_plans(rx, config(token, OperationMode::Copy, 2), |_| {});
        assert!(report.cancelled);
        assert_eq!(report.total, 0, "取消时不领取任何任务");
        assert!(!tmp.path().join("dst/a.jpg").exists());
    }

    #[test]
    fn cancel_after_first_file_stops_rest() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(8);
        let src_dir = tmp.path().join("src");
        let token = CancellationToken::new();
        let token2 = token.clone();
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            let s = src_dir.join(name);
            write(&s, b"x");
            tx.send(plan_for(&s, &tmp.path().join("dst").join(name)))
                .expect("send");
        }
        drop(tx);
        let progressed = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let progressed2 = progressed.clone();
        let report = execute_plans(rx, config(token, OperationMode::Copy, 1), move |_| {
            progressed2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            token2.cancel(); // 第一个文件完成后取消
        });
        assert!(report.cancelled);
        assert_eq!(report.total, 1, "在途文件完成到安全状态后停止");
        assert_eq!(progressed.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(tmp.path().join("dst/a.jpg").exists());
        assert!(!tmp.path().join("dst/b.jpg").exists());
    }

    #[test]
    fn target_occupied_since_preview_is_renamed() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(4);
        let s = tmp.path().join("src/a.jpg");
        write(&s, b"new-content");
        let target = tmp.path().join("dst/a.jpg");
        let plan = plan_for(&s, &target);
        tx.send(plan).expect("send");
        drop(tx);
        // 预览之后目标被别人占掉
        write(&target, b"occupied");

        let report = execute_plans(
            rx,
            config(CancellationToken::new(), OperationMode::Copy, 1),
            |_| {},
        );
        assert_eq!(report.success, 1, "改名重试成功");
        assert_eq!(std::fs::read(&target).expect("read"), b"occupied");
        assert_eq!(
            std::fs::read(tmp.path().join("dst/a_1.jpg")).expect("read"),
            b"new-content"
        );
    }

    #[test]
    fn duplicate_in_cvd_mode_deletes_source_after_verify() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(4);
        let s = tmp.path().join("src/c.jpg");
        write(&s, b"dup-content");
        let existing = tmp.path().join("dst/archived/c.jpg");
        write(&existing, b"dup-content");

        let mut plan = plan_for(&s, &tmp.path().join("dst/should_not_appear.jpg"));
        plan.status = PlanStatus::Duplicate;
        plan.duplicate = DuplicateResult::ExactDuplicate {
            existing_path: existing.clone(),
        };
        tx.send(plan).expect("send");
        drop(tx);

        let report = execute_plans(
            rx,
            config(CancellationToken::new(), OperationMode::CopyVerifyDelete, 1),
            |_| {},
        );
        assert_eq!(report.duplicate, 1);
        assert!(!s.exists(), "核验通过，源已删");
        assert!(!tmp.path().join("dst/should_not_appear.jpg").exists());
    }

    #[test]
    fn duplicate_in_copy_mode_keeps_everything() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(4);
        let s = tmp.path().join("src/c.jpg");
        write(&s, b"dup");
        let existing = tmp.path().join("dst/e.jpg");
        write(&existing, b"dup");
        let mut plan = plan_for(&s, &tmp.path().join("dst/unused.jpg"));
        plan.status = PlanStatus::Duplicate;
        plan.duplicate = DuplicateResult::ExactDuplicate {
            existing_path: existing,
        };
        tx.send(plan).expect("send");
        drop(tx);
        let report = execute_plans(
            rx,
            config(CancellationToken::new(), OperationMode::Copy, 1),
            |_| {},
        );
        assert_eq!(report.duplicate, 1);
        assert!(s.exists(), "Copy 模式源保留");
    }

    #[test]
    fn single_failure_does_not_stop_batch() {
        let tmp = tempfile::tempdir().expect("tmp");
        let (tx, rx) = sync_channel::<PhotoPlan>(4);
        let ghost = tmp.path().join("src/ghost.jpg"); // 不存在的源
        let good = tmp.path().join("src/good.jpg");
        write(&good, b"good");
        // ghost 不存在：手工构造 plan（plan_for 会 stat 源）
        let ghost_plan = PhotoPlan {
            source: PhotoFile {
                path: ghost.clone(),
                size: 1,
                extension: "jpg".to_string(),
                modified_time: SystemTime::UNIX_EPOCH,
            },
            metadata: Some(PhotoMetadata::default()),
            location: None,
            target_path: tmp.path().join("dst/ghost.jpg"),
            status: PlanStatus::Ready,
            duplicate: DuplicateResult::NotDuplicate,
            warnings: vec![],
        };
        tx.send(ghost_plan).expect("send");
        tx.send(plan_for(&good, &tmp.path().join("dst/good.jpg")))
            .expect("send");
        drop(tx);
        let report = execute_plans(
            rx,
            config(CancellationToken::new(), OperationMode::Copy, 1),
            |_| {},
        );
        assert_eq!(report.failed, 1);
        assert_eq!(report.success, 1);
        assert_eq!(report.errors.len(), 1);
        assert!(tmp.path().join("dst/good.jpg").exists());
    }
}
