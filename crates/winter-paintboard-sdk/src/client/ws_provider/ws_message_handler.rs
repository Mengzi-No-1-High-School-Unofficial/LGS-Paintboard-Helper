use crate::{event::EventBus, models::{ProtocolMessage, OpCode, PaintResult, PaintStatus, Pos, Rgb}};
use crate::client::ws_provider::ws_response_tracker::WsResponseTracker;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tokio_tungstenite::tungstenite::protocol::Message;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio::net::TcpStream;
use tracing::{debug, error, trace, warn};
use futures::SinkExt; // bring `send` into scope

/// 消息处理器：解析来自 WebSocket 的消息并分发处理逻辑。
/// 该模块不负责轮询（next()），而是提供单条消息的处理函数，供后台任务调用。
pub struct WsMessageHandler {
    response_tracker: Arc<WsResponseTracker>,
}

impl WsMessageHandler {
    pub fn new(response_tracker: Arc<WsResponseTracker>) -> Self {
        Self { response_tracker }
    }

    /// 处理单条从 WebSocket 接收到的消息。
    /// connection_stream 用于在需要时发送心跳/响应等回包。
    pub async fn handle_message_stream(
        &self,
        message: Message,
        connection_stream: Arc<TokioMutex<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
    ) {
        match message {
            Message::Binary(data) => {
                trace!("收到二进制消息，长度: {}", data.len());
                if let Ok(protocol_msg) = ProtocolMessage::parse(&data) {
                    match protocol_msg {
                        ProtocolMessage::HeartbeatPing => {
                            trace!("收到服务器心跳 PING");
                            let _ = EventBus::global().send(crate::event::Event::HeartbeatEvent);

                            // 回复心跳 PONG
                            let pong_msg = vec![OpCode::HeartbeatPong as u8];
                            trace!("发送心跳 PONG 响应");
                            let mut guard = connection_stream.lock().await;
                            if let Some(ref mut ws_stream) = *guard {
                                if let Err(e) = ws_stream.send(Message::Binary(pong_msg)).await {
                                    error!("心跳 PONG 发送失败: {}", e);
                                    let _ = EventBus::global().send(crate::event::Event::error_event(format!("Heartbeat PONG send failed: {}", e)));
                                } else {
                                    trace!("心跳 PONG 发送成功");
                                }
                            }
                        }

                        ProtocolMessage::PaintResult { drawing_id, status } => {
                            trace!("收到绘图结果，drawing_id: {}, status: {}", drawing_id, status);
                            let paint_result = PaintResult {
                                drawing_id,
                                status: PaintStatus::from(status),
                                message: format!("Paint result received with status: {}", status),
                            };

                            // 优先尝试按 drawing_id 匹配请求通道
                            let matched = self.response_tracker.complete_request(drawing_id as u64, paint_result.clone()).await;
                            if !matched {
                                // 回退策略：如果无法直接匹配，就完成第一个挂起的请求（保留现有行为）
                                trace!("未找到匹配的 paint_id，尝试回退到第一个挂起请求");
                                let _ = self.response_tracker.complete_first_request(paint_result).await;
                            } else {
                                trace!("已完成匹配的响应通道");
                            }
                        }

                        ProtocolMessage::PaintEvent { pos, color } => {
                            debug!("收到绘图事件: ({}, {}) color ({}, {}, {})", pos.x, pos.y, color.r, color.g, color.b);
                            let _ = EventBus::global().send(crate::event::Event::other_paint_event(pos, color));
                        }

                        ProtocolMessage::Unknown { opcode, data } => {
                            warn!("Received unknown message with opcode: {}, data length: {}", opcode, data.len());
                        }

                        other => {
                            trace!("收到其他协议消息: {:?}", other);
                        }
                    }
                } else {
                    debug!("无法解析二进制消息: {:?}", &data[..std::cmp::min(10, data.len())]);
                    let _ = EventBus::global().send(crate::event::Event::error_event(format!(
                        "Failed to parse binary message: {:?}",
                        &data[..std::cmp::min(10, data.len())]
                    )));
                }
            }

            Message::Close(close_frame) => {
                debug!("收到 Close: {:?}", close_frame);
                let _ = EventBus::global().send(crate::event::Event::ConnectionClosed);
            }

            Message::Text(text) => {
                warn!("收到意外的文本消息: {}", text);
                let _ = EventBus::global().send(crate::event::Event::error_event(format!(
                    "Received unexpected text message: {}", text
                )));
            }

            Message::Ping(payload) => {
                trace!("收到 WebSocket ping");
                let mut guard = connection_stream.lock().await;
                if let Some(ref mut ws_stream) = *guard {
                    let _ = ws_stream.send(Message::Pong(payload)).await;
                    trace!("发送 WebSocket pong 响应");
                }
            }

            Message::Pong(_) => {
                trace!("收到 WebSocket pong");
            }

            _ => {
                trace!("收到其他类型的消息");
            }
        }
    }
}