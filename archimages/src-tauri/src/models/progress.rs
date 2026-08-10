//! 进度事件模型：Rust 后端推送，前端 Pinia 消费，字段与 TS ProgressEvent 一致。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JobPhase {
    Scanning,
    Metadata,
    Planning,
    Executing,
    Verifying,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressEvent {
    pub job_id: String,
    pub phase: JobPhase,
    pub current: u64,
    pub total: u64,
    /// 展示用路径（lossy），可为空。
    pub current_file: Option<String>,
    pub success: u64,
    pub skipped: u64,
    pub duplicate: u64,
    pub failed: u64,
    pub percent: f32,
}

impl ProgressEvent {
    pub fn new(job_id: impl Into<String>, phase: JobPhase, total: u64) -> Self {
        Self {
            job_id: job_id.into(),
            phase,
            current: 0,
            total,
            current_file: None,
            success: 0,
            skipped: 0,
            duplicate: 0,
            failed: 0,
            percent: 0.0,
        }
    }

    /// 由计数器重算百分比，避免各处手写除法（total=0 时为 0）。
    pub fn recompute_percent(&mut self) {
        self.percent = if self.total == 0 {
            0.0
        } else {
            (self.current as f32 / self.total as f32) * 100.0
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_math() {
        let mut p = ProgressEvent::new("job-1", JobPhase::Executing, 200);
        p.current = 50;
        p.recompute_percent();
        assert!((p.percent - 25.0).abs() < f32::EPSILON);
    }

    #[test]
    fn zero_total_is_zero_percent() {
        let mut p = ProgressEvent::new("job-2", JobPhase::Scanning, 0);
        p.recompute_percent();
        assert_eq!(p.percent, 0.0);
    }

    #[test]
    fn serializes_camel_case_for_frontend() {
        let p = ProgressEvent::new("j", JobPhase::Scanning, 1);
        let json = serde_json::to_string(&p).expect("serialize");
        assert!(json.contains("\"jobId\":\"j\""));
        assert!(json.contains("\"phase\":\"scanning\""));
    }
}
