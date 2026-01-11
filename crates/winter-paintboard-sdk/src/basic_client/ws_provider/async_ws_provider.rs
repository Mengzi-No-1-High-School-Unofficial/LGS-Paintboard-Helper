use crate::{
    config::{Config, ConnectionMode},
    error::PaintboardError,
    models::{OpCode, PaintOperation, PaintResult, PaintStatus, Pos, ProtocolMessage, Rgb},
};
use color_eyre::Report;
use futures::{SinkExt, StreamExt};
use std::collections::VecDeque;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{mpsc, oneshot, Mutex as TokioMutex, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration, Instant};
use tracing::{debug, error, trace, warn};

use super::{
    ws_rate_limiter::WsRateLimiter, ws_reconnect::WsReconnectManager,
    ws_response_tracker::WsResponseTracker,
};

/// 最大包大小 (32 KB)
const MAX_PACKET_SIZE: usize = 32 * 1024; // 32 KB
const PENDING_PACKETS_SIZE_LIMIT: usize = 32;
// 优化：将延迟从200ms降至20ms，大幅减少反击延迟
const PENDING_PACKETS_DURATION_MILLS: u64 = 20;

/// 连接请求类型
enum ConnectionRequest {
    Connect(oneshot::Sender<Result<(), PaintboardError>>),
    IsConnected(oneshot::Sender<bool>),
    Close(oneshot::Sender<Result<(), PaintboardError>>),
    /// 用于标记连接已断开（例如，消息处理任务结束）
    MarkDisconnected,
    /// 内部消息，用于触发重连任务
    StartReconnect,
}

/// 发送请求类型
enum SendRequest {
    SendBinary {
        data: Vec<u8>,
        response_tx: oneshot::Sender<Result<(), Report>>,
    },
    /// 用于发送 Pong 响应
    SendPong {
        payload: Vec<u8>,
        response_tx: oneshot::Sender<Result<(), Report>>,
    },
}

/// 控制请求类型
enum ControlRequest {
    Shutdown(oneshot::Sender<()>),
}

/// WsActor 消息类型
enum WsActorMessage {
    Connection(ConnectionRequest),
    Send(SendRequest),
    Control(ControlRequest),
}

/// WsActor 消息处理器
struct WsActor {
    config: Arc<Config>,
    response_tracker: Arc<WsResponseTracker>,
    reconnect_manager: Arc<TokioMutex<WsReconnectManager>>,
    #[allow(dead_code)]
    rate_limiter: Arc<WsRateLimiter>,
    message_task_ready: Arc<Notify>,
    reconnecting: Arc<AtomicBool>,
    pending_packets: Arc<RwLock<VecDeque<Vec<u8>>>>,
    /// 用于接收消息的通道
    receiver: mpsc::UnboundedReceiver<WsActorMessage>,
    /// 用于发送消息的通道
    sender: mpsc::UnboundedSender<WsActorMessage>,
    /// 专用的WebSocket发送channel（用于解耦发送操作）
    ws_send_tx: Option<mpsc::UnboundedSender<(Vec<u8>, oneshot::Sender<Result<(), Report>>)>>,
    /// WebSocket 发送端
    ws_sender: Option<
        futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            tokio_tungstenite::tungstenite::protocol::Message,
        >,
    >,
    /// WebSocket 接收流
    ws_receiver_stream: Option<
        futures::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
    >,
    /// 消息处理任务句柄
    message_task_handle: Option<JoinHandle<()>>,
    /// 发送按时间间隔触发的任务句柄
    #[allow(dead_code)]
    send_pending_packets_by_duration_handle: Option<JoinHandle<()>>,
    /// 发送按大小限制触发的任务句柄
    #[allow(dead_code)]
    send_pending_packets_by_limit_handle: Option<JoinHandle<()>>,
    /// 清理任务句柄
    cleanup_task_handle: Option<JoinHandle<()>>,
    /// 健康检查任务句柄
    health_check_task_handle: Option<JoinHandle<()>>,
    /// 重连任务句柄
    reconnect_task_handle: Option<JoinHandle<()>>,
    /// 连接是否健康
    connected: Arc<TokioMutex<bool>>,
    /// 上次发送数据包的时间
    last_send_time: Arc<TokioMutex<Instant>>,
    /// 最后一次健康检查的时间
    last_health_check: Arc<TokioMutex<Instant>>,
}

