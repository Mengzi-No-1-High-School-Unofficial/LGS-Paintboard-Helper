use crate::basic_client::ws_provider::ws_connection::WsConnection;
use crate::basic_client::ws_provider::ws_reconnect::WsReconnectManager;
use crate::basic_client::ws_provider::ws_response_tracker::WsResponseTracker;
use crate::{
    config::Config,
    models::ProtocolMessage,
};
use color_eyre::eyre::Context;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::{Mutex as TokioMutex, Notify};
use tokio::task::JoinHandle;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tracing::{debug, error, warn};

/// 消息处理器：管理 WebSocket 消息处理循环和连接状态
pub struct WsMessageHandler {
    response_tracker: Arc<WsResponseTracker>,
}

impl WsMessageHandler {
    pub fn new(response_tracker: Arc<WsResponseTracker>) -> Self {
        Self { response_tracker }
    }

    /// 处理单条从 WebSocket 接收到的消息
    pub async fn handle_message_stream(
        &self,
        message: tokio_tungstenite::tungstenite::protocol::Message,
        connection_stream: Arc<TokioMutex<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    ) {
        match message {
            Message::Binary(data) => {
                let messages = ProtocolMessage::parse_batch(&data);

                if let Ok(messages) = messages {
                    for message in messages {
                        self.handle_single_message(message, connection_stream.clone())
                            .await;
                    }
                } else {
                    error!(
                        "❌ 无法解析二进制消息，前10字节: {:?}",
                        &data[..std::cmp::min(10, data.len())]
                    );
                }
            }

            Message::Close(close_frame) => {
                warn!("🔌 收到连接关闭: {:?}", close_frame);
            }

            Message::Text(text) => {
                warn!("⚠️  收到意外的文本消息: {}", text);
            }

            Message::Ping(payload) => {
                debug!("🔔 收到 WebSocket ping (payload: {} 字节)", payload.len());
                let mut guard = connection_stream.lock().await;
                if let Some(ref mut ws_stream) = *guard {
                    if let Err(e) = ws_stream.send(Message::Pong(payload)).await {
                        error!("❌ 发送 WebSocket pong 响应失败: {}", e);
                    } else {
                        debug!("✅ 发送 WebSocket pong 响应成功");
                    }
                }
            }

            Message::Pong(_) => {
                debug!("🔔 收到 WebSocket pong");
            }

            _ => {
                debug!("📨 收到其他类型的消息");
            }
        }
    }

    async fn handle_single_message(
        &self,
        protocol_msg: ProtocolMessage,
        connection_stream: Arc<TokioMutex<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    ) {
        match protocol_msg {
            ProtocolMessage::HeartbeatPing => {
                debug!("💓 收到服务器心跳 PING");

                // 回复心跳 PONG
                use crate::models::OpCode;
                let pong_msg = vec![OpCode::HeartbeatPong as u8];
                debug!("💓 发送心跳 PONG 响应");
                let mut guard = connection_stream.lock().await;
                if let Some(ref mut ws_stream) = *guard {
                    if let Err(e) = ws_stream.send(Message::Binary(pong_msg)).await {
                        error!("❌ 心跳 PONG 发送失败: {}", e);
                    } else {
                        debug!("✅ 心跳 PONG 发送成功");
                    }
                }
            }

            ProtocolMessage::PaintResult { drawing_id, status } => {
                debug!(
                    "🎨 收到绘图结果: drawing_id={}, status=0x{:02x}",
                    drawing_id, status
                );
                use crate::models::{PaintResult, PaintStatus};
                let paint_status = PaintStatus::from(status);
                debug!("🎨 绘图状态: {:?}", paint_status);

                let paint_result = PaintResult {
                    drawing_id,
                    status: paint_status,
                    message: format!("Paint result received with status: 0x{:02x}", status),
                };

                // 优先尝试按 drawing_id 匹配请求通道
                let matched = self
                    .response_tracker
                    .complete_request(drawing_id, paint_result.clone())
                    .await;

                if !matched {
                    debug!("⚠️  未找到匹配的 paint_id ({}),", drawing_id)
                } else {
                    debug!("✅ 已完成匹配的响应通道 (drawing_id: {})", drawing_id);
                }
            }

            ProtocolMessage::PaintEvent { .. } => {
                // 不再需要处理这个事件
            }

            ProtocolMessage::Unknown { opcode, data } => {
                warn!(
                    "⚠️  收到未知消息类型: opcode=0x{:02x}, 数据长度: {} 字节",
                    opcode,
                    data.len()
                );
            }

            other => {
                // debug!("📨 收到其他协议消息: {:?}", other);
            }
        }
    }

    /// 启动消息处理任务，包括消息处理循环和重连逻辑
    pub async fn start_message_processing_task(
        &self,
        connection: Arc<WsConnection>,
        reconnect_manager: Arc<TokioMutex<WsReconnectManager>>,
        config: Arc<Config>,
        ready_notify: Arc<Notify>,
    ) -> JoinHandle<()> {
        let stream_arc = connection.stream();
        let handler = self.clone();
        let reconnect_manager_clone = reconnect_manager.clone();
        let connection_clone = connection.clone();
        let config_clone = config.clone();
        let ready_notify_clone = ready_notify.clone();

        tokio::spawn(async move {
            debug!("消息处理任务开始运行");

            // 发送启动完成信号
            ready_notify_clone.notify_one();
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

                                break; // break inner loop to attempt reconnection
                            }
                        }
                    } else {
                        drop(guard);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }

                // Check if reconnection is allowed
                let rm = reconnect_manager_clone.lock().await;
                if !rm.should_reconnect() {
                    debug!("收到停止重连信号，退出消息处理任务");
                    break;
                }

                let _ = reconnect_manager_clone
                    .lock()
                    .await
                    .reconnect(connection_clone.clone())
                    .await
                    .wrap_err("在 `WsMessageHandler` 中尝试重联失败")
                    .map_err(|e| eprintln!("{}", e));
            }
            debug!("消息处理任务结束");
        })
    }
}

impl Clone for WsMessageHandler {
    fn clone(&self) -> Self {
        Self {
            response_tracker: self.response_tracker.clone(),
        }
    }
}
