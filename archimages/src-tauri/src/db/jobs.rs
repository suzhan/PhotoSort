//! 任务日志 DAO（需求 §十六）：jobs / job_files。
//! 写入全部批量事务化——单文件单事务意味着每文件一次 fsync，10 万行扛不住。

use std::path::{Path, PathBuf};

use rusqlite::params;

use super::{now_unix, Database};
use crate::error::{AppError, Result};
use crate::models::job::{Job, JobFileStatus, JobKind, JobStatus};

#[derive(Clone)]
pub struct JobJournal {
    db: Database,
}

/// 启动恢复展示用：未完成任务 + 进度计数。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingJobSummary {
    pub job_id: String,
    pub kind: JobKind,
    pub source_root: Option<String>,
    pub destination_root: Option<String>,
    pub started_at: i64,
    pub total_files: i64,
    pub finished_files: i64,
}

/// 终态集合：恢复计数与"未完成"判定共用。
const FINISHED_FILE_STATUSES: &[&str] = &[
    "verified",
    "sourceDeleted",
    "duplicate",
    "skipped",
    "failed",
    "cancelled",
];

impl JobJournal {
    pub(crate) fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn begin_job(
        &self,
        id: &str,
        kind: JobKind,
        source_root: Option<&Path>,
        destination_root: Option<&Path>,
        settings_json: Option<&str>,
    ) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "INSERT INTO jobs (id, kind, status, source_root, destination_root, settings_json, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                kind.as_str(),
                JobStatus::Running.as_str(),
                source_root.map(|p| p.to_string_lossy().into_owned()),
                destination_root.map(|p| p.to_string_lossy().into_owned()),
                settings_json,
                now_unix(),
            ],
        )
        .map_err(|e| AppError::Database(format!("begin job: {e}")))?;
        Ok(())
    }

    /// 批量插入 Planned 行（单事务）。同 job 同源路径去重（重跑时忽略旧的）。
    pub fn record_planned(&self, job_id: &str, rows: &[(PathBuf, Option<PathBuf>)]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("tx planned: {e}")))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO job_files (job_id, source_path, target_path, status, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(job_id, source_path) DO NOTHING",
                )
                .map_err(|e| AppError::Database(format!("prepare planned: {e}")))?;
            for (source, target) in rows {
                stmt.execute(params![
                    job_id,
                    source.to_string_lossy(),
                    target.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    JobFileStatus::Planned.as_str(),
                    now_unix(),
                ])
                .map_err(|e| AppError::Database(format!("insert planned: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("commit planned: {e}")))?;
        Ok(())
    }

    /// 批量写终态 outcome（单事务）；目标被改名重试过时顺带更新 target_path。
    pub fn record_outcomes(
        &self,
        job_id: &str,
        outcomes: &[(PathBuf, JobFileStatus, Option<PathBuf>, Option<String>)],
    ) -> Result<()> {
        if outcomes.is_empty() {
            return Ok(());
        }
        let mut conn = self.db.lock();
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(format!("tx outcomes: {e}")))?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO job_files (job_id, source_path, target_path, status, error, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(job_id, source_path) DO UPDATE SET
                       status = excluded.status,
                       error = excluded.error,
                       target_path = COALESCE(excluded.target_path, job_files.target_path),
                       updated_at = excluded.updated_at",
                )
                .map_err(|e| AppError::Database(format!("prepare outcome: {e}")))?;
            for (source, status, target, error) in outcomes {
                stmt.execute(params![
                    job_id,
                    source.to_string_lossy(),
                    target.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    status.as_str(),
                    error,
                    now_unix(),
                ])
                .map_err(|e| AppError::Database(format!("insert outcome: {e}")))?;
            }
        }
        tx.commit()
            .map_err(|e| AppError::Database(format!("commit outcomes: {e}")))?;
        Ok(())
    }

    pub fn finish_job(&self, job_id: &str, status: JobStatus) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE jobs SET status = ?2, finished_at = ?3 WHERE id = ?1",
            params![job_id, status.as_str(), now_unix()],
        )
        .map_err(|e| AppError::Database(format!("finish job: {e}")))?;
        Ok(())
    }

    /// 启动时调用：上次的 Running 全是异常退出 → Interrupted。返回受影响任务。
    pub fn mark_interrupted(&self) -> Result<Vec<Job>> {
        let interrupted = self.list_by_status(JobStatus::Running)?;
        if interrupted.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.db.lock();
        conn.execute(
            "UPDATE jobs SET status = ?1 WHERE status = ?2",
            params![JobStatus::Interrupted.as_str(), JobStatus::Running.as_str()],
        )
        .map_err(|e| AppError::Database(format!("mark interrupted: {e}")))?;
        Ok(interrupted)
    }

    /// 待恢复的 Interrupted 任务 + 进度计数。
    pub fn pending_recovery(&self) -> Result<Vec<PendingJobSummary>> {
        let jobs = self.list_by_status(JobStatus::Interrupted)?;
        let mut out = Vec::with_capacity(jobs.len());
        let finished_list = FINISHED_FILE_STATUSES.join("','");
        for job in jobs {
            let conn = self.db.lock();
            let (total, finished): (i64, i64) = conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*), COALESCE(SUM(status IN ('{finished_list}')), 0)
                         FROM job_files WHERE job_id = ?1"
                    ),
                    params![job.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| AppError::Database(format!("count job files: {e}")))?;
            out.push(PendingJobSummary {
                job_id: job.id,
                kind: job.kind,
                source_root: job.source_root.map(|p| p.to_string_lossy().into_owned()),
                destination_root: job
                    .destination_root
                    .map(|p| p.to_string_lossy().into_owned()),
                started_at: job.started_at,
                total_files: total,
                finished_files: finished,
            });
        }
        Ok(out)
    }

    /// 用户选择放弃：Interrupted → Abandoned（只改标记，绝不动文件）。
    pub fn abandon(&self, job_id: &str) -> Result<()> {
        let conn = self.db.lock();
        conn.execute(
            "UPDATE jobs SET status = ?2, finished_at = ?3 WHERE id = ?1 AND status = ?4",
            params![
                job_id,
                JobStatus::Abandoned.as_str(),
                now_unix(),
                JobStatus::Interrupted.as_str()
            ],
        )
        .map_err(|e| AppError::Database(format!("abandon job: {e}")))?;
        Ok(())
    }

    fn list_by_status(&self, status: JobStatus) -> Result<Vec<Job>> {
        let conn = self.db.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, status, source_root, destination_root, settings_json, started_at, finished_at
                 FROM jobs WHERE status = ?1 ORDER BY started_at DESC",
            )
            .map_err(|e| AppError::Database(format!("prepare jobs: {e}")))?;
        let rows = stmt
            .query_map(params![status.as_str()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                ))
            })
            .map_err(|e| AppError::Database(format!("query jobs: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, kind, status, source, dest, settings, started, finished) =
                row.map_err(|e| AppError::Database(format!("row jobs: {e}")))?;
            out.push(Job {
                id,
                kind: JobKind::parse(&kind)
                    .ok_or_else(|| AppError::Database(format!("bad job kind: {kind}")))?,
                status: JobStatus::parse(&status)
                    .ok_or_else(|| AppError::Database(format!("bad job status: {status}")))?,
                source_root: source.map(PathBuf::from),
                destination_root: dest.map(PathBuf::from),
                settings_json: settings,
                started_at: started,
                finished_at: finished,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn begin(journal: &JobJournal, id: &str) {
        journal
            .begin_job(
                id,
                JobKind::Organize,
                Some(Path::new("/src")),
                Some(Path::new("/dst")),
                Some("{}"),
            )
            .expect("begin");
    }

    #[test]
    fn lifecycle_begin_plan_outcome_finish() {
        let db = Database::open_in_memory().expect("db");
        let journal = db.journal();
        begin(&journal, "j1");

        journal
            .record_planned(
                "j1",
                &[
                    (
                        PathBuf::from("/src/a.jpg"),
                        Some(PathBuf::from("/dst/a.jpg")),
                    ),
                    (
                        PathBuf::from("/src/b.jpg"),
                        Some(PathBuf::from("/dst/b.jpg")),
                    ),
                ],
            )
            .expect("planned");
        journal
            .record_outcomes(
                "j1",
                &[(
                    PathBuf::from("/src/a.jpg"),
                    JobFileStatus::SourceDeleted,
                    None,
                    None,
                )],
            )
            .expect("outcome");
        journal
            .finish_job("j1", JobStatus::Completed)
            .expect("finish");

        // 已完成任务不再出现在恢复列表
        assert!(journal.pending_recovery().expect("pending").is_empty());
        assert!(journal.mark_interrupted().expect("mark").is_empty());
    }

    #[test]
    fn running_job_is_marked_interrupted_and_listed() {
        let db = Database::open_in_memory().expect("db");
        let journal = db.journal();
        begin(&journal, "j-crash");
        journal
            .record_planned(
                "j-crash",
                &[
                    (
                        PathBuf::from("/src/a.jpg"),
                        Some(PathBuf::from("/dst/a.jpg")),
                    ),
                    (
                        PathBuf::from("/src/b.jpg"),
                        Some(PathBuf::from("/dst/b.jpg")),
                    ),
                    (
                        PathBuf::from("/src/c.jpg"),
                        Some(PathBuf::from("/dst/c.jpg")),
                    ),
                ],
            )
            .expect("planned");
        journal
            .record_outcomes(
                "j-crash",
                &[(
                    PathBuf::from("/src/a.jpg"),
                    JobFileStatus::Verified,
                    None,
                    None,
                )],
            )
            .expect("outcome");

        // 模拟崩溃重启：Running → Interrupted
        let interrupted = journal.mark_interrupted().expect("mark");
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].id, "j-crash");

        let pending = journal.pending_recovery().expect("pending");
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].total_files, 3);
        assert_eq!(pending[0].finished_files, 1);

        // 放弃后不再提示
        journal.abandon("j-crash").expect("abandon");
        assert!(journal.pending_recovery().expect("pending").is_empty());
    }

    #[test]
    fn outcome_upserts_without_planned_row() {
        let db = Database::open_in_memory().expect("db");
        let journal = db.journal();
        begin(&journal, "j2");
        // 没走过 record_planned 也能记 outcome（防御：日志不完整时不丢终态）
        journal
            .record_outcomes(
                "j2",
                &[(
                    PathBuf::from("/src/x.jpg"),
                    JobFileStatus::Failed,
                    None,
                    Some("io error".to_string()),
                )],
            )
            .expect("outcome");
        let count: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM job_files WHERE job_id='j2' AND status='failed'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(count, 1);
    }
}
