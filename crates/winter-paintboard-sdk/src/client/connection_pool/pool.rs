use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::{
    PaintboardClientTrait,

    error::PaintboardError, 
    models::{Rgb, Pos, PaintResult}, 
    config::Config,
    PaintboardClient,
};

// 连接池项，包含连接和元数据
struct PoolItem {
    client: PaintboardClient,
    last_used: Instant,
    usage_count: u64,
    is_broken: bool,
}

impl PoolItem {
    fn new(client: PaintboardClient) -> Self {
        Self {
            client,
            last_used: Instant::now(),
            usage_count: 0,
            is_broken: false,
        }
    }
}

use crate::client::connection_pool::pool_client::PoolMetrics;

// 连接池管理器
pub struct ConnectionPool {
    pub pool: Arc<Mutex<VecDeque<PoolItem>>>,
    pub semaphore: Arc<Semaphore>,
    pub min_connections: usize,
    pub max_connections: usize,
    pub config: Config,
    pub uid: Option<u32>,
    pub token: Option<String>,
    active_count: Arc<Mutex<usize>>, // 当前活跃连接数
    pub metrics: Arc<Mutex<PoolMetrics>>, // 监控指标
}

impl ConnectionPool {
    pub fn new(config: Config, min_connections: usize, max_connections: usize) -> Self {
        let semaphore = Arc::new(Semaphore::new(max_connections));
        let pool = Arc::new(Mutex::new(VecDeque::new()));
        let active_count = Arc::new(Mutex::new(0));
        let metrics = Arc::new(Mutex::new(PoolMetrics::new()));
        
        Self {
            pool,
            semaphore,
            min_connections,
            max_connections,
            config,
            uid: None,
            token: None,
            active_count,
            metrics,
        }
    }
    
    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }
    
    // 获取一个连接，带有负载均衡
    pub async fn acquire(&self) -> Result<PaintboardClient, PaintboardError> {
        // 创建信号量许可以确保不超过最大连接数
        let _permit = self.semaphore.acquire().await
            .map_err(|_| PaintboardError::ConnectionClosed)?;
        
        // 尝试从池中获取一个连接
        let mut pool_guard = self.pool.lock().await;
        if let Some(mut pool_item) = pool_guard.pop_front() {
            // 检查连接是否已经失效
            if pool_item.is_broken {
                // 连接已损坏，丢弃并创建新连接
                drop(pool_guard);
                return self.create_new_connection().await;
            }
            
            // 更新连接使用情况
            pool_item.usage_count += 1;
            pool_item.last_used = Instant::now();
            let mut client = pool_item.client;
            
            // 确保连接有认证信息
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                client.set_auth(uid, token.clone());
            }
            
            // 增加活跃连接数和使用计数
            {
                *self.active_count.lock().await += 1;
            }
            self.increment_usage_count().await;
            
            drop(pool_guard);
            Ok(client)
        } else {
            drop(pool_guard);
            // 如果池中没有连接，创建新连接
            let mut client = self.create_new_connection().await?;
            
            // 确保新连接有认证信息
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                client.set_auth(uid, token.clone());
            }
            
            // 增加活跃连接数和连接创建计数
            {
                *self.active_count.lock().await += 1;
            }
            self.increment_created_count().await;
            
            Ok(client)
        }
    }
    
    // 释放连接归还池中
    pub async fn release(&self, mut client: PaintboardClient, is_broken: bool) {
        // 减少活跃连接数
        {
            let mut active = self.active_count.lock().await;
            if *active > 0 {
                *active -= 1;
            }
        }
        
        // 增加释放连接计数
        self.increment_released_count(is_broken).await;
        
        // 释放信号量许可
        drop(self.semaphore.acquire().await);
        
        let mut pool_guard = self.pool.lock().await;
        if pool_guard.len() < self.max_connections && !is_broken {
            // 确保连接有认证信息
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                client.set_auth(uid, token.clone());
            }
            
            let pool_item = PoolItem {
                client,
                last_used: Instant::now(),
                usage_count: 0, // 归还时重置使用次数以便重新计算
                is_broken: false,
            };
            pool_guard.push_back(pool_item);
        }
        // 否则丢弃连接
    }
    
    // 创建新连接
    async fn create_new_connection(&self) -> Result<PaintboardClient, PaintboardError> {
        let client = PaintboardClient::new(self.config.clone()).await?;
        Ok(client)
    }
    
    // 获取当前池中的连接数
    pub async fn pool_size(&self) -> usize {
        let pool_guard = self.pool.lock().await;
        pool_guard.len()
    }
    
    // 获取活跃连接数
    pub async fn active_count(&self) -> usize {
        let active = self.active_count.lock().await;
        *active
    }
    
    // 定期清理不活跃连接
    pub async fn cleanup_inactive_connections(&self, max_idle_time: Duration) {
        let mut pool_guard = self.pool.lock().await;
        pool_guard.retain(|item| {
            item.last_used.elapsed() <= max_idle_time
        });
    }
    
    // 内部更新监控数据的方法
    async fn update_metrics<F>(&self, update_fn: F) 
    where 
        F: FnOnce(&mut PoolMetrics)
    {
        let mut metrics = self.metrics.lock().await;
        update_fn(&mut metrics);
    }
    
    // 增加请求计数
    pub async fn increment_requests(&self, packet_count: u64) {
        self.update_metrics(|m| {
            m.total_requests += 1;
            m.total_packets += packet_count;
        }).await;
    }
    
    // 增加批量请求计数
    pub async fn increment_batch_requests(&self, operations_count: u64) {
        self.update_metrics(|m| {
            m.batch_requests += 1;
            m.total_packets += operations_count;
        }).await;
    }
    
    // 增加错误计数
    pub async fn increment_errors(&self) {
        self.update_metrics(|m| {
            m.total_errors += 1;
        }).await;
    }
    
    // 增加重试计数
    pub async fn increment_retry(&self, success: bool) {
        self.update_metrics(|m| {
            m.retry_count += 1;
            if success {
                m.retry_success_count += 1;
            } else {
                m.retry_failed_count += 1;
            }
        }).await;
    }
    
    // 增加创建连接计数
    async fn increment_created_count(&self) {
        self.update_metrics(|m| {
            m.created_count += 1;
        }).await;
    }
    
    // 增加释放连接计数
    async fn increment_released_count(&self, is_broken: bool) {
        self.update_metrics(|m| {
            m.released_count += 1;
            if is_broken {
                m.broken_count += 1;
            }
        }).await;
    }
    
    // 增加使用计数
    async fn increment_usage_count(&self) {
        self.update_metrics(|m| {
            m.total_usage_count += 1;
        }).await;
    }
}

// 通用的故障转移函数
async fn fault_tolerance<T, F, Fut>(
    pool: &ConnectionPool,
    operation: F,
) -> Result<T, PaintboardError>
where
    F: Fn(PaintboardClient) -> Fut,
    Fut: std::future::Future<Output = Result<T, PaintboardError>> + Send,
{
    // 尝试最多3次
    let mut attempts = 0;
    let max_attempts = 3;
    
    loop {
        let client = match pool.acquire().await {
            Ok(client) => client,
            Err(e) => return Err(e),
        };
        
        match operation(client).await {
            Ok(result) => {
                return Ok(result);
            }
            Err(e) => {
                attempts += 1;
                if attempts >= max_attempts {
                    return Err(e);
                }
                // 继续下一次尝试
            }
        }
    }
}