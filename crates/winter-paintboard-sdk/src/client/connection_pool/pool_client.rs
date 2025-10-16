use std::sync::Arc;
use tokio::sync::Mutex;
use std::time::Duration;
use async_trait::async_trait;

use crate::{
    error::PaintboardError, 
    models::{Board, Rgb, Pos, PaintResult}, 
    config::{Config, ConnectionMode},
    BasicClient,
    PaintboardClientTrait,
    event::{EventBus, Event},
};

use super::pool::{ConnectionPool, ConnectionGuard};

// PoolClient 实现
pub struct PoolClient {
    pub write_pool: ConnectionPool,          // 专门用于写操作的连接池
    read_client: Arc<Mutex<Option<BasicClient>>>, // 专门用于读取事件的只读连接
    event_bus: EventBus, // 事件总线
}

use crate::client::connection_pool::manager::start_connection_manager_task;

impl PoolClient {
    pub async fn new(config: Config, min_connections: usize, max_connections: usize) -> Result<Self, PaintboardError> {
        // 创建写连接池配置（使用WriteOnly模式）
        let mut write_config = config.clone();
        write_config.connection_mode = ConnectionMode::WriteOnly;
        let write_pool = ConnectionPool::new(write_config, min_connections, max_connections);
        
        // 创建读连接配置（使用ReadOnly模式）
        let mut read_config = config.clone();
        read_config.connection_mode = ConnectionMode::ReadOnly;
        
        let pool_client = Self {
            write_pool,
            read_client: Arc::new(Mutex::new(None)),
            event_bus: EventBus::global(), // 使用全局事件总线
        };
        
        // 启动后台连接管理器（只管理写连接池）
        start_connection_manager_task(pool_client.write_pool.clone(), None).await;
        
        // 初始化只读连接
        pool_client.setup_read_only_connection().await?;
        
        Ok(pool_client)
    }
    
    // 初始化只读连接
    async fn setup_read_only_connection(&self) -> Result<(), PaintboardError> {
        // 创建只读配置
        let mut read_config = self.write_pool.config.clone();
        read_config.connection_mode = ConnectionMode::ReadOnly;
        
        let mut read_client = BasicClient::new(read_config).await?;
        if let (Some(uid), Some(token)) = (self.write_pool.uid, self.write_pool.token.as_ref()) {
            read_client.set_auth(uid, token.clone());
        }
        
        {
            let mut client_guard = self.read_client.lock().await;
            *client_guard = Some(read_client);
        }
        
        // 确保只读客户端连接到WebSocket以开始接收事件
        self.connect_read_only_client().await?;
        
        // 启动事件监听
        self.start_event_listener().await?;
        
        Ok(())
    }
    
    // 连接只读客户端到WebSocket
    async fn connect_read_only_client(&self) -> Result<(), PaintboardError> {
        if let Some(ref mut client) = *self.read_client.lock().await {
            // 调用get_board以触发WebSocket连接的初始化
            let _ = client.get_board().await;
        }
        Ok(())
    }
    
    // 启动事件监听任务
    pub async fn start_event_listener(&self) -> Result<(), PaintboardError> {
        // 由于BasicClient内部的WsProvider已经通过EventBus自动分发事件
        // 这里只需要确保只读客户端已连接，事件会自动通过EventBus传播
        // BasicClient内部会自动将事件转发到EventBus，所以我们不需要额外的监听循环
        
        Ok(())
    }
    
    // 健康检查只读连接
    pub async fn check_read_client_health(&self) -> bool {
        if let Some(ref client) = *self.read_client.lock().await {
            // 检查连接是否仍然有效（通过调用get_board检查连接状态）
            client.get_board().await.is_ok()
        } else {
            false
        }
    }
    
