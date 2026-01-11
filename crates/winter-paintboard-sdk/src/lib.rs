//! Winter Paintboard SDK
//!
//! A Rust SDK for interacting with the Winter Paintboard 2026 API.
//! Provides functionality for painting, authentication, and real-time events.

use config::Config;
use std::sync::Arc;
use tokio::sync::OnceCell;

/// 全局 AsyncClient 实例
static GLOBAL_CLIENT: OnceCell<Arc<crate::basic_client::AsyncClient>> = OnceCell::const_new();

/// 获取全局 AsyncClient 实例
pub async fn get_global_client(
    config: Config,
) -> Result<Arc<crate::basic_client::AsyncClient>, crate::error::PaintboardError> {
    GLOBAL_CLIENT
        .get_or_try_init(|| async move {
            Ok(Arc::new(
                crate::basic_client::AsyncClient::new_impl(config).await?,
            ))
        })
        .await
        .cloned()
}

/// 客户端模块，包含与Paintboard API交互的不同客户端实现。
pub mod basic_client;
/// 配置模块，包含了SDK的各种配置选项。
pub mod config;
/// 错误处理模块，定义了SDK特有的错误类型。
pub mod error;
/// 错误统计模块，用于追踪绘制结果错误。
pub mod error_stats;
/// 事件模块，用于处理实时事件。
pub mod event;
/// 数据模型模块，定义了API请求和响应的数据结构。
pub mod models;
/// 实用工具模块，提供辅助函数和数据结构。
pub mod utils;

/// 从客户端模块导出主要客户端类型和工厂函数。
pub use basic_client::{
    create_client_by_type, AsyncClient, AsyncWsProvider, ClientType, HttpProvider,
    PaintboardClientTrait,
};
/// 导出SDK的统一错误类型。
pub use error::PaintboardError;
/// 从数据模型模块导出常用的数据结构。
pub use models::{Board, PaintResult, Pos, Rgb};
