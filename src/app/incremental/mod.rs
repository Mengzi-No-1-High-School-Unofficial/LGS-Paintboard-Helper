// 模块: src/app/incremental
//! 子模块组织：pixel_comparison, restoration_manager, incremental_service

pub mod incremental_service;
pub mod pixel_comparison;
pub mod restoration_manager;

// 便捷导出：增量服务的核心类型和启动函数
pub use incremental_service::start_incremental_if_enabled;