impl WsActor {
    #[allow(clippy::too_many_arguments)]
    fn new(
        config: Arc<Config>,
        response_tracker: Arc<WsResponseTracker>,
        reconnect_manager: Arc<TokioMutex<WsReconnectManager>>,
        rate_limiter: Arc<WsRateLimiter>,
        message_task_ready: Arc<Notify>,
        reconnecting: Arc<AtomicBool>,
        pending_packets: Arc<RwLock<VecDeque<Vec<u8>>>>,
        receiver: mpsc::UnboundedReceiver<WsActorMessage>,
        sender: mpsc::UnboundedSender<WsActorMessage>,
    ) -> Self {
        Self {
            config,
            response_tracker,
            reconnect_manager,
            rate_limiter,
            message_task_ready,
            reconnecting,
            pending_packets,
            receiver,
            sender: sender.clone(),
            ws_send_tx: None, // 初始化为None，在连接时创建
            ws_sender: None,
            ws_receiver_stream: None,
            message_task_handle: None,
            send_pending_packets_by_duration_handle: None,
            send_pending_packets_by_limit_handle: None,
            cleanup_task_handle: None,
            health_check_task_handle: None,
            reconnect_task_handle: None,
            connected: Arc::new(TokioMutex::new(false)),
            last_send_time: Arc::new(TokioMutex::new(Instant::now())),
            last_health_check: Arc::new(TokioMutex::new(Instant::now())),
        }
    }

    async fn run(&mut self) {
        // 启动清理任务
        self.start_cleanup_task().await;
        // 启动健康检查任务
        self.start_health_check_task().await;

        // 主循环：处理来自通道的消息和发送待处理数据包
        let mut duration_interval =
            tokio::time::interval(Duration::from_millis(PENDING_PACKETS_DURATION_MILLS));
        let mut limit_check_interval = tokio::time::interval(Duration::from_millis(100)); // 每100ms检查一次

        loop {
            tokio::select! {
                // 处理来自外部的 Actor 消息
                Some(message) = self.receiver.recv() => {
                    match message {
                        WsActorMessage::Connection(req) => {
                            self.handle_connection_request(req).await;
                        }
                        WsActorMessage::Send(req) => {
                            self.handle_send_request(req).await;
                        }
                        WsActorMessage::Control(req) => {
                            if self.handle_control_request(req).await {
                                break; // 收到关闭信号，退出循环
                            }
                        }
                    }
                }
                // 按时间间隔发送待处理数据包
                _ = duration_interval.tick() => {
                    let mut pending_packets = self.pending_packets.write().await;
                    let mut last_send_time = self.last_send_time.lock().await;

                    if !pending_packets.is_empty() && last_send_time.elapsed() >= Duration::from_millis(PENDING_PACKETS_DURATION_MILLS) {
                        let packets_to_send: Vec<Vec<u8>> = pending_packets.drain(..).collect();
                        let data_to_send = packets_to_send.iter().flatten().cloned().collect::<Vec<u8>>();
                        if !data_to_send.is_empty() {
                            let data_len = data_to_send.len();
                            let (response_tx, _) = oneshot::channel();
                            let message = WsActorMessage::Send(SendRequest::SendBinary {
                                data: data_to_send,
                                response_tx,
                            });
                            if let Err(e) = self.sender.send(message) {
                                error!("发送 pending_packets (按时间) 失败: {:?}，触发重连检查", e);
                                // 发送失败，将数据放回队列
                                *pending_packets = packets_to_send.into_iter().collect();
                                // 触发连接状态检查
                                let _ = self.sender.send(WsActorMessage::Connection(ConnectionRequest::MarkDisconnected));
                            } else {
                                debug!("按时间间隔发送了 {} 字节的 pending_packets", data_len);
                                *last_send_time = Instant::now();
                            }
                        }
                    }
                }
                // 按大小限制发送待处理数据包
                _ = limit_check_interval.tick() => {
                    let mut pending_packets = self.pending_packets.write().await;
                    let mut last_send_time = self.last_send_time.lock().await;

                    if pending_packets.len() >= PENDING_PACKETS_SIZE_LIMIT {
                        let packets_to_send: Vec<Vec<u8>> = pending_packets.drain(..).collect();
                        let data_to_send = packets_to_send.iter().flatten().cloned().collect::<Vec<u8>>();
                        if !data_to_send.is_empty() {
                            let data_len = data_to_send.len();
                            let (response_tx, _) = oneshot::channel();
                            let message = WsActorMessage::Send(SendRequest::SendBinary {
                                data: data_to_send,
                                response_tx,
                            });
                            if let Err(e) = self.sender.send(message) {
                                error!("发送 pending_packets (按大小) 失败: {:?}，触发重连检查", e);
                                // 发送失败，将数据放回队列
                                *pending_packets = packets_to_send.into_iter().collect();
                                // 触发连接状态检查
                                let _ = self.sender.send(WsActorMessage::Connection(ConnectionRequest::MarkDisconnected));
                            } else {
                                debug!("按大小限制发送了 {} 字节的 pending_packets", data_len);
                                *last_send_time = Instant::now();
                            }
                        }
                    }
                }
            }
        }

        // 清理资源
        self.cleanup().await;
    }