    // 重新连接只读连接
    pub async fn reconnect_read_client(&self) -> Result<(), PaintboardError> {
        // 创建只读配置
        let mut read_config = self.write_pool.config.clone();
        read_config.connection_mode = ConnectionMode::ReadOnly;
        
        let mut read_client = BasicClient::new(read_config).await?;
        if let (Some(uid), Some(token)) = (self.write_pool.uid, self.write_pool.token.as_ref()) {
            read_client.set_auth(uid, token.clone());
        }
        
        {
            let mut client_guard = self.read_client.lock().await;
            *client_guard = Some(read_client);
        }
        
        // 确保只读客户端连接到WebSocket
        self.connect_read_only_client().await?;
        
        Ok(())
    }
    
    // 获取只读连接状态
    pub async fn get_read_client_status(&self) -> String {
        if self.read_client.lock().await.is_some() {
            "Connected".to_string()
        } else {
            "Disconnected".to_string()
        }
    }
    
    // 获取写连接池状态
    pub async fn get_write_pool_status(&self) -> String {
        format!(
            "PoolSize: {}, ActiveConnections: {}", 
            self.write_pool.pool_size().await,
            self.write_pool.active_count().await
        )
    }
    
    // 为连接池中的所有连接设置认证信息
    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.write_pool.set_auth(uid, token.clone());
        // 也需要为只读连接设置认证，使用spawn来处理异步操作
        let read_client = Arc::clone(&self.read_client);
        let token_clone = token.clone();
        tokio::spawn(async move {
            if let Some(ref mut client) = *read_client.lock().await {
                client.set_auth(uid, token_clone);
            }
        });
    }
    
    // 获取池大小
    pub async fn pool_size(&self) -> usize {
        self.write_pool.pool_size().await
    }
    
    // 获取活跃连接数
    pub async fn active_count(&self) -> usize {
        self.write_pool.active_count().await
    }
    
    // 清理不活跃连接
    pub async fn cleanup_inactive_connections(&self, max_idle_time: Duration) {
        self.write_pool.cleanup_inactive_connections(max_idle_time).await;
    }
    
    // 为事件监听创建专用连接
    pub async fn setup_event_client(&mut self) -> Result<(), PaintboardError> {
        // 创建只读配置
        let mut read_config = self.write_pool.config.clone();
        read_config.connection_mode = ConnectionMode::ReadOnly;
        
        let mut read_client = BasicClient::new(read_config).await?;
        if let (Some(uid), Some(token)) = (self.write_pool.uid, self.write_pool.token.as_ref()) {
            read_client.set_auth(uid, token.clone());
        }
        *self.read_client.lock().await = Some(read_client);
        
        // 通过读客户端建立 WebSocket 连接以开始接收事件
        // 实际上，当首次调用 paint 或 paint_batch 时会自动初始化 WebSocket 连接
        // 为了事件监听，我们可以让事件客户端连接到 WebSocket，但这通常是在执行操作时触发的
        
        Ok(())
    }
    
    // 强制事件客户端连接到 WebSocket 以开始监听
    pub async fn connect_event_client(&self) -> Result<(), PaintboardError> {
        {
            let mut client_guard = self.read_client.lock().await;
            if let Some(client) = client_guard.as_mut() {
                // 调用一个操作来触发 WebSocket 连接的初始化
                // 但我们会使用一个不会实际发送数据的虚拟操作
                // 注意：这里使用 get_board 可能不会触发 WebSocket 初始化
                // 更好的方式是尝试直接初始化 WebSocket 连接
                let _ = client.get_board().await; // 这将确保 WebSocket 客户端被初始化
            }
        }
        Ok(())
    }
    
    // 内部方法：执行带故障转移的操作
    // 现在使用 ConnectionPool 的 execute_with_connection 方法，不再需要这个
    
    // 获取事件总线实例
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }
    
    // 订阅事件
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.event_bus.subscribe()
    }
    
    // 开始监听事件（需要先 setup_event_client）
    pub async fn start_listening_events(&self) -> Result<(), PaintboardError> {
        // 事件监听是通过全局 EventBus 自动处理的，不需要特殊启动
        // 只要 BasicClient 连接了 WebSocket，事件就会自动发送到 EventBus
        Ok(())
    }
}

