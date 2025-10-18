use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, OwnedSemaphorePermit};
use tokio::time::timeout;

use crate::{config::Config, error::PaintboardError, BasicClient, PaintboardClientTrait};

// 连接池项，包含连接和元数据
struct PoolItem {
    client: BasicClient,
    last_used: Instant,
    usage_count: u64,
    is_broken: bool,
}

impl PoolItem {
    fn new(client: BasicClient) -> Self {
        Self {
            client,
            last_used: Instant::now(),
            usage_count: 0,
            is_broken: false,
        }
    }
}

use crate::client::connection_pool::pool_client::PoolMetrics;

// 连接守卫，利用 RAII 自动管理连接生命周期
pub struct ConnectionGuard {
    connection: Option<BasicClient>,
    pool: Arc<ConnectionPool>,
    _permit: OwnedSemaphorePermit,
    broken: bool,
}

impl ConnectionGuard {
    pub fn new(connection: BasicClient, pool: Arc<ConnectionPool>, permit: OwnedSemaphorePermit) -> Self {
        Self {
            connection: Some(connection),
            pool,
            _permit: permit,
            broken: false,
        }
    }

    pub fn mark_broken(&mut self) {
        self.broken = true;
    }

    pub fn as_mut(&mut self) -> Option<&mut BasicClient> {
        self.connection.as_mut()
    }

    // 执行操作并返回结果和是否损坏连接的指示
    pub async fn execute_with_retry<F, Fut, T>(
        &mut self,
        operation: F,
    ) -> Result<T, PaintboardError>
    where
        F: Fn(&mut BasicClient) -> Fut,
        Fut: std::future::Future<Output = Result<T, PaintboardError>>,
    {
        if let Some(client) = self.connection.as_mut() {
            operation(client).await
        } else {
            Err(PaintboardError::ConnectionClosed)
        }
    }
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        // 如果连接存在，则归还到池中
        if let Some(connection) = self.connection.take() {
            // 在后台任务中异步归还连接
            let pool = self.pool.clone();
            let broken = self.broken;
            tokio::spawn(async move {
                pool.release(connection, broken).await;
            });
        }
    }
}

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// 熔断器状态
#[derive(Debug, Clone, Copy, PartialEq)]
enum CircuitBreakerState {
    Closed,   // 正常状态，请求正常发送
    Open,     // 熔断开启，直接失败
    HalfOpen, // 半开启状态，尝试恢复
}

// 熔断器结构
struct CircuitBreaker {
    state: AtomicBool, // 使用简单的开/关状态，true表示开启（断开）
    failure_count: AtomicUsize,
    last_failure_time: std::sync::Mutex<std::time::Instant>,
    max_failures: usize,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    fn new(max_failures: usize, reset_timeout: Duration) -> Self {
        Self {
            state: AtomicBool::new(false),
            failure_count: AtomicUsize::new(0),
            last_failure_time: std::sync::Mutex::new(std::time::Instant::now()),
            max_failures,
            reset_timeout,
        }
    }

    fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;

        // 更新最后失败时间
        *self.last_failure_time.lock().unwrap() = std::time::Instant::now();

        // 如果失败次数超过阈值，打开熔断器
        if count >= self.max_failures {
            self.state.store(true, Ordering::SeqCst);
        }
    }

    fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        self.state.store(false, Ordering::SeqCst);
    }

    fn is_open(&self) -> bool {
        let is_open = self.state.load(Ordering::SeqCst);

        // 如果熔断器是开启的，检查是否已到重置时间
        if is_open {
            let last_failure = *self.last_failure_time.lock().unwrap();
            if last_failure.elapsed() > self.reset_timeout {
                // 时间到了，进入半开启状态（这里简化为尝试重置）
                self.state.store(false, Ordering::SeqCst);
                self.failure_count.store(0, Ordering::SeqCst);
                false // 返回false表示现在是关闭状态
            } else {
                true // 仍然开启
            }
        } else {
            false // 未开启
        }
    }
}

