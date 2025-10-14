use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use std::collections::VecDeque;
use std::time::{Duration, Instant};
use async_trait::async_trait;

use crate::{
    error::PaintboardError, 
    models::{Board, Rgb, Pos, PaintResult}, 
    config::Config,
    PaintboardClient,
    PaintboardClientTrait,
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

// 连接池管理器
pub struct ConnectionPool {
    pool: Arc<Mutex<VecDeque<PoolItem>>>,
    semaphore: Arc<Semaphore>,
    min_connections: usize,
    max_connections: usize,
    config: Config,
    uid: Option<u32>,
    token: Option<String>,
    active_count: Arc<Mutex<usize>>, // 当前活跃连接数
}

impl ConnectionPool {
    pub fn new(config: Config, min_connections: usize, max_connections: usize) -> Self {
        let semaphore = Arc::new(Semaphore::new(max_connections));
        let pool = Arc::new(Mutex::new(VecDeque::new()));
        let active_count = Arc::new(Mutex::new(0));
        
        Self {
            pool,
            semaphore,
            min_connections,
            max_connections,
            config,
            uid: None,
            token: None,
            active_count,
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
            
            // 增加活跃连接数
            *self.active_count.lock().await += 1;
            
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
            
            // 增加活跃连接数
            *self.active_count.lock().await += 1;
            
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

// ConnectionPoolClient 实现
pub struct ConnectionPoolClient {
    pool: ConnectionPool,
    event_client: Arc<Mutex<Option<PaintboardClient>>>, // 专门用于事件监听的连接
}

impl ConnectionPoolClient {
    pub async fn new(config: Config, min_connections: usize, max_connections: usize) -> Result<Self, PaintboardError> {
        let pool = ConnectionPool::new(config, min_connections, max_connections);
        
        Ok(Self {
            pool,
            event_client: Arc::new(Mutex::new(None)),
        })
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
        let mut event_client = PaintboardClient::new(self.pool.config.clone()).await?;
        if let (Some(uid), Some(token)) = (self.pool.uid, self.pool.token.as_ref()) {
            event_client.set_auth(uid, token.clone());
        }
        *self.event_client.lock().await = Some(event_client);
        Ok(())
    }
    
    // 内部方法：执行带故障转移的操作
    async fn execute_with_fault_tolerance<T, F, Fut>(&self, operation: F) -> Result<T, PaintboardError>
    where
        F: Fn(&mut PaintboardClient) -> Fut,
        Fut: std::future::Future<Output = Result<T, PaintboardError>>,
    {
        // 尝试最多3次
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let mut client = match self.pool.acquire().await {
                Ok(client) => client,
                Err(e) => return Err(e),
            };
            
            match operation(&mut client).await {
                Ok(result) => {
                    // 操作成功，归还连接
                    self.pool.release(client, false).await;
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        // 已达到最大重试次数，归还损坏的连接
                        self.pool.release(client, true).await;
                        return Err(e);
                    }
                    
                    // 连接可能已损坏，归还并标记为损坏
                    self.pool.release(client, true).await;
                    // 继续下一次尝试
                }
            }
        }
    }
}

#[async_trait]
impl PaintboardClientTrait for ConnectionPoolClient {
    async fn new(config: Config) -> Result<Self, PaintboardError> 
    where 
        Self: Sized 
    {
        // 默认使用最小2个连接，最大7个连接
        Self::new(config, 2, 7).await
    }

    fn set_auth(&mut self, uid: u32, token: String) {
        self.pool.set_auth(uid, token);
    }

    async fn get_board(&self) -> Result<Board, PaintboardError> {
        // 尝试最多3次
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let mut client = match self.pool.acquire().await {
                Ok(client) => client,
                Err(e) => return Err(e),
            };
            
            match client.get_board().await {
                Ok(result) => {
                    // 操作成功，归还连接
                    self.pool.release(client, false).await;
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        // 已达到最大重试次数，归还损坏的连接
                        self.pool.release(client, true).await;
                        return Err(e);
                    }
                    
                    // 连接可能已损坏，归还并标记为损坏
                    self.pool.release(client, true).await;
                    // 继续下一次尝试
                }
            }
        }
    }

    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        let access_key = access_key.to_string(); // 创建一个拥有所有权的字符串
        // 尝试最多3次
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let mut client = match self.pool.acquire().await {
                Ok(client) => client,
                Err(e) => return Err(e),
            };
            
            match client.get_token(uid, &access_key).await {
                Ok(result) => {
                    // 操作成功，归还连接
                    self.pool.release(client, false).await;
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        // 已达到最大重试次数，归还损坏的连接
                        self.pool.release(client, true).await;
                        return Err(e);
                    }
                    
                    // 连接可能已损坏，归还并标记为损坏
                    self.pool.release(client, true).await;
                    // 继续下一次尝试
                }
            }
        }
    }

    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        // 尝试最多3次
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let mut client = match self.pool.acquire().await {
                Ok(client) => client,
                Err(e) => return Err(e),
            };
            
            match client.paint(pos, color).await {
                Ok(result) => {
                    // 操作成功，归还连接
                    self.pool.release(client, false).await;
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        // 已达到最大重试次数，归还损坏的连接
                        self.pool.release(client, true).await;
                        return Err(e);
                    }
                    
                    // 连接可能已损坏，归还并标记为损坏
                    self.pool.release(client, true).await;
                    // 继续下一次尝试
                }
            }
        }
    }

    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        // 尝试最多3次
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let mut client = match self.pool.acquire().await {
                Ok(client) => client,
                Err(e) => return Err(e),
            };
            
            match client.paint_batch(operations.clone()).await {
                Ok(result) => {
                    // 操作成功，归还连接
                    self.pool.release(client, false).await;
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        // 已达到最大重试次数，归还损坏的连接
                        self.pool.release(client, true).await;
                        return Err(e);
                    }
                    
                    // 连接可能已损坏，归还并标记为损坏
                    self.pool.release(client, true).await;
                    // 继续下一次尝试
                }
            }
        }
    }
}