    async fn handle_connection_request(&mut self, req: ConnectionRequest) {
        match req {
            ConnectionRequest::Connect(response_tx) => {
                let result = self.connect_internal().await;
                let _ = response_tx.send(result);
            }
            ConnectionRequest::IsConnected(response_tx) => {
                let connected = *self.connected.lock().await;
                let _ = response_tx.send(connected);
            }
            ConnectionRequest::Close(response_tx) => {
                let result = self.close_internal().await;
                let _ = response_tx.send(result);
            }
            ConnectionRequest::MarkDisconnected => {
                debug!("标记连接为断开");
                *self.connected.lock().await = false;
                self.ws_sender = None;
                self.ws_receiver_stream = None;

                if !self.reconnecting.load(Ordering::Relaxed) {
                    debug!("检测到连接断开，发送 StartReconnect 消息");
                    let _ = self.sender.send(WsActorMessage::Connection(
                        ConnectionRequest::StartReconnect,
                    ));
                }
            }
            ConnectionRequest::StartReconnect => {
                self.start_reconnect_task().await;
            }
        }
    }

    async fn handle_send_request(&mut self, req: SendRequest) {
        match req {
            SendRequest::SendBinary { data, response_tx } => {
                // 真正的异步优化：使用专用channel，Actor立即返回，不阻塞
                if let Some(ref send_tx) = self.ws_send_tx {
                    let is_connected = *self.connected.lock().await;
                    let is_reconnecting = self.reconnecting.load(Ordering::Relaxed);

                    if !is_connected || is_reconnecting {
                        let _ = response_tx
                            .send(Err(Report::new(PaintboardError::ConnectionClosed)
                                .wrap_err("无法发送消息，连接未建立或正在重连")));
                        return;
                    }

                    // 立即发送到专用channel，不等待实际发送完成
                    // Actor可以立即处理下一个消息
                    if send_tx.send((data, response_tx)).is_err() {
                        error!("发送channel已关闭");
                        let _ = self.sender.send(WsActorMessage::Connection(
                            ConnectionRequest::MarkDisconnected,
                        ));
                    }
                } else {
                    *self.connected.lock().await = false;
                    let _ = response_tx.send(Err(Report::new(PaintboardError::ConnectionClosed)
                        .wrap_err("WebSocket 发送端不存在")));
                }
            }
            SendRequest::SendPong {
                payload,
                response_tx,
            } => {
                let result = self.send_pong_internal(payload).await;
                let _ = response_tx.send(result);
            }
        }
    }

    async fn handle_control_request(&mut self, req: ControlRequest) -> bool {
        match req {
            ControlRequest::Shutdown(response_tx) => {
                let _ = response_tx.send(());
                true // 返回 true 表示应该退出
            }
        }
    }

    async fn connect_internal(&mut self) -> Result<(), PaintboardError> {
        let url = url::Url::parse(&self.config.ws_url)
            .map_err(|e| PaintboardError::InvalidUrl(e.to_string()))?;

        // 根据连接模式添加查询参数
        let mut url = url.clone();
        match self.config.connection_mode {
            ConnectionMode::ReadOnly => {
                url.set_query(Some("readonly=1"));
            }
            ConnectionMode::WriteOnly => {
                url.set_query(Some("writeonly=1"));
            }
            ConnectionMode::ReadWrite => {
                // 默认模式，不需要参数
            }
        }

        debug!("连接到 WebSocket: {}", url);
        match tokio_tungstenite::connect_async(url).await {
            Ok((ws_stream, _)) => {
                debug!("WebSocket 连接建立成功");
                let (mut sender, receiver) = ws_stream.split();

                // 创建专用的发送channel
                let (send_tx, mut send_rx) =
                    mpsc::unbounded_channel::<(Vec<u8>, oneshot::Sender<Result<(), Report>>)>();

                // 启动独立的发送任务，避免阻塞Actor
                let last_send_time = self.last_send_time.clone();
                let actor_sender = self.sender.clone();
                tokio::spawn(async move {
                    while let Some((data, response_tx)) = send_rx.recv().await {
                        let send_result = sender
                            .send(tokio_tungstenite::tungstenite::protocol::Message::Binary(
                                data,
                            ))
                            .await;

                        match send_result {
                            Ok(_) => {
                                *last_send_time.lock().await = Instant::now();
                                let _ = response_tx.send(Ok(()));
                            }
                            Err(e) => {
                                error!("发送二进制消息时检测到连接错误: {}", e);
                                let _ = actor_sender.send(WsActorMessage::Connection(
                                    ConnectionRequest::MarkDisconnected,
                                ));
                                let _ =
                                    response_tx.send(Err(Report::new(e).wrap_err("发送消息失败")));
                                break; // 发送失败，退出循环
                            }
                        }
                    }
                    debug!("独立发送任务已退出");
                });

                self.ws_send_tx = Some(send_tx);
                self.ws_sender = None; // 不再需要直接持有sender
                self.ws_receiver_stream = Some(receiver);
                *self.connected.lock().await = true;
                self.reconnecting.store(false, Ordering::Relaxed);

                // 启动消息处理任务
                self.start_message_processing_task().await;

                Ok(())
            }
            Err(e) => {
                error!("WebSocket 连接失败: {}", e);
                Err(PaintboardError::websocket(e.to_string()))
            }
        }
    }