// 连接池管理器
#[derive(Clone)]
pub struct ConnectionPool {
    pub pool: Arc<Mutex<VecDeque<PoolItem>>>,
    pub semaphore: Arc<Semaphore>,
    pub min_connections: usize,
    pub max_connections: usize,
    pub config: Config,
    pub uid: Option<u32>,
    pub token: Option<String>,
    active_count: Arc<Mutex<usize>>,      // 当前活跃连接数
    pub metrics: Arc<Mutex<PoolMetrics>>, // 监控指标
    circuit_breaker: Arc<CircuitBreaker>, // 熔断器
}

impl ConnectionPool {
    pub fn new(config: Config, min_connections: usize, max_connections: usize) -> Self {
        let semaphore = Arc::new(Semaphore::new(max_connections));
        let pool = Arc::new(Mutex::new(VecDeque::new()));
        let active_count = Arc::new(Mutex::new(0));
        let metrics = Arc::new(Mutex::new(PoolMetrics::new()));
        let circuit_breaker = Arc::new(CircuitBreaker::new(
            5,                       // 最大失败次数
            Duration::from_secs(30), // 30秒后尝试重置
        ));

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
            circuit_breaker,
        }
    }

    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }

    // 获取一个连接，带有负载均衡
    pub async fn acquire(&self) -> Result<(BasicClient, OwnedSemaphorePermit), PaintboardError> {
        // 创建信号量许可以确保不超过最大连接数
        let permit = Arc::clone(&self.semaphore)
            .acquire_owned()
            .await
            .map_err(|_| PaintboardError::ConnectionClosed)?;

        // 尝试从池中获取一个连接
        let mut pool_guard = self.pool.lock().await;
        if let Some(mut pool_item) = pool_guard.pop_front() {
            // 检查连接是否已经失效
            if pool_item.is_broken {
                // 连接已损坏，丢弃并创建新连接
                drop(pool_guard);
                let client = self.create_new_connection().await?;
                return Ok((client, permit));
            }

            // 健康检查：在返回连接前验证其状态
            let mut client = pool_item.client;

            // 确保连接有认证信息
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                client.set_auth(uid, token.clone());
            }

            // 进行健康检查
            if !self.is_connection_healthy(&mut client).await {
                // 连接不健康，丢弃并创建新连接
                drop(pool_guard);
                let client = self.create_new_connection().await?;
                return Ok((client, permit));
            }

            // 更新连接使用情况
            pool_item.usage_count += 1;
            pool_item.last_used = Instant::now();

            // 增加活跃连接数和使用计数
            {
                *self.active_count.lock().await += 1;
            }
            self.increment_usage_count().await;

            drop(pool_guard);
            Ok((client, permit))
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

            Ok((client, permit))
        }
    }

    // 释放连接归还池中
    pub async fn release(&self, mut client: BasicClient, is_broken: bool) {
        // 减少活跃连接数
        {
            let mut active = self.active_count.lock().await;
            if *active > 0 {
                *active -= 1;
            }
        }

        // 增加释放连接计数
        self.increment_released_count(is_broken).await;

        // 只有在连接健康且池中连接数低于 min_connections 时才归还。
        // 损坏的连接始终丢弃，以节省资源并保证池健康。
        if !is_broken {
            let mut pool_guard = self.pool.lock().await;

            // 如果池中的连接数少于最小期望数量，则将连接归还到池中；否则丢弃连接
            if pool_guard.len() < self.min_connections {
                // 确保连接有认证信息
                if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                    client.set_auth(uid, token.clone());
                }

                let pool_item = PoolItem {
                    client,
                    last_used: Instant::now(),
                    usage_count: 0,
                    is_broken: false,
                };
                pool_guard.push_back(pool_item);
            }
            // 否则池已达到或超过 min_connections，丢弃额外连接以节省资源
        }
        // 损坏的连接直接丢弃
    }

    /// 创建新连接 (供后台管理器使用)
    pub async fn create_new_connection(&self) -> Result<BasicClient, PaintboardError> {
        let client = BasicClient::new(self.config.clone()).await?;
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
        // 首先获取需要移除的连接，避免长时间持有锁
        let to_remove_indices = {
            let pool_guard = self.pool.lock().await;
            let mut indices = Vec::new();
            for (i, item) in pool_guard.iter().enumerate() {
                if item.last_used.elapsed() > max_idle_time {
                    indices.push(i);
                }
            }
            indices
        };

        // 移除过期连接
        if !to_remove_indices.is_empty() {
            let mut pool_guard = self.pool.lock().await;
            // 从后往前删除，避免索引变化问题
            for &idx in to_remove_indices.iter().rev() {
                pool_guard.remove(idx);
            }

            log::info!("清理了 {} 个闲置连接", to_remove_indices.len());
        }
    }

    /// 将连接添加到池中 (用于后台管理器)
    pub async fn add_connection_to_pool(&self, client: BasicClient) {
        let mut pool_guard = self.pool.lock().await;
        if pool_guard.len() < self.max_connections {
            let pool_item = PoolItem {
                client,
                last_used: std::time::Instant::now(),
                usage_count: 0,
                is_broken: false,
            };
            pool_guard.push_back(pool_item);
            log::debug!("连接已添加到池中，当前池中连接数: {}", pool_guard.len());
        } else {
            log::debug!("池已满，无法添加更多连接");
        }
    }

    /// 获取池中的连接数
    pub async fn get_pool_stats(&self) -> (usize, usize) {
        let pool_size = self.pool.lock().await.len();
        let active_count = *self.active_count.lock().await;
        (pool_size, active_count)
    }

    // 内部更新监控数据的方法
    async fn update_metrics<F>(&self, update_fn: F)
    where
        F: FnOnce(&mut PoolMetrics),
    {
        let mut metrics = self.metrics.lock().await;
        update_fn(&mut metrics);
    }

    /// 使用连接执行操作，自动管理连接生命周期和重试逻辑
    pub async fn execute_with_connection<T, F>(&self, operation: F) -> Result<T, PaintboardError>
    where
        F: Clone, // 添加 Clone 约束
        F: FnOnce(&mut BasicClient) -> Result<T, PaintboardError>,
        T: Send,
    {
        // 检查熔断器状态
        if self.circuit_breaker.is_open() {
            return Err(PaintboardError::Internal(
                "Circuit breaker is open".to_string(),
            ));
        }

        let mut attempts = 0;
        let max_attempts = 3;

        loop {
            let (client, permit) = self.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.clone()), permit);

            match operation.clone()(guard.as_mut().unwrap()) {
                // 使用 clone() 来获取每次迭代的副本
                Ok(result) => {
                    // 操作成功，记录成功并返回结果
                    self.circuit_breaker.record_success();
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;

                    // 根据错误类型判断是否需要重试
                    if !self.should_retry_on_error(&e) {
                        self.circuit_breaker.record_failure();
                        return Err(e);
                    }

                    guard.mark_broken(); // 标记连接已损坏
                    if attempts >= max_attempts {
                        // 达到最大重试次数，记录失败
                        self.circuit_breaker.record_failure();
                        return Err(e);
                    }

                    // 实现指数退避延迟
                    let delay = std::time::Duration::from_millis((1 << attempts) * 100); // 100ms, 200ms, 400ms
                    tokio::time::sleep(delay).await;

                    // 作用域结束时连接会被自动归还（并标记为损坏）
                }
            }
        }
    }

    /// 使用异步操作执行连接，支持异步闭包
    pub async fn execute_with_connection_async<F, Fut, T>(
        &self,
        operation: F,
    ) -> Result<T, PaintboardError>
    where
        F: Clone, // 添加 Clone 约束以支持循环中的多次使用
        F: Fn(&mut BasicClient) -> Fut,
        Fut: std::future::Future<Output = Result<T, PaintboardError>> + Send + 'static,
        T: Send + 'static,
    {
        // 检查熔断器状态
        if self.circuit_breaker.is_open() {
            return Err(PaintboardError::Internal(
                "Circuit breaker is open".to_string(),
            ));
        }

        let mut attempts = 0;
        let max_attempts = 3;

        loop {
            let (client, permit) = self.acquire().await?;
            let mut guard = ConnectionGuard::new(client, Arc::new(self.clone()), permit);

            match operation(guard.as_mut().unwrap()).await {
                Ok(result) => {
                    // 操作成功，连接保持完好，通过 Drop 自动归还
                    return Ok(result);
                }
                Err(e) => {
                    attempts += 1;

                    // 根据错误类型判断是否需要重试
                    if !self.should_retry_on_error(&e) {
                        return Err(e);
                    }

                    guard.mark_broken(); // 标记连接已损坏
                    if attempts >= max_attempts {
                        return Err(e);
                    }

                    // 实现指数退避延迟
                    let delay = std::time::Duration::from_millis((1 << attempts) * 100); // 100ms, 200ms, 400ms
                    tokio::time::sleep(delay).await;

                    // 作用域结束时连接会被自动归还（并标记为损坏）
                }
            }
        }
    }

    // 增加请求计数
    pub async fn increment_requests(&self, packet_count: u64) {
        self.update_metrics(|m| {
            m.total_requests += 1;
            m.total_packets += packet_count;
        })
        .await;
    }

    // 增加批量请求计数
    pub async fn increment_batch_requests(&self, operations_count: u64) {
        self.update_metrics(|m| {
            m.batch_requests += 1;
            m.total_packets += operations_count;
        })
        .await;
    }

    // 增加错误计数
    pub async fn increment_errors(&self) {
        self.update_metrics(|m| {
            m.total_errors += 1;
        })
        .await;
    }

    /// 判断是否应该对特定错误进行重试
    fn should_retry_on_error(&self, error: &PaintboardError) -> bool {
        match error {
            // 这些错误类型表明连接或网络问题，可以重试
            PaintboardError::Network(_)
            | PaintboardError::WebSocket(_)
            | PaintboardError::ConnectionClosed
            | PaintboardError::Timeout
            | PaintboardError::ResponseChannelClosed => {
                self.increment_error_type_counter_sync("transient"); // 暂时性错误
                true
            }
            // 这些错误是客户端错误，不应该重试
            PaintboardError::Http(status) if *status >= 400 && *status < 500 => {
                self.increment_error_type_counter_sync(&format!("http_{}", status)); // 客户端错误
                false
            }
            // 服务器错误可以重试
            PaintboardError::Http(status) if *status >= 500 => {
                self.increment_error_type_counter_sync(&format!("http_{}", status)); // 服务器错误
                true
            }
            // 认证错误表示配置问题，不应该重试
            PaintboardError::Auth(_) => {
                self.increment_error_type_counter_sync("auth"); // 认证错误
                false
            }
            // 其他错误类型可能需要重试
            _ => {
                self.increment_error_type_counter_sync("other");
                true
            }
        }
    }

    /// 检查连接是否健康
    pub async fn is_connection_healthy(&self, client: &mut BasicClient) -> bool {
        // 5秒超时，对于健康检查来说足够了
        match timeout(Duration::from_secs(5), client.get_board()).await {
            Ok(Ok(_)) => {
                // 成功获取画板数据，连接健康
                true
            }
            Ok(Err(e)) => {
                // 请求返回错误，根据错误类型判断
                match e {
                    PaintboardError::Network(_)
                    | PaintboardError::WebSocket(_)
                    | PaintboardError::ConnectionClosed
                    | PaintboardError::Timeout => {
                        log::debug!("连接不健康: {:?}", e);
                        false
                    }
                    _ => {
                        // 其他错误不一定表示连接问题
                        true
                    }
                }
            }
            Err(_) => {
                // 超时，连接不健康
                log::debug!("健康检查超时，连接被视为不健康");
                false
            }
        }
    }

    /// 根据错误原因返回错误类别
    fn categorize_error(&self, error: &PaintboardError) -> &'static str {
        match error {
            PaintboardError::Network(_) => "network",
            PaintboardError::WebSocket(_) => "websocket",
            PaintboardError::ConnectionClosed => "connection_closed",
            PaintboardError::Timeout => "timeout",
            PaintboardError::ResponseChannelClosed => "response_channel_closed",
            PaintboardError::Http(status) if *status >= 400 && *status < 500 => "client_error",
            PaintboardError::Http(status) if *status >= 500 => "server_error",
            PaintboardError::Auth(_) => "authentication",
            PaintboardError::InvalidCoordinate { .. } => "invalid_coordinate",
            PaintboardError::IndexOutOfRange { .. } => "index_out_of_range",
            PaintboardError::InvalidData(_) => "invalid_data",
            PaintboardError::InvalidUrl(_) => "invalid_url",
            PaintboardError::ClientNotInitialized => "client_not_initialized",
            PaintboardError::RateLimit => "rate_limit",
            PaintboardError::Internal(_) => "internal",
            _ => "unknown",
        }
    }

    // 增加特定错误类型的计数（同步版本）
    pub fn increment_error_type_counter_sync(&self, error_type: &str) {
        let pool = self.clone();
        let error_type = error_type.to_string(); // 转换为拥有所有权的字符串
        tokio::spawn(async move {
            pool.update_metrics(|m| {
                match error_type.as_str() {
                    "network" | "transient" => m.network_errors += 1, // 同时处理网络和暂时性错误
                    "auth" => m.auth_errors += 1,
                    "http" => m.http_errors += 1, // 只处理http前缀的错误
                    "other" => m.other_errors += 1,
                    s if s.starts_with("http_") => {
                        if s.contains("4") && !s.contains("5") {
                            // 客户端错误
                            m.client_errors += 1;
                        } else if s.contains("5") {
                            // 服务器错误
                            m.server_errors += 1;
                        } else {
                            m.http_errors += 1; // 其他HTTP错误
                        }
                    }
                    _ => m.other_errors += 1,
                }
            })
            .await;
        });
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
        })
        .await;
    }

    // 增加创建连接计数
    async fn increment_created_count(&self) {
        self.update_metrics(|m| {
            m.created_count += 1;
        })
        .await;
    }

    // 增加释放连接计数
    async fn increment_released_count(&self, is_broken: bool) {
        self.update_metrics(|m| {
            m.released_count += 1;
            if is_broken {
                m.broken_count += 1;
            }
        })
        .await;
    }

    // 增加使用计数
    async fn increment_usage_count(&self) {
        self.update_metrics(|m| {
            m.total_usage_count += 1;
        })
        .await;
    }
}

// 通用的故障转移函数
async fn fault_tolerance<T, F, Fut>(
    pool: &ConnectionPool,
    operation: F,
) -> Result<T, PaintboardError>
where
    F: Fn(BasicClient) -> Fut,
    Fut: std::future::Future<Output = Result<T, PaintboardError>> + Send,
{
    // 尝试最多3次
    let mut attempts = 0;
    let max_attempts = 3;

    loop {
        let (client, _permit) = match pool.acquire().await {
            Ok((client, permit)) => (client, permit),
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

// 定义一个重试宏，只处理重试逻辑，不管理连接
// 用法: with_retry!(operation_code)
// operation_code 应该是一个表达式或闭包，会重试最多3次
#[macro_export]
macro_rules! with_retry {
    ($operation:expr) => {{
        use crate::error::PaintboardError;

        let mut attempts = 0;
        let max_attempts = 3;

        loop {
            match $operation {
                Ok(result) => return Ok(result),
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_attempts {
                        return Err(e);
                    }
                    // 继续下一次尝试
                }
            }
        }
    }};
}
