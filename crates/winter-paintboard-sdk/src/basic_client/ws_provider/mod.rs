mod ws_connection;
mod ws_message_handler;
mod ws_rate_limiter;
mod ws_reconnect;
mod ws_response_tracker;

use crate::basic_client::ws_provider::ws_connection::WsConnection;
use crate::basic_client::ws_provider::ws_message_handler::WsMessageHandler;
use crate::basic_client::ws_provider::ws_rate_limiter::WsRateLimiter;
use crate::basic_client::ws_provider::ws_reconnect::WsReconnectStrategy;
use crate::basic_client::ws_provider::ws_response_tracker::WsResponseTracker;
use crate::{
    config::{Config, ConnectionMode},
    error::PaintboardError,
    event::{Event, EventBus},
    models::{OpCode, PaintOperation, PaintResult, PaintStatus, Pos, ProtocolMessage, Rgb},
};
use futures::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex as TokioMutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout, Duration};
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{debug, error, info, trace, warn};
use url::Url;

/// 最大包大小 (32 KB)
const MAX_PACKET_SIZE: usize = 32 * 1024; // 32 KB
const PENDING_PACKETS_SIZE_LIMIT: usize = 32;
const PENDING_PACKETS_DURATION_MILLS: u64 = 200;

/// Refactored WebSocket provider for Winter Paintboard API
#[derive(Clone)]
pub struct WsProvider {
    config: Arc<Config>,
    connection: Arc<WsConnection>,
    response_tracker: Arc<WsResponseTracker>,
    message_handler: Arc<WsMessageHandler>,
    message_task_handle: Arc<TokioMutex<Option<JoinHandle<()>>>>,
    send_pending_packets_by_duration_handle: Arc<TokioMutex<Option<JoinHandle<()>>>>,
    send_pending_packets_by_limit_handle: Arc<TokioMutex<Option<JoinHandle<()>>>>,
    reconnect_strategy: Arc<TokioMutex<WsReconnectStrategy>>,
    rate_limiter: Arc<WsRateLimiter>,
    cleanup_task_handle: Arc<TokioMutex<Option<JoinHandle<()>>>>,
    message_task_ready: Arc<Notify>,
    pending_packets: Arc<RwLock<VecDeque<Vec<u8>>>>,
}

impl WsProvider {
    /// Create a new WebSocket provider (refactored)
    pub async fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        let connection = Arc::new(WsConnection::new(config.clone()));
        let response_tracker = Arc::new(WsResponseTracker::new());
        let message_handler = Arc::new(WsMessageHandler::new(response_tracker.clone()));
        let reconnect_strategy = Arc::new(TokioMutex::new(WsReconnectStrategy::default()));
        // 采用与原实现一致的速率：256 rps（注意：原注释存在 120/256 的混淆）
        let rate_limiter = Arc::new(WsRateLimiter::new(256));