    async fn send_pong_internal(&mut self, payload: Vec<u8>) -> Result<(), Report> {
        debug!("尝试发送 Pong 消息，负载大小: {} 字节", payload.len());

        if !*self.connected.lock().await {
            return Err(Report::new(PaintboardError::ConnectionClosed)
                .wrap_err("无法发送 Pong，连接未建立"));
        }

        if let Some(ref mut ws_sender) = self.ws_sender {
            debug!("开始发送 Pong 消息");
            match ws_sender
                .send(tokio_tungstenite::tungstenite::protocol::Message::Pong(
                    payload,
                ))
                .await
            {
                Ok(_) => {
                    debug!("Pong 消息发送成功");
                    // 更新上次发送时间
                    *self.last_send_time.lock().await = Instant::now();
                    Ok(())
                }
                Err(e) => {
                    error!("发送 Pong 消息时检测到连接错误: {}", e);
                    // 发送 MarkDisconnected 消息来触发重连
                    let _ = self.sender.send(WsActorMessage::Connection(
                        ConnectionRequest::MarkDisconnected,
                    ));
                    Err(Report::new(e).wrap_err("发送 Pong 消息失败"))
                }
            }
        } else {
            *self.connected.lock().await = false;
            Err(Report::new(PaintboardError::ConnectionClosed).wrap_err("WebSocket 发送端不存在"))
        }
    }

    async fn close_internal(&mut self) -> Result<(), PaintboardError> {
        if let Some(mut ws_sender) = self.ws_sender.take() {
            let _ = ws_sender.close().await;
        }
        *self.connected.lock().await = false;
        self.ws_receiver_stream = None;
        Ok(())
    }

    async fn start_message_processing_task(&mut self) {
        if let Some(read) = self.ws_receiver_stream.take() {
            let response_tracker_clone = self.response_tracker.clone();
            let config_clone = self.config.clone();
            let reconnect_manager_clone = self.reconnect_manager.clone();
            let actor_sender = self.sender.clone();
            let task_handle = tokio::spawn(async move {
                Self::message_processing_loop(
                    read,
                    response_tracker_clone,
                    config_clone,
                    reconnect_manager_clone,
                    actor_sender.clone(),
                )
                .await;
                // After the message loop ends (due to error or close), mark the connection as disconnected
                let _ = actor_sender.send(WsActorMessage::Connection(
                    ConnectionRequest::MarkDisconnected,
                ));
            });

            self.message_task_handle = Some(task_handle);
        }

        debug!("开始启动消息处理任务");

        // 重置连接状态通知
        self.message_task_ready.notify_one();
        debug!("后台消息处理任务已启动");
    }
    async fn message_processing_loop(
        mut ws_stream: futures::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        response_tracker: Arc<WsResponseTracker>,
        _config: Arc<Config>,
        _reconnect_manager: Arc<TokioMutex<WsReconnectManager>>,
        actor_sender: mpsc::UnboundedSender<WsActorMessage>,
    ) {
        loop {
            match ws_stream.next().await {
                Some(Ok(message)) => {
                    match message {
                        tokio_tungstenite::tungstenite::protocol::Message::Binary(data) => {
                            let messages = ProtocolMessage::parse_batch(&data);

                            if let Ok(messages) = messages {
                                for protocol_msg in messages {
                                    Self::handle_protocol_message(
                                        protocol_msg,
                                        response_tracker.clone(),
                                        actor_sender.clone(),
                                    )
                                    .await;
                                }
                            } else {
                                error!(
                                    "无法解析二进制消息，前10字节: {:?}",
                                    &data[..std::cmp::min(10, data.len())]
                                );
                            }
                        }

                        tokio_tungstenite::tungstenite::protocol::Message::Ping(payload) => {
                            debug!("收到 WebSocket ping (payload: {} 字节)", payload.len());
                            // Send Pong response using the actor's sender
                            let (response_tx, _) = oneshot::channel();
                            let _ =
                                actor_sender.send(WsActorMessage::Send(SendRequest::SendPong {
                                    payload,
                                    response_tx,
                                }));
                        }

                        tokio_tungstenite::tungstenite::protocol::Message::Pong(_) => {
                            debug!("收到 WebSocket pong");
                        }

                        tokio_tungstenite::tungstenite::protocol::Message::Close(frame) => {
                            warn!("收到连接关闭: {:?}", frame);
                            break;
                        }

                        tokio_tungstenite::tungstenite::protocol::Message::Text(text) => {
                            warn!("收到意外的文本消息: {}", text);
                        }
                        _ => {
                            debug!("收到其他类型的消息");
                        }
                    }
                }
                Some(Err(e)) => {
                    error!("WebSocket 错误: {}", e);
                    break;
                }
                None => {
                    warn!("WebSocket 连接关闭 (收到 None)");
                    break;
                }
            }
        }
    }

