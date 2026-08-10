//! 业务核心层：扫描、元数据、模板、查重、文件操作、任务调度（按 Phase 填充）。

pub mod api_key;
pub mod duplicate;
pub mod file_ops;
pub mod geocode;
pub mod hash;
pub mod metadata;
pub mod organizer;
pub mod planner;
pub mod scanner;
pub mod sequence;
pub mod task_manager;
pub mod template;
