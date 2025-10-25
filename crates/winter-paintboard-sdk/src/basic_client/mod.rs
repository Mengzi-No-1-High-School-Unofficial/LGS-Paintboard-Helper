//! `client` 模块提供了与 Winter Paintboard API 交互的不同客户端实现。
//! 它包括基础客户端、连接池客户端以及用于处理HTTP和WebSocket通信的提供者。

mod async_client;
mod batch_client;
mod factory;
mod http_client;
mod paintboard_client_trait;
mod ws_provider;

/// 导出异步客户端。
pub use async_client::AsyncClient;
/// 导出批量操作助手。
pub use batch_client::BatchHelper;
/// 从工厂模块导出客户端类型枚举和客户端创建工厂函数。
pub use factory::{create_client_by_type, ClientType};
/// 导出 HTTP 客户端提供者。
pub use http_client::HttpProvider;
/// 导出 Paintboard 客户端的 trait 定义。
pub use paintboard_client_trait::PaintboardClientTrait;
/// 导出 WebSocket 客户端提供者。
pub use ws_provider::WsProvider;

use crate::{
    config::Config,
    error::PaintboardError,
    models::{Board, Pos, Rgb},
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use async_trait::async_trait;