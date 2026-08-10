//! 序号协调器：保证并发整理时同一目标目录内的 {seq} 单调唯一。
//!
//! 语义：
//! - 每个目标目录独立计数（1 起）；
//! - 执行前由 Planner 用目录现有内容 seed（避免与已归档照片冲突）；
//! - 全部分配经同一把锁，天然并发安全。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct SequenceCoordinator {
    /// 目录 → 下一个待分配序号。
    counters: Mutex<HashMap<PathBuf, u64>>,
}

impl SequenceCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置目录的下一个序号（通常在扫描目标目录现有文件后调用）。
    /// 重复 seed 不覆盖已有更大值。
    pub fn seed(&self, dir: &Path, next: u64) {
        let mut guard = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(dir.to_path_buf()).or_insert(next);
        if next > *entry {
            *entry = next;
        }
    }

    /// 分配目录内下一个序号（从 1 开始）。
    pub fn next(&self, dir: &Path) -> u64 {
        let mut guard = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.entry(dir.to_path_buf()).or_insert(1);
        let value = *entry;
        *entry += 1;
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn sequential_per_directory() {
        let c = SequenceCoordinator::new();
        let a = Path::new("/dest/a");
        let b = Path::new("/dest/b");
        assert_eq!(c.next(a), 1);
        assert_eq!(c.next(a), 2);
        assert_eq!(c.next(b), 1); // 目录独立计数
        assert_eq!(c.next(a), 3);
    }

    #[test]
    fn seed_skips_existing_numbers() {
        let c = SequenceCoordinator::new();
        let dir = Path::new("/dest/2017");
        c.seed(dir, 100);
        assert_eq!(c.next(dir), 100);
        // 更小的 seed 不回退
        c.seed(dir, 5);
        assert_eq!(c.next(dir), 101);
    }

    #[test]
    fn concurrent_allocations_are_unique() {
        let c = Arc::new(SequenceCoordinator::new());
        let dir = Path::new("/dest/shared").to_path_buf();
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let c = Arc::clone(&c);
                let dir = dir.clone();
                std::thread::spawn(move || (0..500).map(|_| c.next(&dir)).collect::<Vec<u64>>())
            })
            .collect();
        let mut all = HashSet::new();
        for t in threads {
            for n in t.join().expect("join") {
                assert!(all.insert(n), "duplicate seq allocated: {n}");
            }
        }
        assert_eq!(all.len(), 4000);
        assert!(all.contains(&1) && all.contains(&4000));
    }
}
