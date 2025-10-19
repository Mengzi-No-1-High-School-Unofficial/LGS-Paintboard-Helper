use crate::{event::EventBus, models::{ProtocolMessage, OpCode, PaintResult, PaintStatus, Pos, Rgb}};
use crate::basic_client::ws_provider::ws_response_tracker::WsResponseTracker;
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
                // debug!("📨 收到二进制消息，长度: {} 字节，前10字节: {:?}",
                    // data.len(),
                    // &data[..std::cmp::min(10, data.len())]);

                if let Ok(protocol_msg) = ProtocolMessage::parse(&data) {
                    match protocol_msg {
                        ProtocolMessage::HeartbeatPing => {
                            debug!("💓 收到服务器心跳 PING");
                            let _ = EventBus::global().send(crate::event::Event::HeartbeatEvent);

                            // 回复心跳 PONG
                            let pong_msg = vec![OpCode::HeartbeatPong as u8];
                            debug!("💓 发送心跳 PONG 响应");
                            let mut guard = connection_stream.lock().await;
                            if let Some(ref mut ws_stream) = *guard {
                                if let Err(e) = ws_stream.send(Message::Binary(pong_msg)).await {
                                    error!("❌ 心跳 PONG 发送失败: {}", e);
                                    let _ = EventBus::global().send(crate::event::Event::error_event(format!("Heartbeat PONG send failed: {}", e)));
                                } else {
                                    debug!("✅ 心跳 PONG 发送成功");
                                }
                            }
                        }

                        ProtocolMessage::PaintResult { drawing_id, status } => {
                            debug!("🎨 收到绘图结果: drawing_id={}, status=0x{:02x}", drawing_id, status);
                            let paint_status = PaintStatus::from(status);
                            debug!("🎨 绘图状态: {:?}", paint_status);
                            
                            let paint_result = PaintResult {
                                drawing_id,
                                status: paint_status,
                                message: format!("Paint result received with status: 0x{:02x}", status),
                            };

                            // 优先尝试按 drawing_id 匹配请求通道
                            let matched = self.response_tracker.complete_request(drawing_id as u64, paint_result.clone()).await;
                            if !matched {
                                // 回退策略：如果无法直接匹配，就完成第一个挂起的请求（保留现有行为）
                                debug!("⚠️  未找到匹配的 paint_id ({}), 尝试回退到第一个挂起请求", drawing_id);
                                let fallback_matched = self.response_tracker.complete_first_request(paint_result).await;
                                if fallback_matched {
                                    debug!("✅ 回退策略成功，已完成第一个挂起请求");
                                } else {
                                    warn!("❌ 回退策略失败，没有挂起的请求");
                                }
                            } else {
                                debug!("✅ 已完成匹配的响应通道 (drawing_id: {})", drawing_id);
                            }
                        }

                        ProtocolMessage::PaintEvent { pos, color } => {
                            // debug!("🖌️  收到其他用户绘图事件: ({}, {}) RGB({}, {}, {})",
                                // pos.x, pos.y, color.r, color.g, color.b);
                            let _ = EventBus::global().send(crate::event::Event::other_paint_event(pos, color));
                        }

                        ProtocolMessage::Unknown { opcode, data } => {
                            warn!("⚠️  收到未知消息类型: opcode=0x{:02x}, 数据长度: {} 字节", opcode, data.len());
                        }

                        other => {
                            // debug!("📨 收到其他协议消息: {:?}", other);
                        }
                    }
                } else {
                    error!("❌ 无法解析二进制消息，前10字节: {:?}", &data[..std::cmp::min(10, data.len())]);
                    let _ = EventBus::global().send(crate::event::Event::error_event(format!(
                        "Failed to parse binary message: {:?}",
                        &data[..std::cmp::min(10, data.len())]
                    )));
                }
            }

            Message::Close(close_frame) => {
                warn!("🔌 收到连接关闭: {:?}", close_frame);
                let _ = EventBus::global().send(crate::event::Event::ConnectionClosed);
            }

            Message::Text(text) => {
                warn!("⚠️  收到意外的文本消息: {}", text);
                let _ = EventBus::global().send(crate::event::Event::error_event(format!(
                    "Received unexpected text message: {}", text
                )));
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
}