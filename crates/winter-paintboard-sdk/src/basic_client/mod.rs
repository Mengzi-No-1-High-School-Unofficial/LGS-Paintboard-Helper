//! `client` 模块提供了与 Winter Paintboard API 交互的不同客户端实现。
//! 它包括基础客户端、连接池客户端以及用于处理HTTP和WebSocket通信的提供者。

mod batch_client;
mod factory;
mod http_client;
mod paintboard_client_trait;
mod ws_provider;

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

/// `BasicClient` 是与 Winter Paintboard API 交互的基础客户端。
/// 它封装了 HTTP 和 WebSocket 通信，并提供了获取画板数据、绘制像素等功能。
/// 认证信息通过方法参数传递，不保存在客户端状态中。
pub struct BasicClient {
    /// 用于 HTTP 请求的客户端。
    http_client: HttpProvider,
    /// 可选的 WebSocket 客户端，按需创建。
    ws_client: Option<WsProvider>,
    /// 共享配置。
    config: Arc<Config>,
    /// 连接健康状态标记
    healthy: Arc<AtomicBool>,
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

        let mut res = Self {
            http_client,
            ws_client: None,
            config,
            healthy: Arc::new(AtomicBool::new(true)),
        };

        res.init_ws_provider_if_none().await?;

        Ok(res)
    }

    /// 设置认证凭据 (UID 和令牌) (deprecated - use methods with auth parameters)
    pub fn set_auth_impl(&mut self, _uid: u32, _token: String) {
        // This method is deprecated in the new architecture
        // Authentication is now passed as parameters to each method
    }

    /// 标记连接为不健康
    pub fn mark_unhealthy(&self) {
        self.healthy.store(false, Ordering::Relaxed);
    }

    /// 检查连接是否健康
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    /// 重置健康状态
    pub fn reset_health(&self) {
        self.healthy.store(true, Ordering::Relaxed);
    }

    /// 处理操作结果，根据结果标记健康状态
    fn handle_result<T>(&self, result: Result<T, PaintboardError>) -> Result<T, PaintboardError> {
        match &result {
            Ok(_) => {
                // 只在不健康时才重置，避免不必要的原子操作
                if !self.is_healthy() {
                    self.reset_health();
                }
                result
            }
            Err(_) => {
                self.mark_unhealthy();
                result
            }
        }
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
    pub async fn get_token_impl(
        &self,
        uid: u32,
        access_key: &str,
    ) -> Result<String, PaintboardError> {
        self.http_client.get_token(uid, access_key).await
    }

    /// 在给定位置绘制一个像素，并指定颜色。
    /// 如果 WebSocket 客户端尚未初始化，则会先进行初始化。
    /// 使用提供的认证信息。
    ///
    /// # 参数
    /// - `pos`: 像素位置。
    /// - `color`: 像素颜色。
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，失败时包含 `PaintboardError`。
    pub async fn paint_with_auth_impl(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        // Get a mutable reference to the WebSocket client and call paint_with_auth
        self.init_ws_provider_if_none().await?;

        let result = if let Some(ref mut ws_client) = self.ws_client {
            // TODO: 将来允许配置
            ws_client.paint_delayed(pos, color, uid, token).await
            // ws_client.paint_with_auth(pos, color, uid, token).await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        };

        self.handle_result(result)
    }

    /// 在给定位置绘制一个像素，并指定颜色 (deprecated - use paint_with_auth_impl)
    pub async fn paint_impl(
        &mut self,
        pos: Pos,
        color: Rgb,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        Err(PaintboardError::auth(
            "Authentication required. Use paint_with_auth_impl instead.".to_string(),
        ))
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应)。
    /// 如果 WebSocket 客户端尚未初始化，则会先进行初始化。
    /// 使用提供的认证信息。
    ///
    /// # 参数
    /// - `operations`: 包含位置和颜色的操作向量。
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    pub async fn paint_batch_with_auth_impl(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        // Initialize WebSocket client if not already created
        self.init_ws_provider_if_none().await?;

        // Get a mutable reference to the WebSocket client and call paint_batch_with_auth
        let result = if let Some(ref mut ws_client) = self.ws_client {
            ws_client
                .paint_batch_with_auth(operations, uid, token)
                .await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        };

        self.handle_result(result)
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应) (deprecated - use paint_batch_with_auth_impl)
    pub async fn paint_batch_impl(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
    ) -> Result<(), PaintboardError> {
        Err(PaintboardError::auth(
            "Authentication required. Use paint_batch_with_auth_impl instead.".to_string(),
        ))
    }

    /// 初始化 WebSocket 提供者（如果尚未初始化）
    /// 注意：这是内部方法，主要供连接池管理器使用
    pub async fn init_ws_provider_if_none(&mut self) -> Result<(), PaintboardError> {
        // Initialize WebSocket client if not already created
        if self.ws_client.is_none() {
            let mut ws_client = WsProvider::new(self.config.clone()).await?;
            ws_client.connect().await?;
            self.ws_client = Some(ws_client);
        }

        Ok(())
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
        Self: Sized,
    {
        Self::new_impl(config).await
    }

    /// 设置认证凭据 (UID 和令牌) (deprecated - use methods with auth parameters)
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

    /// 在给定位置绘制一个像素，并指定颜色 (deprecated - use paint_with_auth)
    async fn paint(
        &mut self,
        pos: Pos,
        color: Rgb,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        self.paint_impl(pos, color).await
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应)。
    /// 使用提供的认证信息。
    ///
    /// # 参数
    /// - `operations`: 包含位置和颜色的操作向量。
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    async fn paint_batch_with_auth(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        self.paint_batch_with_auth_impl(operations, uid, token)
            .await
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应) (deprecated - use paint_batch_with_auth)
    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        self.paint_batch_impl(operations).await
    }

    /// 获取客户端配置信息。
    ///
    /// # 返回
    /// `Config` 的引用，用于获取连接信息等。
    fn get_config(&self) -> &Config {
        &self.config
    }

    /// 使用临时 Token 绘制像素（不修改客户端状态）。
    ///
    /// # 参数
    /// - `pos`: 像素位置。
    /// - `color`: 像素颜色。
    /// - `uid`: 临时 UID。
    /// - `token`: 临时认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，失败时包含 `PaintboardError`。
    async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        // 直接使用提供的认证信息，无需保存/恢复状态
        self.paint_with_auth_impl(pos, color, uid, &token).await
    }
}
