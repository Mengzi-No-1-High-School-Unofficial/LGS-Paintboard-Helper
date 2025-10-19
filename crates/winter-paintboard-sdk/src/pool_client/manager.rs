use crate::{
    basic_client::BasicClient,
    config::{Config, ConnectionMode},
    error::PaintboardError,
};
use deadpool::managed::{Manager, RecycleResult, RecycleError};
use std::sync::Arc;

/// WriteOnly 连接池管理器
/// 负责创建和管理 WriteOnly 模式的 BasicClient 连接
pub struct WriteOnlyManager {
    config: Arc<Config>,
}

impl WriteOnlyManager {
    /// 创建新的 WriteOnlyManager
    pub fn new(config: Arc<Config>) -> Self {
        Self { config }
    }
}

impl Manager for WriteOnlyManager {
    type Type = BasicClient;
    type Error = PaintboardError;

    /// 创建新的 WriteOnly BasicClient 连接
    async fn create(&self) -> Result<BasicClient, PaintboardError> {
        // 创建 WriteOnly 模式的配置
        let mut config = (*self.config).clone();
        config.connection_mode = ConnectionMode::WriteOnly;
        config.ws_url = format!("{}?writeonly=1", config.ws_url);

        // 创建 BasicClient 并预初始化 WebSocket 连接
        let mut client = BasicClient::new_impl(config).await?;
        client.init_ws_provider_if_none().await?;

        Ok(client)
    }

    /// 回收连接前的健康检查
    async fn recycle(&self, obj: &mut BasicClient, _metrics: &deadpool::managed::Metrics) -> RecycleResult<PaintboardError> {
        // 轻量检查：只检查健康状态标记
        if obj.is_healthy() {
            tracing::debug!("连接健康检查通过");
            Ok(())
        } else {
            tracing::debug!("连接标记为不健康，拒绝回收");
            Err(RecycleError::Message("连接不健康".into()))
        }
    }
}