//! Winter Paintboard SDK
//!
//! A Rust SDK for interacting with the Winter Paintboard 2026 API.
//! Provides functionality for painting, authentication, and real-time events.

/// 客户端模块，包含与Paintboard API交互的不同客户端实现。
pub mod basic_client;
/// 配置模块，包含了SDK的各种配置选项。
pub mod config;
/// 错误处理模块，定义了SDK特有的错误类型。
pub mod error;
/// 事件模块，用于处理实时事件。
pub mod event;
/// 数据模型模块，定义了API请求和响应的数据结构。
pub mod models;
/// 连接池工具
pub mod pool_client;
/// 实用工具模块，提供辅助函数和数据结构。
pub mod utils;

/// 从客户端模块导出主要客户端类型和工厂函数。
pub use basic_client::{create_client_by_type, BasicClient, ClientType, PaintboardClientTrait};
/// 导出SDK的统一错误类型。
pub use error::PaintboardError;
/// 从数据模型模块导出常用的数据结构。
pub use models::{Board, PaintResult, Pos, Rgb};
/// 从连接池模块导出连接池客户端。
pub use pool_client::PoolClient;
