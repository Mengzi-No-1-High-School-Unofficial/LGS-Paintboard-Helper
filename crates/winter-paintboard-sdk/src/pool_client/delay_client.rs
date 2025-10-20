use crate::{config::Config, Board, HttpProvider, PaintResult, PaintboardClientTrait, PaintboardError, Pos, Rgb, WsProvider};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::*;


// `DelayClient` 将请求制成 Pending Packets，加入 WsProvider 的队列中并等待
pub struct DelayClient {
    http_provider: Arc<Mutex<HttpProvider>>,
    ws_provider: Arc<RwLock<WsProvider>>,
    /// 被弃用，仅作兼容处理
    cred: Arc<RwLock<Option<(u32, String)>>>
}

impl DelayClient {
    pub fn new(
        ws_provider: Arc<RwLock<WsProvider>>,
        http_provider: Arc<Mutex<HttpProvider>>,
    ) -> Self {
        Self {
            ws_provider,
            http_provider,
            cred: Arc::new(RwLock::new(None))
        }
    }
}

#[async_trait::async_trait]
impl PaintboardClientTrait for DelayClient {
    async fn new(config: Config) -> Result<Self, PaintboardError>
    where
        Self: Sized,
    {
        Err(
            PaintboardError::Internal("抱歉！DelayClient 不能 `DelayClient::new(config)` 方法，请使用 DelayClient::new(ws_provider) 方法".to_string())
        )
    }

     /// 设置客户端的认证信息 (deprecated - use methods with auth parameters)
    fn set_auth(&mut self, uid: u32, token: String) {
        error!("使用已经被弃用的 set_auth 是极不恰当的，它会进行同步阻塞，并可能导致 Panic");

        let rt = tokio::runtime::Runtime::new();

        if let Err(ref e) = rt {
            error!("`set_auth` 执行失败，无法获取 Runtime: {:?}", e);
        }

        let rt = rt.unwrap();
        
        rt.block_on(async move {
            let mut guard = self.cred.write().await;
            *guard = Some((uid, token))
        })
    }

    /// 获取当前画板的完整数据。
    ///
    /// 此异步函数将从 API 获取整个画板的像素数据。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Board` 结构体，其中包含画板的宽度、高度和像素数据，
    /// 失败时包含 `PaintboardError`。
    async fn get_board(&self) -> Result<Board, PaintboardError> {
        self.http_provider.lock().await.get_board().await
    }

    /// 通过用户 ID (UID) 和访问密钥 (access_key) 获取认证令牌。
    ///
    /// 此异步函数通常用于首次认证以获取会话令牌。
    ///
    /// # 参数
    /// - `uid`: 用户的唯一标识符。
    /// - `access_key`: 用于验证用户身份的密钥。
    ///
    /// # 返回
    /// `Result`，成功时包含一个 `String` 类型的认证令牌，
    /// 失败时包含 `PaintboardError` (例如，认证失败或网络问题)。
    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        self.http_provider.lock().await.get_token(uid, access_key).await
    }

    /// 在画板的指定位置绘制一个像素 (deprecated - use paint_with_auth)
    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        if let Some((uid, token)) = self.cred.read().await.clone() {
            self.ws_provider.write().await.paint_delayed(pos, color, uid, &token).await
        } else {
            Err(PaintboardError::Auth("UID + Token 没有设置".to_string()))
        }
    }

    /// 批量绘制多个像素 (deprecated - use paint_batch_with_auth)
    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        if let Some((uid, token)) = self.cred.read().await.clone() {
            self.ws_provider.write().await.paint_batch_with_auth(operations, uid, &token).await
        } else {
            Err(PaintboardError::Auth("UID + Topaint_batchken 没有设置".to_string()))
        }
    }

    /// 获取客户端配置信息。
    ///
    /// # 返回
    /// `Config` 的引用，用于获取连接信息等。
    fn get_config(&self) -> &Config {
        unimplemented!("DelayClient 不能实现 `get_config")
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
        self.ws_provider.write().await.paint_delayed(pos, color, uid, &token).await
    }

    /// 使用提供的认证信息批量绘制像素 (新方法)
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
        self.ws_provider.write().await.paint_batch_with_auth(operations, uid, token).await
    }
}