    async fn handle_protocol_message(
        protocol_msg: ProtocolMessage,
        response_tracker: Arc<WsResponseTracker>,
        actor_sender: mpsc::UnboundedSender<WsActorMessage>,
    ) {
        match protocol_msg {
            ProtocolMessage::HeartbeatPing => {
                debug!("收到服务器心跳 PING");
                // Send Heartbeat Pong (0xfb) using the actor's sender
                let pong_msg = vec![OpCode::HeartbeatPong as u8];
                let (response_tx, _) = oneshot::channel();
                let _ = actor_sender.send(WsActorMessage::Send(SendRequest::SendBinary {
                    data: pong_msg,
                    response_tx,
                }));
            }

            ProtocolMessage::PaintResult { drawing_id, status } => {
                debug!(
                    "收到绘图结果: drawing_id={}, status=0x{:02x}",
                    drawing_id, status
                );
                use crate::models::{PaintResult, PaintStatus};
                let paint_status = PaintStatus::from(status);
                debug!("绘图状态: {:?}", paint_status);

                // 记录错误统计
                crate::error_stats::record_paint_result(&paint_status);

                let paint_result = PaintResult {
                    drawing_id,
                    status: paint_status,
                    message: format!("Paint result received with status: 0x{:02x}", status),
                };

                // 通过响应追踪器完成请求
                let matched = response_tracker
                    .complete_request(drawing_id, paint_result.clone())
                    .await;

                if !matched {
                    debug!("未找到匹配的 paint_id ({})", drawing_id);
                } else {
                    debug!("已完成匹配的响应通道 (drawing_id: {})", drawing_id);
                }
            }

            ProtocolMessage::PaintEvent { pos, color } => {
                use crate::event;
                // 发布像素更新事件（不区分来源，因为 PixelSource 字段未被使用）
                event::post(event::PaintEvent::PixelUpdate { pos, color });
                trace!("收到像素更新: ({}, {})", pos.x, pos.y);
            }

            ProtocolMessage::Unknown { opcode, data } => {
                warn!(
                    "收到未知消息类型: opcode=0x{:02x}, 数据长度: {} 字节",
                    opcode,
                    data.len()
                );
            }

            _ => {}
        }
    }