#[async_trait]
impl PaintboardClientTrait for PoolClient {
    async fn new(config: Config) -> Result<Self, PaintboardError> 
    where 
        Self: Sized 
    {
        // 默认使用最小4个连接，最大7个连接
        Self::new(config, 2, 7).await
    }

    fn set_auth(&mut self, uid: u32, token: String) {
        self.write_pool.set_auth(uid, token);
    }

    async fn get_board(&self) -> Result<Board, PaintboardError> {
        // 对于get_board操作，我们可以使用只读连接
        // 但为了保持与现有逻辑一致，仍使用连接池
        
        // 增加请求计数
        self.write_pool.increment_requests(1).await;
        
        // 改进的重试逻辑：包含错误分类和指数退避
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let client = self.write_pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.write_pool.clone()));
            
            match guard.as_mut().unwrap().get_board().await {
                Ok(result) => {
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    
                    // 根据错误类型判断是否需要重试
                    match &e {
                        PaintboardError::Network(_) |
                        PaintboardError::WebSocket(_) |
                        PaintboardError::ConnectionClosed |
                        PaintboardError::Timeout |
                        PaintboardError::ResponseChannelClosed => {
                            self.write_pool.increment_error_type_counter_sync("network");
                            guard.mark_broken();
                        },
                        PaintboardError::Http(status) if *status >= 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 对于服务器错误，可能连接仍是好的，不标记损坏
                        },
                        PaintboardError::Auth(_) => {
                            self.write_pool.increment_error_type_counter_sync("auth");
                            // 认证错误不重试，直接返回
                            return Err(e);
                        },
                        PaintboardError::Http(status) if *status >= 400 && *status < 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 客户端错误不重试，直接返回
                            return Err(e);
                        },
                        _ => {
                            self.write_pool.increment_error_type_counter_sync("other");
                            guard.mark_broken();
                        }
                    }
                    
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    
                    // 实现指数退避延迟
                    let delay = std::time::Duration::from_millis((1 << attempts) * 100); // 100ms, 200ms, 400ms
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        // 增加请求计数
        self.write_pool.increment_requests(1).await;
        
        let access_key = access_key.to_string();
        
        // 改进的重试逻辑：包含错误分类和指数退避
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let client = self.write_pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.write_pool.clone()));
            
            match guard.as_mut().unwrap().get_token(uid, &access_key).await {
                Ok(result) => {
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    
                    // 根据错误类型判断是否需要重试
                    match &e {
                        PaintboardError::Network(_) |
                        PaintboardError::WebSocket(_) |
                        PaintboardError::ConnectionClosed |
                        PaintboardError::Timeout |
                        PaintboardError::ResponseChannelClosed => {
                            self.write_pool.increment_error_type_counter_sync("network");
                            guard.mark_broken();
                        },
                        PaintboardError::Http(status) if *status >= 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 对于服务器错误，可能连接仍是好的，不标记损坏
                        },
                        PaintboardError::Auth(_) => {
                            self.write_pool.increment_error_type_counter_sync("auth");
                            // 认证错误不重试，直接返回
                            return Err(e);
                        },
                        PaintboardError::Http(status) if *status >= 400 && *status < 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 客户端错误不重试，直接返回
                            return Err(e);
                        },
                        _ => {
                            self.write_pool.increment_error_type_counter_sync("other");
                            guard.mark_broken();
                        }
                    }
                    
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    
                    // 实现指数退避延迟
                    let delay = std::time::Duration::from_millis((1 << attempts) * 100); // 100ms, 200ms, 400ms
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        // 增加请求计数
        self.write_pool.increment_requests(1).await;
        
        // 改进的重试逻辑：包含错误分类和指数退避
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let client = self.write_pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.write_pool.clone()));
            
            match guard.as_mut().unwrap().paint(pos, color).await {
                Ok(result) => {
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    
                    // 根据错误类型判断是否需要重试
                    match &e {
                        PaintboardError::Network(_) |
                        PaintboardError::WebSocket(_) |
                        PaintboardError::ConnectionClosed |
                        PaintboardError::Timeout |
                        PaintboardError::ResponseChannelClosed => {
                            self.write_pool.increment_error_type_counter_sync("network");
                            guard.mark_broken();
                        },
                        PaintboardError::Http(status) if *status >= 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 对于服务器错误，可能连接仍是好的，不标记损坏
                        },
                        PaintboardError::Auth(_) => {
                            self.write_pool.increment_error_type_counter_sync("auth");
                            // 认证错误不重试，直接返回
                            return Err(e);
                        },
                        PaintboardError::Http(status) if *status >= 400 && *status < 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 客户端错误不重试，直接返回
                            return Err(e);
                        },
                        _ => {
                            self.write_pool.increment_error_type_counter_sync("other");
                            guard.mark_broken();
                        }
                    }
                    
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    
                    // 实现指数退避延迟
                    let delay = std::time::Duration::from_millis((1 << attempts) * 100); // 100ms, 200ms, 400ms
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }

    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        // 增加批量请求计数
        self.write_pool.increment_batch_requests(operations.len() as u64).await;
        
        // 改进的重试逻辑：包含错误分类和指数退避
        let mut attempts = 0;
        let max_attempts = 3;
        
        loop {
            let client = self.write_pool.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.write_pool.clone()));
            
            match guard.as_mut().unwrap().paint_batch(operations.clone()).await {
                Ok(result) => {
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;
                    
                    // 根据错误类型判断是否需要重试
                    match &e {
                        PaintboardError::Network(_) |
                        PaintboardError::WebSocket(_) |
                        PaintboardError::ConnectionClosed |
                        PaintboardError::Timeout |
                        PaintboardError::ResponseChannelClosed => {
                            self.write_pool.increment_error_type_counter_sync("network");
                            guard.mark_broken();
                        },
                        PaintboardError::Http(status) if *status >= 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 对于服务器错误，可能连接仍是好的，不标记损坏
                        },
                        PaintboardError::Auth(_) => {
                            self.write_pool.increment_error_type_counter_sync("auth");
                            // 认证错误不重试，直接返回
                            return Err(e);
                        },
                        PaintboardError::Http(status) if *status >= 400 && *status < 500 => {
                            self.write_pool.increment_error_type_counter_sync("http");
                            // 客户端错误不重试，直接返回
                            return Err(e);
                        },
                        _ => {
                            self.write_pool.increment_error_type_counter_sync("other");
                            guard.mark_broken();
                        }
                    }
                    
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    
                    // 实现指数退避延迟
                    let delay = std::time::Duration::from_millis((1 << attempts) * 100); // 100ms, 200ms, 400ms
                    tokio::time::sleep(delay).await;
                }
            }
        }
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
    pub network_errors: u64,        // 网络错误数
    pub http_errors: u64,           // HTTP错误数
    pub auth_errors: u64,           // 认证错误数
    pub other_errors: u64,          // 其他错误数
    pub circuit_breaker_tripped: u64, // 熔断器触发次数
    pub transient_errors: u64,      // 暂时性错误数
    pub client_errors: u64,         // 客户端错误数
    pub server_errors: u64,         // 服务器错误数
    
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
            network_errors: 0,
            http_errors: 0,
            auth_errors: 0,
            other_errors: 0,
            circuit_breaker_tripped: 0,
            transient_errors: 0,
            client_errors: 0,
            server_errors: 0,
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
    
    // 计算网络错误率
    pub fn network_error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.network_errors as f64) / (self.total_requests as f64) * 100.0
        }
    }
    
    // 计算认证错误率
    pub fn auth_error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.auth_errors as f64) / (self.total_requests as f64) * 100.0
        }
    }
    
    // 计算暂时性错误率
    pub fn transient_error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.transient_errors as f64) / (self.total_requests as f64) * 100.0
        }
    }
    
    // 计算客户端错误率
    pub fn client_error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.client_errors as f64) / (self.total_requests as f64) * 100.0
        }
    }
    
    // 计算服务器错误率
    pub fn server_error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            (self.server_errors as f64) / (self.total_requests as f64) * 100.0
        }
    }
}