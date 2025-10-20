use crate::{
    basic_client::BasicClient, config::Config, error::PaintboardError, HttpProvider, WsProvider,
};
use deadpool::managed::{Pool, PoolBuilder};
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};

use super::manager::DelayClientManager;

/// DelayOnly 连接池
pub struct DelayPool {
    http_provider: Arc<Mutex<HttpProvider>>,
    ws_provider: Arc<RwLock<WsProvider>>,
    pool: Pool<DelayClientManager>,
}

impl DelayPool {
    /// 创建新的 DelayPool
    pub async fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        tracing::info!("正在创建 DelayClient Pool，配置: max_size=128");

        let http_provider = HttpProvider::new(config.clone())?;
        let http_provider = Arc::new(Mutex::new(http_provider));

        let mut ws_provider = WsProvider::new(config.clone()).await?;
        ws_provider.connect().await?;

        let ws_provider = Arc::new(RwLock::new(ws_provider));

        let manager = DelayClientManager::new(http_provider.clone(), ws_provider.clone());

        tracing::debug!("正在构建连接池...");
        let pool = Pool::builder(manager)
            .max_size(128)
            .wait_timeout(Some(Duration::from_secs(30))) // 获取连接超时
            .create_timeout(Some(Duration::from_secs(10))) // 创建连接超时
            .recycle_timeout(Some(Duration::from_secs(5))) // 回收连接超时
            .runtime(deadpool::Runtime::Tokio1)
            .build()
            .map_err(|e| {
                tracing::error!("连接池构建失败: {}", e);
                PaintboardError::Internal(format!("Error occurred while building the pool: {}", e))
            })?;

        tracing::info!("DelayPool 创建成功");

        Ok(Self {
            pool,
            http_provider: http_provider.clone(),
            ws_provider: ws_provider.clone(),
        })
    }

    /// 从连接池获取连接
    /// 如果池中没有可用连接且未达到最大连接数，会创建新连接
    /// 如果达到最大连接数，会等待其他连接被归还
    pub async fn get(
        &self,
    ) -> Result<deadpool::managed::Object<DelayClientManager>, PaintboardError> {
        tracing::debug!(
            "当前可用的连接数：{}, 连接池大小：{}",
            self.pool.status().available,
            self.pool.status().size
        );

        self.pool
            .get()
            .await
            .map_err(|e| PaintboardError::Internal(e.to_string()))
    }
}

impl Drop for DelayPool {
    fn drop(&mut self) {
        tracing::debug!("WriteOnlyPool 正在被销毁");
    }
}
