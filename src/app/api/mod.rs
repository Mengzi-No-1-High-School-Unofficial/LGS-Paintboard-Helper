//! HTTP API 模块
//!
//! 提供监控数据的 HTTP API 接口

pub mod metrics_api;

pub use metrics_api::create_metrics_router;