    async fn start_cleanup_task(&mut self) {
        let cleanup_tracker = self.response_tracker.clone();
        let cleanup_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // 每60秒清理一次
            loop {
                interval.tick().await;
                let removed_count = cleanup_tracker
                    .cleanup_expired_requests(Duration::from_secs(60))
                    .await;
                if removed_count > 0 {
                    debug!("响应追踪器清理任务：移除了 {} 个过期请求", removed_count);
                }
            }
        });

        self.cleanup_task_handle = Some(cleanup_task);
    }

    async fn cleanup(&mut self) {
        // 停止所有任务
        if let Some(handle) = self.message_task_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.cleanup_task_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.health_check_task_handle.take() {
            handle.abort();
        }
        if let Some(handle) = self.reconnect_task_handle.take() {
            handle.abort();
        }

        // 关闭连接
        let _ = self.close_internal().await;
    }

    /// 启动健康检查任务
    async fn start_health_check_task(&mut self) {
        let response_tracker_clone = self.response_tracker.clone();
        let actor_sender = self.sender.clone();
        let last_health_check_clone = self.last_health_check.clone();

        let task_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30)); // 每30秒进行一次健康检查

            loop {
                interval.tick().await;

                // 检查连接是否仍然存在（通过发送 IsConnected 请求）
                let (is_connected_tx, is_connected_rx) = oneshot::channel();
                let check_conn_msg =
                    WsActorMessage::Connection(ConnectionRequest::IsConnected(is_connected_tx));

                if actor_sender.send(check_conn_msg).is_err() {
                    continue; // 如果无法发送消息，跳过本次检查
                }

                let is_connected = match tokio::time::timeout(
                    std::time::Duration::from_millis(100),
                    is_connected_rx,
                )
                .await
                {
                    Ok(Ok(result)) => result,
                    _ => false,
                };

                if !is_connected {
                    // 如果未连接，则跳过健康检查
                    continue;
                }

                // 更新健康检查时间
                {
                    let mut guard = last_health_check_clone.lock().await;
                    *guard = Instant::now();
                }

                // 创建一个无效的 PaintRequest 用于健康检查
                use crate::models::{PaintOperation, Pos, Rgb};

                let invalid_token = "0000000-00-0000-0000-00000000"; // 无效的 UUID
                let invalid_uid = 0u32; // 无效的 UID
                let pos = Pos { x: 0, y: 0 };
                let color = Rgb { r: 0, g: 0, b: 0 };
                let paint_id = rand::random::<u32>();

                let operation = PaintOperation {
                    pos,
                    color,
                    token_uid: invalid_uid,
                    token: invalid_token.to_string(),
                    paint_id,
                };

                let binary_data = operation.to_binary();

                // 注册响应接收器
                let response_rx = response_tracker_clone.register_request(paint_id).await;

                // 通过 actor 发送消息
                let (send_response_tx, _) = oneshot::channel(); // 不关心发送结果
                let message = WsActorMessage::Send(SendRequest::SendBinary {
                    data: binary_data,
                    response_tx: send_response_tx,
                });

                if actor_sender.send(message).is_err() {
                    // 发送失败，连接可能已断开
                    // 发送 MarkDisconnected 消息来标记连接断开
                    let _ = actor_sender.send(WsActorMessage::Connection(
                        ConnectionRequest::MarkDisconnected,
                    ));
                    continue;
                }

                // 等待响应，设置较短的超时时间
                match tokio::time::timeout(std::time::Duration::from_secs(5), response_rx).await {
                    Ok(Ok(result)) => {
                        // 收到响应，检查是否是预期的 "Token 无效" 状态
                        if result.status != PaintStatus::InvalidToken {
                            // 收到其他响应，可能表示连接有问题
                            warn!("健康检查失败：收到意外响应状态 {:?}", result.status);
                            let _ = actor_sender.send(WsActorMessage::Connection(
                                ConnectionRequest::MarkDisconnected,
                            ));
                        } else {
                            debug!("健康检查成功：收到预期的无效 Token 响应");
                        }
                    }
                    Ok(Err(_)) => {
                        // 响应通道关闭，说明连接有问题
                        warn!("健康检查失败：响应通道关闭");
                        let _ = actor_sender.send(WsActorMessage::Connection(
                            ConnectionRequest::MarkDisconnected,
                        ));
                    }
                    Err(_) => {
                        // 超时，说明连接有问题
                        warn!("健康检查超时：未收到响应");
                        let _ = actor_sender.send(WsActorMessage::Connection(
                            ConnectionRequest::MarkDisconnected,
                        ));
                    }
                }
            }
        });

        self.health_check_task_handle = Some(task_handle);
    }

    async fn start_reconnect_task(&mut self) {
        if let Some(handle) = &self.reconnect_task_handle {
            if !handle.is_finished() {
                debug!("重连任务已在运行，跳过");
                return;
            }
        }

        self.reconnecting.store(true, Ordering::Relaxed);
        debug!("启动自动重连任务");

        let sender = self.sender.clone();
        let reconnect_manager = self.reconnect_manager.clone();
        let reconnecting_clone = self.reconnecting.clone();

        let task = tokio::spawn(async move {
            loop {
                let mut rm = reconnect_manager.lock().await;
                if !rm.should_reconnect() {
                    debug!("重连被禁用，停止重连任务");
                    reconnecting_clone.store(false, Ordering::Relaxed);
                    break;
                }

                let delay = rm.next_delay().await;
                drop(rm);

                debug!("等待 {:?} 后尝试重连...", delay);
                tokio::time::sleep(delay).await;

                let (response_tx, response_rx) = oneshot::channel();
                let message = WsActorMessage::Connection(ConnectionRequest::Connect(response_tx));
                if sender.send(message).is_err() {
                    error!(
                        "Actor channel closed, cannot attempt reconnect. Stopping reconnect task."
                    );
                    reconnecting_clone.store(false, Ordering::Relaxed);
                    break;
                }

                match response_rx.await {
                    Ok(Ok(())) => {
                        debug!("重连成功，退出重连任务");
                        reconnect_manager.lock().await.reset().await;
                        break;
                    }
                    Ok(Err(e)) => {
                        warn!("重连尝试失败: {}", e);
                    }
                    Err(_) => {
                        error!("Actor response channel closed during reconnect. Stopping reconnect task.");
                        reconnecting_clone.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });

        self.reconnect_task_handle = Some(task);
    }
}

/// 基于 Actor 模型的 WebSocket 提供者
#[derive(Clone)]
pub struct AsyncWsProvider {
    #[allow(dead_code)]
    config: Arc<Config>,
    response_tracker: Arc<WsResponseTracker>,
    reconnect_manager: Arc<TokioMutex<WsReconnectManager>>,
    #[allow(dead_code)]
    rate_limiter: Arc<WsRateLimiter>,
    message_task_ready: Arc<Notify>,
    reconnecting: Arc<AtomicBool>,
    pending_packets: Arc<RwLock<VecDeque<Vec<u8>>>>,
    /// 用于向 Actor 发送消息的发送端
    sender: mpsc::UnboundedSender<WsActorMessage>,
    /// Actor 任务句柄
    #[allow(dead_code)]
    actor_task_handle: Arc<TokioMutex<Option<JoinHandle<()>>>>,
}

impl AsyncWsProvider {
    /// Create a new WebSocket provider based on Actor model
    pub async fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        let response_tracker = Arc::new(WsResponseTracker::new());
        let reconnect_manager = Arc::new(TokioMutex::new(WsReconnectManager::default()));
        let rate_limiter = Arc::new(WsRateLimiter::new(256));
        let message_task_ready = Arc::new(Notify::new());
        let reconnecting = Arc::new(AtomicBool::new(false));
        let pending_packets = Arc::new(RwLock::new(VecDeque::new()));

        let (sender, receiver) = mpsc::unbounded_channel();

        let mut actor = WsActor::new(
            config.clone(),
            response_tracker.clone(),
            reconnect_manager.clone(),
            rate_limiter.clone(),
            message_task_ready.clone(),
            reconnecting.clone(),
            pending_packets.clone(),
            receiver,
            sender.clone(),
        );

        let actor_handle = tokio::spawn(async move {
            actor.run().await;
        });

        Ok(Self {
            config,
            response_tracker,
            reconnect_manager,
            rate_limiter,
            message_task_ready,
            reconnecting,
            pending_packets,
            sender,
            actor_task_handle: Arc::new(TokioMutex::new(Some(actor_handle))),
        })
    }

    /// Connect to the WebSocket server
    pub async fn connect(&self) -> Result<(), PaintboardError> {
        debug!("开始连接到 WebSocket 服务器: {}", self.config.ws_url);

        let (response_tx, response_rx) = oneshot::channel();
        let message = WsActorMessage::Connection(ConnectionRequest::Connect(response_tx));

        self.sender
            .send(message)
            .map_err(|_| PaintboardError::Internal("Actor channel closed".to_string()))?;

        let result = response_rx
            .await
            .map_err(|_| PaintboardError::Internal("Actor response channel closed".to_string()))?;

        if result.is_ok() {
            debug!("等待后台消息处理任务启动...");
            tokio::time::timeout(Duration::from_secs(5), self.message_task_ready.notified())
                .await
                .map_err(|_| PaintboardError::timeout())?;
            debug!("后台消息处理任务已启动");
        }

        result
    }

    /// Check if the WebSocket connection is established
    pub async fn is_connected(&self) -> bool {
        let (response_tx, response_rx) = oneshot::channel();
        let message = WsActorMessage::Connection(ConnectionRequest::IsConnected(response_tx));

        let _ = self.sender.send(message);

        match tokio::time::timeout(Duration::from_millis(3000), response_rx).await {
            Ok(Ok(result)) => result,
            _ => false,
        }
    }

    /// Send binary data through the WebSocket connection
    async fn send_binary(&self, data: Vec<u8>) -> Result<(), Report> {
        let (response_tx, response_rx) = oneshot::channel();
        let message = WsActorMessage::Send(SendRequest::SendBinary { data, response_tx });

        self.sender.send(message).map_err(|_| {
            Report::new(PaintboardError::Internal(
                "Actor channel closed".to_string(),
            ))
        })?;

        response_rx.await.map_err(|_| {
            Report::new(PaintboardError::Internal(
                "Actor response channel closed".to_string(),
            ))
        })?
    }

    /// 延迟绘画
    pub async fn paint_delayed(
        &self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<PaintResult, PaintboardError> {
        if self.reconnecting.load(Ordering::Relaxed) || !self.is_connected().await {
            return Err(PaintboardError::ConnectionClosed);
        }
        let paint_id = rand::random::<u32>();
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token: token.to_string(),
            paint_id,
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

                let still_connected = self.is_connected().await;
                warn!("超时后连接状态: {}", still_connected);

                Err(PaintboardError::timeout())
            }
        }
    }

    /// Paint a pixel at the given position with the specified color using provided authentication
    pub async fn paint_with_auth(
        &self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<PaintResult, Report> {
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

        let paint_id = rand::random::<u32>();

        if self.reconnecting.load(Ordering::Relaxed) {
            debug!("paint_with_auth() - 正在重连 (paint_id: {})", paint_id);
            return Err(
                Report::new(PaintboardError::ConnectionClosed).wrap_err("连接正在重连，请稍后重试")
            );
        }

        if !self.is_connected().await {
            debug!("paint_with_auth() - 连接已断开 (paint_id: {})", paint_id);
            return Err(
                Report::new(PaintboardError::ConnectionClosed).wrap_err("连接已断开，请稍后重试")
            );
        }

        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token: token.to_string(),
            paint_id,
        };

        let binary_data = operation.to_binary();

        if binary_data.len() > MAX_PACKET_SIZE {
            return Err(Report::new(PaintboardError::invalid_data(format!(
                "绘图消息大小 {} 字节超过限制 {} 字节 ({}KB)",
                binary_data.len(),
                MAX_PACKET_SIZE,
                MAX_PACKET_SIZE / 1024
            ))));
        }

        trace!(
            "绘图操作二进制数据长度: {}, 前几个字节: {:?}",
            binary_data.len(),
            &binary_data[..std::cmp::min(10, binary_data.len())]
        );

        debug!("注册响应追踪器，paint_id: {}", paint_id);
        let response_rx = self.response_tracker.register_request(paint_id).await;

        debug!("准备发送绘图消息，paint_id: {}", paint_id);
        self.send_binary(binary_data).await?;

        debug!("绘图消息已发送，等待响应 (paint_id: {})", paint_id);

        match timeout(Duration::from_secs(10), response_rx).await {
            Ok(Ok(result)) => {
                debug!("成功接收到绘图结果");
                Ok(result)
            }
            Ok(Err(_)) => {
                debug!("响应通道关闭");
                Err(Report::new(PaintboardError::ResponseChannelClosed))
            }
            Err(_) => {
                warn!("等待响应超时 (paint_id: {}，超时: 10s)，清理通道", paint_id);
                let removed = self.response_tracker.remove_request(paint_id).await;
                debug!("清理响应通道结果: {}", removed);

                let still_connected = self.is_connected().await;
                warn!("超时后连接状态: {}", still_connected);

                Err(Report::new(PaintboardError::timeout()))
            }
        }
    }

    /// Send multiple paint operations together using sticky packet mechanism, without waiting for responses
    pub async fn paint_batch_with_auth(
        &self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        let ops_count = operations.len();
        debug!("开始发送批量绘图请求，操作数量: {}", ops_count);

        if operations.is_empty() {
            return Ok(());
        }

        if !self.rate_limiter.check() {
            debug!("速率限制：paint_batch 请求被丢弃，操作数量: {}", ops_count);
            return Ok(());
        }

        let mut start = 0;
        while start < operations.len() {
            let mut all_binary = Vec::new();
            let mut paint_ids = Vec::new();

            let mut current_size = 0;
            let mut i = start;
            while i < operations.len() {
                let (pos, color) = operations[i];
                let paint_id = rand::random::<u32>();
                let op = PaintOperation {
                    pos,
                    color,
                    token_uid: uid,
                    token: token.to_string(),
                    paint_id,
                };
                let op_binary = op.to_binary();
                if current_size + op_binary.len() > MAX_PACKET_SIZE && !all_binary.is_empty() {
                    break;
                }
                all_binary.extend(op_binary.clone());
                paint_ids.push(paint_id);
                current_size += op_binary.len();
                i += 1;
            }

            if !all_binary.is_empty() {
                if self.reconnecting.load(Ordering::Relaxed) {
                    debug!("批量发送 - 正在重连");
                    return Err(PaintboardError::ConnectionClosed);
                }
                if !self.is_connected().await {
                    debug!("批量发送 - 连接已断开");
                    return Err(PaintboardError::ConnectionClosed);
                }

                for paint_id in &paint_ids {
                    self.response_tracker.register_request(*paint_id).await;
                }

                let send_result = self.send_binary(all_binary).await;
                if let Err(e) = send_result {
                    error!("发送批量消息失败: {:?}", e);
                    for paint_id in &paint_ids {
                        let _ = self.response_tracker.remove_request(*paint_id).await;
                    }
                    return Err(PaintboardError::Internal(e.to_string()));
                }
            }

            start = i;
        }

        Ok(())
    }

    /// Properly disconnect and clean up the WebSocket connection
    pub async fn disconnect(&self) -> Result<(), PaintboardError> {
        {
            let rm = self.reconnect_manager.lock().await;
            rm.disable_reconnect();
        }

        let (response_tx, response_rx) = oneshot::channel();
        let message = WsActorMessage::Connection(ConnectionRequest::Close(response_tx));

        let _ = self.sender.send(message);

        response_rx
            .await
            .map_err(|_| PaintboardError::Internal("Actor response channel closed".to_string()))?
    }

    /// 检查连接是否健康
    pub async fn is_healthy(&self) -> bool {
        self.is_connected().await
    }

    /// 使用多个 Token 批量绘制多个像素点。
    pub async fn paint_batch_multi_token(
        &self,
        operations: Vec<PaintOperation>,
    ) -> Result<(), PaintboardError> {
        if self.reconnecting.load(Ordering::Relaxed) || !self.is_connected().await {
            return Err(PaintboardError::ConnectionClosed);
        }

        if operations.is_empty() {
            return Ok(());
        }

        let mut combined_data = Vec::new();
        let mut paint_ids = Vec::new();

        for op in &operations {
            paint_ids.push(op.paint_id);
            combined_data.extend(op.to_binary());
        }

        if combined_data.len() > MAX_PACKET_SIZE {
            return Err(PaintboardError::invalid_data(format!(
                "批量绘制消息大小 {} 字节超过限制 {} 字节",
                combined_data.len(),
                MAX_PACKET_SIZE
            )));
        }

        // 批量绘制使用"发送即忘"(fire-and-forget)模式，不追踪响应以减少开销
        // 这样可以：
        // 1. 减少内存分配和HashMap操作
        // 2. 消除异步等待点
        // 3. 提高并发性能
        self.send_binary(combined_data)
            .await
            .map_err(|e| PaintboardError::Internal(e.to_string()))
    }
}

impl Drop for AsyncWsProvider {
    fn drop(&mut self) {
        let (response_tx, _) = oneshot::channel();
        let _ = self
            .sender
            .send(WsActorMessage::Control(ControlRequest::Shutdown(
                response_tx,
            )));
    }
}
