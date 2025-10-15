use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;
use async_trait::async_trait;

use crate::{
    error::PaintboardError, 
    models::{Board, Rgb, Pos, PaintResult}, 
    config::Config,
    BasicClient,
    PaintboardClientTrait,
    with_retry,  // 导入宏
};

use super::pool::{ConnectionPool, ConnectionGuard};

// PoolClient 实现
pub struct PoolClient {
    pub pool: ConnectionPool,
    event_client: Arc<Mutex<Option<BasicClient>>>, // 专门用于事件监听的连接
}

use crate::client::connection_pool::manager::{PoolManagerConfig, start_connection_manager_task};

impl PoolClient {
    pub async fn new(config: Config, min_connections: usize, max_connections: usize) -> Result<Self, PaintboardError> {
        let pool = ConnectionPool::new(config, min_connections, max_connections);
        
        let pool_client = Self {
            pool,
            event_client: Arc::new(Mutex::new(None)),
        };
        
        // 启动后台连接管理器
        start_connection_manager_task(pool_client.pool.clone(), None).await;
        
        Ok(pool_client)
    }
    
    // 为连接池中的所有连接设置认证信息
    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.pool.set_auth(uid, token.clone());
    }
    
    // 获取池大小
    pub async fn pool_size(&self) -> usize {
        self.pool.pool_size().await
    }
    
    // 获取活跃连接数
    pub async fn active_count(&self) -> usize {
        self.pool.active_count().await
    }
    
    // 清理不活跃连接
    pub async fn cleanup_inactive_connections(&self, max_idle_time: Duration) {
        self.pool.cleanup_inactive_connections(max_idle_time).await;
    }
    
    // 为事件监听创建专用连接
    pub async fn setup_event_client(&mut self) -> Result<(), PaintboardError> {
        let mut event_client = BasicClient::new(self.pool.config.clone()).await?;
        if let (Some(uid), Some(token)) = (self.pool.uid, self.pool.token.as_ref()) {
            event_client.set_auth(uid, token.clone());
        }
        *self.event_client.lock().await = Some(event_client);
        Ok(())
    }
    
    // 内部方法：执行带故障转移的操作
    // 现在使用 ConnectionPool 的 execute_with_connection 方法，不再需要这个
}

#[async_trait]
impl PaintboardClientTrait for PoolClient {
    async fn new(config: Config) -> Result<Self, PaintboardError> 
    where 
        Self: Sized 
    {
        // 默认使用最小4个连接，最大7个连接
        Self::new(config, 4, 7).await
    }

    fn set_auth(&mut self, uid: u32, token: String) {
        self.pool.set_auth(uid, token);
    }

    async fn get_board(&self) -> Result<Board, PaintboardError> {
        // 增加请求计数
        self.pool.increment_requests(1).await;
        
        with_retry!({
            let client = self.pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.pool.clone()));
            
            match guard.as_mut().unwrap().get_board().await {
                Ok(result) => Ok(result),
                Err(e) => {
                    if let PaintboardError::ConnectionClosed = e {
                        // 如果是连接关闭错误，标记连接为损坏
                        guard.mark_broken();
                    }
                    Err(e)
                }
            }
        })
    }

    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        // 增加请求计数
        self.pool.increment_requests(1).await;
        
        let access_key = access_key.to_string();
        
        with_retry!({
            let client = self.pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.pool.clone()));
            
            match guard.as_mut().unwrap().get_token(uid, &access_key).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    if let PaintboardError::ConnectionClosed = e {
                        // 如果是连接关闭错误，标记连接为损坏
                        guard.mark_broken();
                    }
                    Err(e)
                }
            }
        })
    }

    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        // 增加请求计数
        self.pool.increment_requests(1).await;
        
        with_retry!({
            let client = self.pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.pool.clone()));
            
            match guard.as_mut().unwrap().paint(pos, color).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    if let PaintboardError::ConnectionClosed = e {
                        // 如果是连接关闭错误，标记连接为损坏
                        guard.mark_broken();
                    }
                    Err(e)
                }
            }
        })
    }

    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        // 增加批量请求计数
        self.pool.increment_batch_requests(operations.len() as u64).await;
        
        let ops = operations.clone();
        
        with_retry!({
            let client = self.pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.pool.clone()));
            
            match guard.as_mut().unwrap().paint_batch(ops.clone()).await {
                Ok(result) => Ok(result),
                Err(e) => {
                    if let PaintboardError::ConnectionClosed = e {
                        // 如果是连接关闭错误，标记连接为损坏
                        guard.mark_broken();
                    }
                    Err(e)
                }
            }
        })
    }
}

// 添加监控相关的监控数据结构和功能，如果需要的话
#[derive(Debug, Clone)]
pub struct PoolMetrics {
    // 连接池基础指标
    pub pool_size: usize,           // 池中连接数
    pub active_count: usize,        // 活跃连接数
    pub min_connections: usize,     // 最小连接数
    pub max_connections: usize,     // 最大连接数
    pub total_usage_count: u64,     // 总使用次数
    pub created_count: u64,         // 创建的连接数
    pub released_count: u64,        // 释放的连接数
    pub broken_count: u64,          // 损坏的连接数
    
    // 发包相关指标
    pub total_requests: u64,        // 总请求数
    pub total_packets: u64,         // 总发包数（包括粘包）
    pub batch_requests: u64,        // 批量请求次数
    
    // 错误相关指标
    pub total_errors: u64,          // 总错误数
    pub retry_count: u64,           // 重试次数
    pub retry_success_count: u64,   // 重试成功次数
    pub retry_failed_count: u64,    // 重试失败次数
    
    // 统计时间
    pub timestamp: std::time::SystemTime, // 统计时间戳
}

impl PoolMetrics {
    pub fn new() -> Self {
        Self {
            pool_size: 0,
            active_count: 0,
            min_connections: 0,
            max_connections: 0,
            total_usage_count: 0,
            created_count: 0,
            released_count: 0,
            broken_count: 0,
            total_requests: 0,
            total_packets: 0,
            batch_requests: 0,
            total_errors: 0,
            retry_count: 0,
            retry_success_count: 0,
            retry_failed_count: 0,
            timestamp: std::time::SystemTime::now(),
        }
    }
    
    // 计算错误率
    pub fn error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.total_errors as f64) / (self.total_requests as f64) * 100.0
        }
    }
    
    // 计算重试成功率
    pub fn retry_success_rate(&self) -> f64 {
        if self.retry_count == 0 {
            0.0
        } else {
            (self.retry_success_count as f64) / (self.retry_count as f64) * 100.0
        }
    }
}