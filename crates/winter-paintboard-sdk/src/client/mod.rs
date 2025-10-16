//! `client` 模块提供了与 Winter Paintboard API 交互的不同客户端实现。
//! 它包括基础客户端、连接池客户端以及用于处理HTTP和WebSocket通信的提供者。

#[macro_use]
mod connection_pool;
mod http_client;
mod ws_provider;
mod batch_client;
mod paintboard_client_trait;
mod factory;

/// 导出 HTTP 客户端提供者。
pub use http_client::HttpProvider;
/// 导出 WebSocket 客户端提供者。
pub use ws_provider::WsProvider;
/// 导出批量操作助手。
pub use batch_client::BatchHelper;
/// 导出 Paintboard 客户端的 trait 定义。
pub use paintboard_client_trait::PaintboardClientTrait;
/// 从连接池模块导出连接池客户端、连接守卫、连接池指标和监控任务启动函数。
pub use connection_pool::{PoolClient, ConnectionGuard, PoolMetrics, start_monitoring_task};
/// 从工厂模块导出客户端类型枚举和客户端创建工厂函数。
pub use factory::{ClientType, create_client_by_type};

use crate::{
    error::PaintboardError,
    models::{Board, Rgb, Pos},
    config::Config
};
use std::sync::Arc;

use async_trait::async_trait;

/// `BasicClient` 是与 Winter Paintboard API 交互的基础客户端。
/// 它封装了 HTTP 和 WebSocket 通信，并提供了认证、获取画板数据、绘制像素等功能。
pub struct BasicClient {
    /// 用于 HTTP 请求的客户端。
    http_client: HttpProvider,
    /// 可选的 WebSocket 客户端，按需创建。
    ws_client: Option<WsProvider>,
    /// 共享配置。
    config: Arc<Config>,
    /// 用户ID，用于认证。
    uid: Option<u32>,
    /// 认证令牌。
    token: Option<String>,
}

impl BasicClient {
    /// 使用给定的配置创建一个新的 `BasicClient` 实例。
    ///
    /// # 参数
    /// - `config`: 客户端的配置。
    ///
    /// # 返回
    /// `Result`，成功时包含 `BasicClient` 实例，失败时包含 `PaintboardError`。
    pub async fn new_impl(config: Config) -> Result<Self, PaintboardError> {
        let config = Arc::new(config);
        let http_client = HttpProvider::new(config.clone())?;
        
        Ok(Self {
            http_client,
            ws_client: None,
            config,
            uid: None,
            token: None,
        })
    }

    /// 设置认证凭据 (UID 和令牌)。
    ///
    /// # 参数
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    pub fn set_auth_impl(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }

    /// 获取当前画板数据。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Board` 数据，失败时包含 `PaintboardError`。
    pub async fn get_board_impl(&self) -> Result<Board, PaintboardError> {
        self.http_client.get_board().await
    }

    /// 使用 UID 和访问密钥获取认证令牌。
    ///
    /// # 参数
    /// - `uid`: 用户ID。
    /// - `access_key`: 访问密钥。
    ///
    /// # 返回
    /// `Result`，成功时包含认证令牌字符串，失败时包含 `PaintboardError`。
    pub async fn get_token_impl(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        self.http_client.get_token(uid, access_key).await
    }

    /// 在给定位置绘制一个像素，并指定颜色。
    /// 如果 WebSocket 客户端尚未初始化，则会先进行初始化。
    ///
    /// # 参数
    /// - `pos`: 像素位置。
    /// - `color`: 像素颜色。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，失败时包含 `PaintboardError`。
    pub async fn paint_impl(&mut self, pos: Pos, color: Rgb) -> Result<crate::models::PaintResult, PaintboardError> {
        // Initialize WebSocket client if not already created
        if self.ws_client.is_none() {
            let mut ws_client = WsProvider::new(self.config.clone()).await?;
            
            // Set authentication if available
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                ws_client.set_auth(uid, token.clone());
            }
            
            self.ws_client = Some(ws_client);
        }
        
        // Get a mutable reference to the WebSocket client and call paint
        if let Some(ref mut ws_client) = self.ws_client {
            ws_client.paint(pos, color).await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        }
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应)。
    /// 如果 WebSocket 客户端尚未初始化，则会先进行初始化。
    ///
    /// # 参数
    /// - `operations`: 包含位置和颜色的操作向量。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    pub async fn paint_batch_impl(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        // Initialize WebSocket client if not already created
        if self.ws_client.is_none() {
            let mut ws_client = WsProvider::new(self.config.clone()).await?;
            
            // Set authentication if available
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                ws_client.set_auth(uid, token.clone());
            }
            
            self.ws_client = Some(ws_client);
        }
        
        // Get a mutable reference to the WebSocket client and call paint_batch
        if let Some(ref mut ws_client) = self.ws_client {
            ws_client.paint_batch(operations).await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        }
    }
}

#[async_trait]
impl PaintboardClientTrait for BasicClient {
    /// 使用给定的配置创建一个新的 `PaintboardClientTrait` 实现实例。
    ///
    /// # 参数
    /// - `config`: 客户端的配置。
    ///
    /// # 返回
    /// `Result`，成功时包含客户端实例，失败时包含 `PaintboardError`。
    async fn new(config: Config) -> Result<Self, PaintboardError>
    where
        Self: Sized
    {
        Self::new_impl(config).await
    }

    /// 设置认证凭据 (UID 和令牌)。
    ///
    /// # 参数
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    fn set_auth(&mut self, uid: u32, token: String) {
        self.set_auth_impl(uid, token);
    }

    /// 获取当前画板数据。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Board` 数据，失败时包含 `PaintboardError`。
    async fn get_board(&self) -> Result<Board, PaintboardError> {
        self.get_board_impl().await
    }

    /// 使用 UID 和访问密钥获取认证令牌。
    ///
    /// # 参数
    /// - `uid`: 用户ID。
    /// - `access_key`: 访问密钥。
    ///
    /// # 返回
    /// `Result`，成功时包含认证令牌字符串，失败时包含 `PaintboardError`。
    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        self.get_token_impl(uid, access_key).await
    }

    /// 在给定位置绘制一个像素，并指定颜色。
    ///
    /// # 参数
    /// - `pos`: 像素位置。
    /// - `color`: 像素颜色。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，失败时包含 `PaintboardError`。
    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<crate::models::PaintResult, PaintboardError> {
        self.paint_impl(pos, color).await
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应)。
    ///
    /// # 参数
    /// - `operations`: 包含位置和颜色的操作向量。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        self.paint_batch_impl(operations).await
    }
}