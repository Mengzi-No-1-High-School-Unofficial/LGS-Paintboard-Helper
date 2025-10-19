use crate::{basic_client::BasicClient, config::Config, error::PaintboardError};
use deadpool::managed::{Pool, PoolBuilder};
use std::{sync::Arc, time::Duration};

use super::manager::WriteOnlyManager;

/// WriteOnly 连接池
/// 管理多个 WriteOnly 模式的 BasicClient 连接
pub struct WriteOnlyPool {
    pool: Pool<WriteOnlyManager>,
}

impl WriteOnlyPool {
    /// 创建新的 WriteOnlyPool
    /// 连接池大小限制为 5，与 API 限制一致
    pub async fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        tracing::info!("正在创建 WriteOnlyPool，配置: max_size=5");
        let manager = WriteOnlyManager::new(config);

        tracing::debug!("正在构建连接池...");
        let pool = Pool::builder(manager)
            .max_size(5) // API 限制：最多 5 个 WriteOnly 连接
            .wait_timeout(Some(Duration::from_secs(30))) // 获取连接超时
            .create_timeout(Some(Duration::from_secs(10))) // 创建连接超时
            .recycle_timeout(Some(Duration::from_secs(5))) // 回收连接超时
            .runtime(deadpool::Runtime::Tokio1)
            .build()
            .map_err(|e| {
                tracing::error!("连接池构建失败: {}", e);
                PaintboardError::Internal(format!("Error occurred while building the pool: {}", e))
            })?;

        tracing::info!("WriteOnlyPool 创建成功");
        Ok(Self { pool })
    }

    /// 从连接池获取连接
    /// 如果池中没有可用连接且未达到最大连接数，会创建新连接
    /// 如果达到最大连接数，会等待其他连接被归还
    pub async fn get(
        &self,
    ) -> Result<deadpool::managed::Object<WriteOnlyManager>, PaintboardError> {
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

impl Drop for WriteOnlyPool {
    fn drop(&mut self) {
        tracing::debug!("WriteOnlyPool 正在被销毁");
    }
}
