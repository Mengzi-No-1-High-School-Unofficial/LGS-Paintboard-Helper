use crate::{
    config::Config,
    error::PaintboardError,
    models::{Board, PaintOperation, Pos, Rgb},
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::paintboard_client_trait::PaintboardClientTrait;

/// `AsyncClient` 是与 Winter Paintboard API 交互的异步客户端。
/// 它基于 Actor 模型设计，避免了锁竞争问题，提供了高性能的异步操作。
/// 认证信息通过方法参数传递，不保存在客户端状态中。
pub struct AsyncClient {
    /// 用于 HTTP 请求的客户端。
    http_client: crate::basic_client::http_client::HttpProvider,
    /// 用于 WebSocket 操作的异步提供者
    ws_provider: crate::basic_client::ws_provider::AsyncWsProvider,
    /// 共享配置。
    config: Arc<Config>,
    /// 连接健康状态标记
    healthy: Arc<AtomicBool>,
}

impl AsyncClient {
    /// 使用给定的配置创建一个新的 `AsyncClient` 实例。
    ///
    /// # 参数
    /// - `config`: 客户端的配置。
    ///
    /// # 返回
    /// `Result`，成功时包含 `AsyncClient` 实例，失败时包含 `PaintboardError`。
    pub async fn new_impl(config: Config) -> Result<Self, PaintboardError> {
        let config = Arc::new(config);
        let http_client = crate::basic_client::http_client::HttpProvider::new(config.clone())?;
        let ws_provider = crate::basic_client::ws_provider::AsyncWsProvider::new(config.clone()).await?;
        
        // 初始化 WebSocket 连接
        ws_provider.connect().await?;
        
        Ok(Self {
            http_client,
            ws_provider,
            config,
            healthy: Arc::new(AtomicBool::new(true)),
        })
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
        &self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        // 使用延迟绘画方法，它会自动处理速率限制和批量发送
        let result = self.ws_provider.paint_delayed(pos, color, uid, token).await;
        self.handle_result(result)
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
    pub async fn paint_batch_with_auth_impl(
        &self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        let result = self.ws_provider
            .paint_batch_with_auth(operations, uid, token)
            .await;
        self.handle_result(result)
    }

    /// 使用多个 Token 批量绘制多个像素点。
    ///
    /// # 参数
    /// - `operations`: 包含完整绘画操作信息的向量。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    pub async fn paint_batch_multi_token_impl(
        &self,
        operations: Vec<PaintOperation>,
    ) -> Result<(), PaintboardError> {
        let result = self
            .ws_provider
            .paint_batch_multi_token(operations)
            .await;
        self.handle_result(result)
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
}


#[async_trait]
impl PaintboardClientTrait for AsyncClient {
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
    fn set_auth(&mut self, _uid: u32, _token: String) {
        // This method is deprecated in the new architecture
        // Authentication is now passed as parameters to each method
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
        &self,
        _pos: Pos,
        _color: Rgb,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        Err(PaintboardError::auth(
            "Authentication required. Use paint_with_auth instead.".to_string(),
        ))
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
        &self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        self.paint_batch_with_auth_impl(operations, uid, token)
            .await
    }

    /// 使用多个 Token 批量绘制多个像素点。
    ///
    /// # 参数
    /// - `operations`: 包含完整绘画操作信息的向量，每个操作可以有不同的 Token。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    async fn paint_batch_multi_token(
        &self,
        operations: Vec<PaintOperation>,
    ) -> Result<(), PaintboardError> {
        self.paint_batch_multi_token_impl(operations).await
    }

    /// 使用批量操作一次性绘制多个像素 (粘性数据包，不等待响应) (deprecated - use paint_batch_with_auth)
    async fn paint_batch(&self, _operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        Err(PaintboardError::auth(
            "Authentication required. Use paint_batch_with_auth instead.".to_string(),
        ))
    }

    /// 获取客户端配置信息。
    ///
    /// # 返回
    /// `Config` 的引用，用于获取连接信息等。
    fn get_config(&self) -> &Config {
        &self.config
    }

    /// 检查连接是否健康
    ///
    /// # 返回
    /// `bool`，如果连接健康则返回 `true`，否则返回 `false`。
    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
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
        &self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<crate::models::PaintResult, PaintboardError> {
        // 直接使用提供的认证信息，无需保存/恢复状态
        self.paint_with_auth_impl(pos, color, uid, &token).await
    }
}