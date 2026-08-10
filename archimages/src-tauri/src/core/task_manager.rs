//! 任务管理器：job_id → CancellationToken 登记处。
//!
//! 取消语义（需求 §十九）：cancel 只置令牌，绝不 kill 线程；
//! 正在 copy/verify 的文件由执行器完成到安全状态后自行停下。

use std::collections::HashMap;
use std::sync::Mutex;

use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct TaskManager {
    tokens: Mutex<HashMap<String, CancellationToken>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记新任务并返回其令牌。
    pub fn register(&self, job_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let mut guard = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(job_id.to_string(), token.clone());
        token
    }

    /// 请求取消。任务不存在（已结束）时返回 false。
    pub fn cancel(&self, job_id: &str) -> bool {
        let guard = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        match guard.get(job_id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// 任务结束清理令牌。
    pub fn finish(&self, job_id: &str) {
        let mut guard = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(job_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_cancel_finish_lifecycle() {
        let tm = TaskManager::new();
        let token = tm.register("job-1");
        assert!(!token.is_cancelled());

        assert!(tm.cancel("job-1"));
        assert!(token.is_cancelled());

        assert!(tm.cancel("job-1"), "取消幂等：登记期内重复调用不报错");
        tm.finish("job-1");
        assert!(!tm.cancel("job-1"), "已结束任务的取消为 false");
    }

    #[test]
    fn jobs_are_independent() {
        let tm = TaskManager::new();
        let a = tm.register("a");
        let b = tm.register("b");
        tm.cancel("a");
        assert!(a.is_cancelled());
        assert!(!b.is_cancelled());
    }
}
