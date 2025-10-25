use crate::{error::PaintboardError, pool_client::DelayClient, HttpProvider, WsProvider};
use deadpool::managed::{Manager, RecycleResult};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// WriteOnly 连接池管理器
/// 负责创建和管理 WriteOnly 模式的 BasicClient 连接
pub struct DelayClientManager {
    http_provider: Arc<Mutex<HttpProvider>>,
    ws_provider: Arc<RwLock<WsProvider>>,
}

impl DelayClientManager {
    /// 创建新的 WriteOnlyManager
    pub fn new(
        http_provider: Arc<Mutex<HttpProvider>>,
        ws_provider: Arc<RwLock<WsProvider>>,
    ) -> Self {
        Self {
            http_provider,
            ws_provider,
        }
    }
}

impl Manager for DelayClientManager {
    type Type = DelayClient;
    type Error = PaintboardError;

    /// 创建新的 DelayClient 连接
    async fn create(&self) -> Result<DelayClient, PaintboardError> {
        let client = DelayClient::new(self.ws_provider.clone(), self.http_provider.clone());
        Ok(client)
    }

    /// 回收连接前的健康检查
    async fn recycle(
        &self,
        obj: &mut DelayClient,
        _metrics: &deadpool::managed::Metrics,
    ) -> RecycleResult<PaintboardError> {
        Ok(())
    }
}
