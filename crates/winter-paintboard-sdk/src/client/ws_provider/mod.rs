mod ws_message_handler;
mod ws_rate_limiter;
mod ws_connection;
mod ws_reconnect;
mod ws_response_tracker;

use crate::{
    config::{Config, ConnectionMode},
    error::PaintboardError,
    event::{Event, EventBus},
    models::{OpCode, PaintOperation, PaintResult, PaintStatus, Pos, ProtocolMessage, Rgb},
};
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, oneshot};
use tracing::{debug, error, info, trace, warn};
use url::Url;
use futures::{SinkExt, StreamExt};
use crate::client::ws_provider::ws_connection::WsConnection;
use crate::client::ws_provider::ws_response_tracker::WsResponseTracker;
use crate::client::ws_provider::ws_message_handler::WsMessageHandler;
use crate::client::ws_provider::ws_reconnect::WsReconnectStrategy;
use crate::client::ws_provider::ws_rate_limiter::WsRateLimiter;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio::time::{timeout, Duration};
use tokio::task::JoinHandle;

/// Refactored WebSocket provider for Winter Paintboard API
pub struct WsProvider {
    config: Arc<Config>,
    connection: Arc<WsConnection>,
    uid: Option<u32>,
    token: Option<String>,
    response_tracker: Arc<WsResponseTracker>,
    message_handler: Arc<WsMessageHandler>,
    message_task_handle: Option<JoinHandle<()>>,
    reconnect_strategy: Arc<TokioMutex<WsReconnectStrategy>>,
    rate_limiter: Arc<WsRateLimiter>,
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

        Ok(Self {
            config,
            connection,
            uid: None,
            token: None,
            response_tracker,
            message_handler,
            message_task_handle: None,
            reconnect_strategy,
            rate_limiter,
        })
    }

    /// Set the user ID and token for authentication
    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }

    /// Connect to the WebSocket server and start background processing
    pub async fn connect(&mut self) -> Result<(), PaintboardError> {
        debug!("开始连接到 WebSocket 服务器: {}", self.config.ws_url);
        self.connection.connect().await?;
        // Cancel previous task and start a new one
        self.start_message_processing_task().await;
        Ok(())
    }

    /// Start the background task for processing incoming WebSocket messages
    async fn start_message_processing_task(&mut self) {
        debug!("开始启动消息处理任务");
        // Cancel previous task
        if let Some(handle) = self.message_task_handle.take() {
            handle.abort();
            debug!("已停止之前的消息处理任务");
        }

        let stream_arc = self.connection.stream();
        let handler = self.message_handler.clone();
        let reconnect_strategy = self.reconnect_strategy.clone();
        let connection = self.connection.clone();
        let config = self.config.clone();

        self.message_task_handle = Some(tokio::spawn(async move {
            debug!("消息处理任务开始运行");
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
                                error!("WebSocket 错误: {}", e);
                                let _ = EventBus::global().send(Event::error_event(format!(
                                    "WebSocket error: {}",
                                    e
                                )));
                                break; // break inner loop to attempt reconnection
                            }
                            None => {
                                drop(guard);
                                debug!("WebSocket 连接关闭 (None received)");
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
    }

    /// Paint a pixel at the given position with the specified color
    pub async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
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

        // Authentication
        let uid = self.uid.ok_or(PaintboardError::auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::auth("Token not set".to_string()))?;

        // Ensure connected
        if !self.connection.is_connected().await {
            debug!("连接不存在，建立新连接");
            self.connection.connect().await?;
        } else {
            debug!("连接已存在，复用连接");
        }

        // Generate unique paint id and operation
        let paint_id = rand::random::<u64>();
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token,
            paint_id: paint_id as u32,
        };

        let binary_data = operation.to_binary();
        trace!(
            "绘图操作二进制数据长度: {}, 前几个字节: {:?}",
            binary_data.len(),
            &binary_data[..std::cmp::min(10, binary_data.len())]
        );

        // Register response receiver
        let response_rx = self.response_tracker.register_request(paint_id).await;

        // Send binary
        self.connection.send_binary(binary_data).await.map_err(|e| {
            error!("发送绘图消息失败: {:?}", e);
            e
        })?;

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
                debug!("等待响应超时，清理通道");
                let _ = self.response_tracker.remove_request(paint_id).await;
                Err(PaintboardError::timeout())
            }
        }
    }

    /// Send multiple paint operations together using sticky packet mechanism, without waiting for responses
    pub async fn paint_batch(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
    ) -> Result<(), PaintboardError> {
        let ops_count = operations.len();
        debug!("开始发送批量绘图请求，操作数量: {}", ops_count);

        if operations.is_empty() {
            return Ok(());
        }

        // Rate limiting: treat batch as single packet
        if !self.rate_limiter.check() {
            debug!(
                "速率限制：paint_batch 请求被丢弃，操作数量: {}",
                ops_count
            );
            return Ok(());
        }

        // Authentication
        let uid = self.uid.ok_or(PaintboardError::auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::auth("Token not set".to_string()))?;

        let mut all_binary = Vec::new();
        for (pos, color) in &operations {
            let paint_id = rand::random::<u64>();
            let op = PaintOperation {
                pos: *pos,
                color: *color,
                token_uid: uid,
                token: token.clone(),
                paint_id: paint_id as u32,
            };
            all_binary.extend(op.to_binary());
        }

        if !self.connection.is_connected().await {
            debug!("批量发送 - 连接不存在，建立新连接");
            self.connection.connect().await?;
        } else {
            debug!("批量发送 - 连接已存在，复用连接");
        }

        self.connection.send_binary(all_binary).await.map_err(|e| {
            error!("发送批量消息失败: {:?}", e);
            e
        })?;

        // Emit own_paint_event for each operation
        let event_bus = EventBus::global();
        for (pos, color) in &operations {
            let _ = event_bus.send(Event::own_paint_event(*pos, *color));
        }

        Ok(())
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

    /// Listen for a single event from the server (helper)
    pub async fn listen_for_events(&mut self) -> Result<ProtocolMessage, PaintboardError> {
        if !self.connection.is_connected().await {
            self.connection.connect().await?;
        }

        let stream = self.connection.stream();
        let mut guard = stream.lock().await;
        let ws_stream = guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;

        if let Some(msg) = ws_stream.next().await {
            let msg = msg.map_err(|e| PaintboardError::websocket(e.to_string()))?;
            match msg {
                Message::Binary(data) => ProtocolMessage::parse(&data),
                Message::Text(_) => Err(PaintboardError::invalid_data(
                    "Received unexpected text message",
                )),
                Message::Close(_) => Err(PaintboardError::ConnectionClosed),
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                    Err(PaintboardError::invalid_data(
                        "Received unexpected WebSocket frame type",
                    ))
                }
                _ => Err(PaintboardError::invalid_data("Unexpected message type")),
            }
        } else {
            Err(PaintboardError::ConnectionClosed)
        }
    }

    /// Properly disconnect and clean up the WebSocket connection
    pub async fn disconnect(&mut self) -> Result<(), PaintboardError> {
        // Prevent reconnection attempts
        {
            let mut rs = self.reconnect_strategy.lock().await;
            rs.disable_reconnect();
        }

        if let Some(handle) = self.message_task_handle.take() {
            handle.abort();
        }

        self.connection.close().await?;
        Ok(())
    }
}