        // 启动响应追踪器清理任务
        let cleanup_tracker = response_tracker.clone();
        let cleanup_task_handle = Some(tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60)); // 每60秒清理一次
            loop {
                interval.tick().await;
                let removed_count = cleanup_tracker
                    .cleanup_expired_requests(Duration::from_secs(60))
                    .await;
                if removed_count > 0 {
                    debug!("响应追踪器清理任务：移除了 {} 个过期请求", removed_count);
                }
            }
        }));

        Ok(Self {
            config,
            connection,
            response_tracker,
            message_handler,
            message_task_handle: Arc::new(TokioMutex::new(None)),
            reconnect_strategy,
            rate_limiter,
            cleanup_task_handle: Arc::new(TokioMutex::new(None)),
            message_task_ready: Arc::new(Notify::new()),
            pending_packets: Arc::new(RwLock::new(VecDeque::new())),
            send_pending_packets_by_duration_handle: Arc::new(TokioMutex::new(None)),
            send_pending_packets_by_limit_handle: Arc::new(TokioMutex::new(None)),
        })
    }

    /// Set the user ID and token for authentication (deprecated - use methods with auth parameters)
    #[deprecated(note = "Use methods that accept auth parameters instead")]
    pub fn set_auth(&mut self, _uid: u32, _token: String) {
        // This method is deprecated in the new architecture
        // Authentication is now passed as parameters to each method
    }

    /// Connect to the WebSocket server and start background processing
    pub async fn connect(&mut self) -> Result<(), PaintboardError> {
        debug!("开始连接到 WebSocket 服务器: {}", self.config.ws_url);
        self.connection.connect().await?;
        // Cancel previous task and start a new one
        self.start_message_processing_task().await;

        // 等待后台消息处理任务真正启动
        debug!("等待后台消息处理任务启动...");
        let ready_notify = self.message_task_ready.clone();
        tokio::time::timeout(Duration::from_secs(5), ready_notify.notified())
            .await
            .map_err(|_| PaintboardError::timeout())?;
        debug!("后台消息处理任务已启动");

        Ok(())
    }

    /// Start the background task for processing incoming WebSocket messages
    async fn start_message_processing_task(&mut self) {
        debug!("开始启动消息处理任务");
        // Cancel previous task
        {
            let mut guard = self.message_task_handle.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
                debug!("已停止之前的消息处理任务");
            }
        }

        let stream_arc = self.connection.stream();
        let handler = self.message_handler.clone();
        let reconnect_strategy = self.reconnect_strategy.clone();
        let connection = self.connection.clone();
        let config = self.config.clone();
        let ready_notify = self.message_task_ready.clone();

        *self.message_task_handle.lock().await = Some(tokio::spawn(async move {
            debug!("消息处理任务开始运行");

            // 发送启动完成信号
            ready_notify.notify_one();
            debug!("消息处理任务启动信号已发送");

            loop {
                // Inner loop: read messages while connected
                loop {
                    let mut guard = stream_arc.lock().await;
                    if let Some(ref mut ws_stream) = *guard {
                        match ws_stream.next().await {
                            Some(Ok(message)) => {
                                // Drop the guard before processing to avoid holding lock
                                drop(guard);
                                // Delegate processing to the message handler
                                handler
                                    .handle_message_stream(message, stream_arc.clone())
                                    .await;
                            }
                            Some(Err(e)) => {
                                drop(guard);
                                error!(
                                    "WebSocket 错误: {} (错误类型: {:?})",
                                    e,
                                    std::any::type_name_of_val(&e)
                                );
                                warn!("消息处理任务检测到错误，将清理连接并尝试重连");

                                // 清理可能的僵尸连接
                                let mut cleanup_guard = stream_arc.lock().await;
                                if cleanup_guard.is_some() {
                                    warn!("清理错误的连接状态");
                                    *cleanup_guard = None;
                                }
                                drop(cleanup_guard);

                                let _ = EventBus::global()
                                    .send(Event::error_event(format!("WebSocket error: {}", e)));
                                break; // break inner loop to attempt reconnection
                            }
                            None => {
                                drop(guard);
                                warn!("WebSocket 连接关闭 (收到 None)，清理连接状态");

                                // 清理连接
                                let mut cleanup_guard = stream_arc.lock().await;
                                if cleanup_guard.is_some() {
                                    warn!("清理已关闭的连接");
                                    *cleanup_guard = None;
                                }
                                drop(cleanup_guard);

                                let _ = EventBus::global().send(Event::ConnectionClosed);
                                break; // break inner loop to attempt reconnection
                            }
                        }
                    } else {
                        // No connection; wait briefly and retry
                        drop(guard);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }

                // Check if reconnection is allowed
                let mut rs = reconnect_strategy.lock().await;
                if !rs.should_reconnect() {
                    debug!("收到停止重连信号，退出消息处理任务");
                    break;
                }

                // Wait according to backoff
                let delay = rs.next_delay();
                info!("尝试重新连接到 WebSocket 服务器，等待 {:?}", delay);
                tokio::time::sleep(delay).await;

                // Try to reconnect via shared connection
                match connection.connect().await {
                    Ok(_) => {
                        debug!("WebSocket 重连成功");
                        rs.reset();
                        let _ = EventBus::global().send(Event::ConnectionOpened);
                        // Continue outer loop to resume reading
                        continue;
                    }
                    Err(e) => {
                        error!("WebSocket 重连失败: {}", e);
                        // Continue to next iteration which will backoff again
                        continue;
                    }
                }
            }
            debug!("消息处理任务结束");
        }));

        if self.config.connection_mode != ConnectionMode::ReadOnly {
            let mut self_clone = self.clone();

            *self.send_pending_packets_by_duration_handle.lock().await =
                Some(tokio::spawn(async move {
                    let mut interval =
                        interval(Duration::from_millis(PENDING_PACKETS_DURATION_MILLS));

                    loop {
                        if self_clone.pending_packets.read().await.len() >= 1 {
                            let _ = self_clone.send_pending_packets().await;
                        }

                        interval.tick().await;
                    }
                }));

            let mut self_clone = self.clone();
            *self.send_pending_packets_by_limit_handle.lock().await =
                Some(tokio::spawn(async move {
                    let mut interval = interval(Duration::from_millis(100));

                    loop {
                        if self_clone.pending_packets.read().await.len()
                            >= PENDING_PACKETS_SIZE_LIMIT
                        {
                            let _ = self_clone.send_pending_packets().await;

                            debug!("触发 Pending Packets Limit，直接发送")
                        }

                        interval.tick().await;
                    }
                }));
        }
    }

    async fn send_pending_packets(&mut self) -> Result<(), PaintboardError> {
        // warn!("开始发送");
        let mut merged_packets: Vec<u8> = vec![];

        {
            let pending_packets = self.pending_packets.read().await;

            if pending_packets.len() == 0 {
                return Ok(());
            }

            for packet in pending_packets.iter() {
                for val in packet {
                    merged_packets.push(*val);
                }
            }
        }

        let size = merged_packets.len();
        let sending_result = tokio::time::timeout(
            Duration::from_millis(5 * 1000),
            self.connection.send_binary(merged_packets),
        )
        .await;

        match sending_result {
            Ok(Ok(_)) => {
                debug!(
                    "成功发送 Pending Packets，共计 {} 个 bytes，清空 Deque",
                    size
                );
                // 只有在成功发送后才清空队列
                self.pending_packets.write().await.clear();
            }
            Ok(Err(e)) => {
                error!("发送 Pending Packets 错误：{:?}", e);
                // 发送失败时不清理队列，让后台任务继续重试
                return Err(e);
            }
            Err(_) => {
                error!("发送 Pending Packets 超时");
                // 发送超时也不清理队列，让后台任务继续重试
                return Err(PaintboardError::Timeout);
            }
        }

        // warn!("发送完毕");

        Ok(())
    }

    /// 延迟绘画
    ///
    /// 将绘画请求放入 [`self.pending_packets`] 中
    ///
    /// 如果队列长度超过 [`PENDING_PACKETS_SIZE_LIMIT`] 或者距离上次发送时间大于 [`PENDING_PACKETS_DURATION_MILLS`] 毫秒，则调用 [`send_pending_packets`] 发送（此操作在后台执行）
    pub async fn paint_delayed(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<PaintResult, PaintboardError> {
        let paint_id = rand::random::<u32>();
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token: token.to_string(),
            paint_id: paint_id as u32,
        };

        let binary_data = operation.to_binary();

        let response_rx = self.response_tracker.register_request(paint_id).await;

        {
            let mut pending_packets = self.pending_packets.write().await;
            pending_packets.push_back(binary_data);
        }

        match timeout(Duration::from_secs(10), response_rx).await {
            Ok(Ok(result)) => {
                debug!("成功接收到绘图结果");
                Ok(result)
            }
            Ok(Err(_)) => {
                debug!("响应通道关闭");
                Err(PaintboardError::ResponseChannelClosed)
            }
            Err(_) => {
                warn!("等待响应超时 (paint_id: {}，超时: 10s)，清理通道", paint_id);
                let removed = self.response_tracker.remove_request(paint_id).await;
                debug!("清理响应通道结果: {}", removed);

                // 检查连接状态
                let still_connected = self.connection.is_connected().await;
                warn!("超时后连接状态: {}", still_connected);

                Err(PaintboardError::timeout())
            }
        }
    }

    /// Paint a pixel at the given position with the specified color using provided authentication
    pub async fn paint_with_auth(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<PaintResult, PaintboardError> {
        // Rate limiting: silent drop when limited
        if !self.rate_limiter.check() {
            debug!(
                "速率限制：paint 请求被丢弃，位置: ({}, {}), 颜色: ({}, {}, {})",
                pos.x, pos.y, color.r, color.g, color.b
            );
            return Ok(PaintResult {
                drawing_id: 0,
                status: PaintStatus::Success,
                message: "Request dropped due to rate limiting".to_string(),
            });
        }

        // Generate unique paint id first (needed for logging)
        let paint_id = rand::random::<u32>();

        // Ensure connected
        let is_connected = self.connection.is_connected().await;
        debug!(
            "paint_with_auth() - 连接状态检查结果: {} (paint_id: {})",
            is_connected, paint_id
        );
        if !is_connected {
            debug!("连接不存在，建立新连接 (paint_id: {})", paint_id);
            self.connection.connect().await?;
            debug!("新连接建立成功 (paint_id: {})", paint_id);
        } else {
            debug!("连接已存在，尝试复用连接 (paint_id: {})", paint_id);
        }

        // Create operation with provided authentication
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token: token.to_string(),
            paint_id: paint_id as u32,
        };

        let binary_data = operation.to_binary();

        // 单次绘图大小检查（理论上不会超过，但为完整性添加）
        if binary_data.len() > MAX_PACKET_SIZE {
            return Err(PaintboardError::invalid_data(format!(
                "绘图消息大小 {} 字节超过限制 {} 字节 ({}KB)",
                binary_data.len(),
                MAX_PACKET_SIZE,
                MAX_PACKET_SIZE / 1024
            )));
        }

        trace!(
            "绘图操作二进制数据长度: {}, 前几个字节: {:?}",
            binary_data.len(),
            &binary_data[..std::cmp::min(10, binary_data.len())]
        );

        // Register response receiver
        debug!("注册响应追踪器，paint_id: {}", paint_id);
        let response_rx = self.response_tracker.register_request(paint_id).await;

        // Send binary
        debug!("准备发送绘图消息，paint_id: {}", paint_id);
        self.connection
            .send_binary(binary_data)
            .await
            .map_err(|e| {
                error!("发送绘图消息失败 (paint_id: {}): {:?}", paint_id, e);
                e
            })?;
        debug!("绘图消息已发送，等待响应 (paint_id: {})", paint_id);

        // Wait for response with timeout
        match timeout(Duration::from_secs(10), response_rx).await {
            Ok(Ok(result)) => {
                debug!("成功接收到绘图结果");
                Ok(result)
            }
            Ok(Err(_)) => {
                debug!("响应通道关闭");
                Err(PaintboardError::ResponseChannelClosed)
            }
            Err(_) => {
                warn!("等待响应超时 (paint_id: {}，超时: 10s)，清理通道", paint_id);
                let removed = self.response_tracker.remove_request(paint_id).await;
                debug!("清理响应通道结果: {}", removed);

                // 检查连接状态
                let still_connected = self.connection.is_connected().await;
                warn!("超时后连接状态: {}", still_connected);

                Err(PaintboardError::timeout())
            }
        }
    }

    /// Paint a pixel at the given position with the specified color (deprecated - use paint_with_auth)
    #[deprecated(note = "Use paint_with_auth instead")]
    pub async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        Err(PaintboardError::auth(
            "Authentication required. Use paint_with_auth instead.".to_string(),
        ))
    }

    /// Send multiple paint operations together using sticky packet mechanism, without waiting for responses
    pub async fn paint_batch_with_auth(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        let ops_count = operations.len();
        debug!("开始发送批量绘图请求，操作数量: {}", ops_count);

        if operations.is_empty() {
            return Ok(());
        }

        // Rate limiting: treat batch as single packet
        if !self.rate_limiter.check() {
            debug!("速率限制：paint_batch 请求被丢弃，操作数量: {}", ops_count);
            return Ok(());
        }

        let mut all_binary = Vec::new();
        let mut paint_ids = Vec::new(); // 存储paint_id用于清理

        for (pos, color) in &operations {
            let paint_id = rand::random::<u32>();
            let op = PaintOperation {
                pos: *pos,
                color: *color,
                token_uid: uid,
                token: token.to_string(),
                paint_id: paint_id as u32,
            };
            all_binary.extend(op.to_binary());
            paint_ids.push(paint_id); // 记录paint_id
        }

        // 检查批量包大小，避免触发服务端 1009 (Message too big)
        if all_binary.len() > MAX_PACKET_SIZE {
            // 如果包太大，清理已注册的请求
            for paint_id in paint_ids {
                let _ = self.response_tracker.remove_request(paint_id).await;
            }
            return Err(PaintboardError::invalid_data(format!(
                "批量包大小 {} 字节超过限制 {} 字节 ({}KB)",
                all_binary.len(),
                MAX_PACKET_SIZE,
                MAX_PACKET_SIZE / 1024
            )));
        }

        if !self.connection.is_connected().await {
            debug!("批量发送 - 连接不存在，建立新连接");
            self.connection.connect().await?;
        } else {
            debug!("批量发送 - 连接已存在，复用连接");
        }

        // 为所有操作注册请求，以便后台清理任务可以清理它们
        for paint_id in &paint_ids {
            let _ = self.response_tracker.register_request(*paint_id).await;
        }

        let send_result = self.connection.send_binary(all_binary).await;
        if let Err(e) = send_result {
            error!("发送批量消息失败: {:?}", e);
            // 发送失败时清理已注册的请求
            for paint_id in &paint_ids {
                let _ = self.response_tracker.remove_request(*paint_id).await;
            }
            return Err(e);
        }

        // Emit own_paint_event for each operation
        let event_bus = EventBus::global();
        for (pos, color) in &operations {
            let _ = event_bus.send(Event::own_paint_event(*pos, *color));
        }

        Ok(())
    }

    /// Send multiple paint operations together using sticky packet mechanism, without waiting for responses (deprecated - use paint_batch_with_auth)
    #[deprecated(note = "Use paint_batch_with_auth instead")]
    pub async fn paint_batch(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
    ) -> Result<(), PaintboardError> {
        Err(PaintboardError::auth(
            "Authentication required. Use paint_batch_with_auth instead.".to_string(),
        ))
    }

    /// Send a heartbeat (PONG) response
    pub async fn send_heartbeat_pong(&mut self) -> Result<(), PaintboardError> {
        if !self.connection.is_connected().await {
            self.connection.connect().await?;
        }
        let pong_message = vec![OpCode::HeartbeatPong as u8];
        self.connection.send_binary(pong_message).await?;
        Ok(())
    }

    /// Properly disconnect and clean up the WebSocket connection
    pub async fn disconnect(&mut self) -> Result<(), PaintboardError> {
        // Prevent reconnection attempts
        {
            let mut rs = self.reconnect_strategy.lock().await;
            rs.disable_reconnect();
        }

        {
            let mut guard = self.message_task_handle.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }

        // 停止清理任务
        {
            let mut guard = self.cleanup_task_handle.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }

        // 停止 Pending Packets 相关
        {
            let mut guard = self.send_pending_packets_by_duration_handle.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }

        {
            let mut guard = self.send_pending_packets_by_limit_handle.lock().await;
            if let Some(handle) = guard.take() {
                handle.abort();
            }
        }

        self.connection.close().await?;
        Ok(())
    }

    /// 使用临时 Token 绘制像素（现在直接使用认证参数）
    pub async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<PaintResult, PaintboardError> {
        // 直接使用提供的认证信息，无需保存/恢复状态
        self.paint_with_auth(pos, color, uid, &token).await
    }
}
