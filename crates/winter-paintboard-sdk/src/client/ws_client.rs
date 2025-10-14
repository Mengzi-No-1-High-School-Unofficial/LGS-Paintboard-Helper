use crate::{
    error::PaintboardError, 
    models::{Rgb, Pos, PaintOperation, PaintResult, PaintStatus, ProtocolMessage, OpCode},
    config::Config,
    event::{EventBus, Event},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures::{SinkExt, StreamExt};
use url::Url;
use log::{debug, error, info, warn, trace};

/// WebSocket client for Winter Paintboard API
pub struct WsClient {
    config: Arc<Config>,
    connection: Arc<TokioMutex<Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>>,
    uid: Option<u32>,
    token: Option<String>,
    // 用于匹配请求和响应的通道映射
    response_channels: Arc<TokioMutex<HashMap<u64, oneshot::Sender<PaintResult>>>>,
    // Background task handle for message processing
    message_task_handle: Option<tokio::task::JoinHandle<()>>,
    // Flag to control reconnection attempts
    should_reconnect: Arc<std::sync::atomic::AtomicBool>,
}

impl WsClient {
    /// Create a new WebSocket client
    pub async fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        Ok(Self {
            config,
            connection: Arc::new(TokioMutex::new(None)),
            uid: None,
            token: None,
            response_channels: Arc::new(TokioMutex::new(HashMap::new())),
            message_task_handle: None,
            should_reconnect: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
    }

    /// Set the user ID and token for authentication
    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }

    /// Connect to the WebSocket server
    pub async fn connect(&mut self) -> Result<(), PaintboardError> {
        debug!("开始连接到 WebSocket 服务器: {}", self.config.ws_url);
        let url = Url::parse(&self.config.ws_url)
            .map_err(|e| {
                debug!("URL 解析失败: {}", e);
                PaintboardError::InvalidUrl(e.to_string())
            })?;
        
        match connect_async(url).await {
            Ok((ws_stream, _)) => {
                debug!("WebSocket 连接建立成功");
                
                // Store the connection
                let mut conn_guard = self.connection.lock().await;
                *conn_guard = Some(ws_stream);
                drop(conn_guard); // Release the lock so we can call start_message_processing_task
                
                // Cancel the previous message processing task if it exists
                if let Some(handle) = self.message_task_handle.take() {
                    handle.abort();
                }
                
                // Start the message processing task
                self.start_message_processing_task().await;
                
                // 发送连接打开事件到事件总线
                let event_bus = EventBus::global();
                let _ = event_bus.send(Event::ConnectionOpened);
                
                debug!("连接和消息处理任务初始化完成");
                Ok(())
            }
            Err(e) => {
                debug!("WebSocket 连接失败: {}", e);
                
                // 发送错误事件到事件总线
                let event_bus = EventBus::global();
                let _ = event_bus.send(Event::error_event(format!("WebSocket connection failed: {}", e)));
                
                Err(PaintboardError::WebSocket(e.to_string()))
            }
        }
    }

    /// Start the background task for processing incoming WebSocket messages
    async fn start_message_processing_task(&mut self) {
        log::debug!("开始启动消息处理任务");
        
        // Cancel the previous task if it exists
        if let Some(handle) = self.message_task_handle.take() {
            handle.abort();
            log::debug!("已停止之前的消息处理任务");
        }
        
        let connection_clone = self.connection.clone();
        let response_channels_clone = self.response_channels.clone();
        let should_reconnect = self.should_reconnect.clone();
        let config = self.config.clone();
        let _uid = self.uid;  // Keep for reconnection with auth
        let _token = self.token.clone();  // Keep for reconnection with auth
        
        self.message_task_handle = Some(tokio::spawn(async move {
            log::debug!("消息处理任务开始运行");
            // Track the reconnection attempts with exponential backoff
            let mut reconnect_delay = tokio::time::Duration::from_secs(1);
            let max_reconnect_delay = tokio::time::Duration::from_secs(60);
            
            loop {
                let mut last_heartbeat_time = std::time::Instant::now();
                
                loop {
                    let mut conn_guard = connection_clone.lock().await;
                    
                    if let Some(ref mut ws_stream) = *conn_guard {
                        match ws_stream.next().await {
                            Some(Ok(message)) => {
                                last_heartbeat_time = std::time::Instant::now(); // 更新最后收到消息的时间
                                drop(conn_guard); // Release the lock before processing
                                match message {
                                    Message::Binary(data) => {
                                        trace!("收到二进制消息，长度: {}", data.len());
                                        // Parse the binary message
                                        if let Ok(protocol_msg) = ProtocolMessage::parse(&data) {
                                            match protocol_msg {
                                                ProtocolMessage::HeartbeatPing => {
                                                    trace!("收到服务器心跳 PING");
                                                    // 发送心跳事件到事件总线
                                                    let event_bus = EventBus::global();
                                                    let _ = event_bus.send(Event::HeartbeatEvent);
                                                    
                                                    // Respond to heartbeat ping
                                                    let pong_msg = vec![OpCode::HeartbeatPong as u8];
                                                    trace!("发送心跳 PONG 响应");
                                                    let mut send_conn_guard = connection_clone.lock().await;
                                                    if let Some(ref mut ws_stream) = *send_conn_guard {
                                                        match ws_stream.send(Message::Binary(pong_msg)).await {
                                                            Ok(_) => {
                                                                trace!("心跳 PONG 发送成功");
                                                            }
                                                            Err(e) => {
                                                                error!("心跳 PONG 发送失败: {}", e);
                                                                
                                                                // 发送错误事件到事件总线
                                                                let event_bus = EventBus::global();
                                                                let _ = event_bus.send(Event::error_event(format!("Heartbeat PONG send failed: {}", e)));
                                                            }
                                                        }
                                                    }
                                                },
                                                ProtocolMessage::PaintResult { drawing_id, status } => {
                                                    trace!("收到绘图结果，drawing_id: {}, status: {}", drawing_id, status);
                                                    // Find the matching pending request based on drawing_id
                                                    // For now, send to the first available channel as a workaround
                                                    // In a complete implementation, we'd need to maintain a mapping between
                                                    // the paint_id we sent and the drawing_id received back from the server
                                                    let mut channels_guard = response_channels_clone.lock().await;
                                                    // Take the first key
                                                    let first_key = channels_guard.keys().next().cloned();
                                                    if let Some(key) = first_key {
                                                        // Remove the key-value pair from the map
                                                        if let Some(tx) = channels_guard.remove(&key) {
                                                            let paint_result = PaintResult {
                                                                drawing_id,
                                                                status: PaintStatus::from(status),
                                                                message: format!("Paint result received with status: {}", status),
                                                            };
                                                            drop(channels_guard); // Release the lock before sending
                                                            trace!("发送 PaintResult 到响应通道");
                                                            let _ = tx.send(paint_result);
                                                            trace!("PaintResult 已发送");
                                                        }
                                                    } else {
                                                        debug!("没有找到匹配的响应通道");
                                                    }
                                                },
                                                ProtocolMessage::PaintEvent { pos, color } => {
                                                    // Handle paint events (for read-only clients)
                                                    debug!("Received paint event at ({}, {}) with color ({}, {}, {})", 
                                                             pos.x, pos.y, color.r, color.g, color.b);
                                                    // 发送绘图事件到事件总线（表示其他用户绘制了该像素）
                                                    let event_bus = EventBus::global();
                                                    let _ = event_bus.send(Event::other_paint_event(pos, color));
                                                },
                                                ProtocolMessage::Unknown { opcode, data } => {
                                                    warn!("Received unknown message with opcode: {}, data length: {}", opcode, data.len());
                                                },
                                                _ => {
                                                    trace!("收到其他协议消息: {:?}", protocol_msg);
                                                }
                                            }
                                        } else {
                                            debug!("无法解析二进制消息: {:?}", &data[..std::cmp::min(10, data.len())]);
                                            
                                            // 发送错误事件到事件总线
                                            let event_bus = EventBus::global();
                                            let _ = event_bus.send(Event::error_event(format!("Failed to parse binary message: {:?}", &data[..std::cmp::min(10, data.len())])));
                                        }
                                    },
                                    Message::Close(close_frame) => {
                                        // Connection closed by server
                                        info!("WebSocket 连接被服务器关闭: {:?}", close_frame);
                                        
                                        // 发送连接关闭事件到事件总线
                                        let event_bus = EventBus::global();
                                        let _ = event_bus.send(Event::ConnectionClosed);
                                        
                                        break; // Break the inner loop to attempt reconnection
                                    },
                                    Message::Text(text) => {
                                        warn!("收到意外的文本消息: {}", text);
                                        
                                        // 发送错误事件到事件总线
                                        let event_bus = EventBus::global();
                                        let _ = event_bus.send(Event::error_event(format!("Received unexpected text message: {}", text)));
                                    },
                                    Message::Ping(_) => {
                                        trace!("收到 WebSocket ping");
                                        // Respond to ping with pong
                                        let mut send_conn_guard = connection_clone.lock().await;
                                        if let Some(ref mut ws_stream) = *send_conn_guard {
                                            let _ = ws_stream.send(Message::Pong(vec![])).await;
                                            trace!("发送 WebSocket pong 响应");
                                        }
                                    },
                                    Message::Pong(_) => {
                                        trace!("收到 WebSocket pong");
                                    },
                                    _ => {
                                        trace!("收到其他类型的消息");
                                    }
                                }
                            },
                            Some(Err(e)) => {
                                drop(conn_guard);
                                error!("WebSocket 错误: {}", e);
                                
                                // 发送错误事件到事件总线
                                let event_bus = EventBus::global();
                                let _ = event_bus.send(Event::error_event(format!("WebSocket error: {}", e)));
                                
                                break; // Break the inner loop to attempt reconnection
                            },
                            None => {
                                drop(conn_guard);
                                // Connection closed
                                debug!("WebSocket 连接关闭 (None received)");
                                
                                // 发送连接关闭事件到事件总线
                                let event_bus = EventBus::global();
                                let _ = event_bus.send(Event::ConnectionClosed);
                                
                                break; // Break the inner loop to attempt reconnection
                            }
                        }
                    } else {
                        drop(conn_guard);
                        // Check if connection has been inactive for too long
                        if last_heartbeat_time.elapsed() > std::time::Duration::from_secs(60) {
                            warn!("连接长时间无活动，可能已断开");
                            
                            // 发送连接关闭事件到事件总线
                            let event_bus = EventBus::global();
                            let _ = event_bus.send(Event::ConnectionClosed);
                            
                            break; // Break the inner loop to attempt reconnection
                        }
                        // Wait a bit before trying to process again
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    }
                }
                
                // Check if we should continue trying to reconnect
                if !should_reconnect.load(std::sync::atomic::Ordering::Relaxed) {
                    debug!("收到停止重连信号，退出消息处理任务");
                    break;
                }
                
                // 发送连接断开事件到事件总线
                let event_bus = EventBus::global();
                let _ = event_bus.send(Event::ConnectionClosed);
                
                info!("尝试重新连接到 WebSocket 服务器...");
                
                // Wait before attempting to reconnect (exponential backoff)
                tokio::time::sleep(reconnect_delay).await;
                
                // Update the reconnect delay for next time (exponential backoff), but cap it
                reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
                
                // Try to reconnect
                // Note: This is simplified - in a real implementation, we'd need to handle reconnection differently
                // because the connection is typically owned by the WsClient instance.
                // Here we'll just signal that we need to reconnect through the event bus or by returning.
                
                // We'll try to establish a new connection here
                match Url::parse(&config.ws_url) {
                    Ok(url) => {
                        match connect_async(url).await {
                            Ok((ws_stream, _)) => {
                                debug!("WebSocket 重连成功");
                                
                                // Store the new connection
                                let mut conn_guard = connection_clone.lock().await;
                                *conn_guard = Some(ws_stream);
                                drop(conn_guard);
                                
                                // Reset the reconnection delay after successful connection
                                reconnect_delay = tokio::time::Duration::from_secs(1);
                                
                                // 发送连接打开事件到事件总线
                                let event_bus = EventBus::global();
                                let _ = event_bus.send(Event::ConnectionOpened);
                                
                                // We successfully reconnected, continue with the outer loop
                                continue;
                            }
                            Err(e) => {
                                error!("WebSocket 重连失败: {}, 将在 {:?} 后重试", e, reconnect_delay);
                            }
                        }
                    }
                    Err(e) => {
                        error!("WebSocket URL 解析失败: {}", e);
                    }
                }
                
                // If we reach here, it means reconnection attempt failed.
                // We'll continue to the next iteration which will try again after the delay.
            }
            debug!("消息处理任务结束");
        }));
    }

    /// Paint a pixel at the given position with the specified color
    pub async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        debug!("开始发送单个绘图请求，位置: ({}, {})", pos.x, pos.y);
        
        // Check if we have authentication
        let uid = self.uid.ok_or(PaintboardError::Auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::Auth("Token not set".to_string()))?;
        
        // Ensure we're connected
        {
            let conn_guard = self.connection.lock().await;
            if conn_guard.is_none() {
                debug!("连接不存在，建立新连接");
                drop(conn_guard); // 释放锁以调用 connect
                self.connect().await?;
            } else {
                debug!("连接已存在，复用连接");
            }
        }
        
        // Generate a unique paint ID
        let paint_id = rand::random::<u64>();
        debug!("生成绘图ID: {}", paint_id);
        
        // Create the paint operation - UPDATE to match the protocol format
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token,
            paint_id: paint_id as u32, // Convert to u32 to match protocol
        };
        
        // Get the binary representation
        let binary_data = operation.to_binary();
        trace!("绘图操作二进制数据长度: {}, 前几个字节: {:?}", binary_data.len(), &binary_data[..std::cmp::min(10, binary_data.len())]);
        
        // Create a one-shot channel to receive the response
        let (response_tx, response_rx) = oneshot::channel();
        
        // Store the response channel for this paint ID
        {
            let mut channels = self.response_channels.lock().await;
            channels.insert(paint_id, response_tx);
            debug!("已将响应通道添加到映射，当前通道数: {}", channels.len());
        }
        
        // Send the message
        {
            let mut conn_guard = self.connection.lock().await;
            match conn_guard.as_mut() {
                Some(ws_stream) => {
                    debug!("发送绘图消息...");
                    match ws_stream.send(Message::Binary(binary_data)).await {
                        Ok(_) => {
                            debug!("绘图消息发送成功");
                            
                            // 发送绘图事件到事件总线（表示当前实例已发送请求）
                            let event_bus = EventBus::global();
                            let _ = event_bus.send(Event::own_paint_event(pos, color));
                        }
                        Err(e) => {
                            error!("发送绘图消息失败: {}", e);
                            
                            // 发送错误事件到事件总线
                            let event_bus = EventBus::global();
                            let _ = event_bus.send(Event::error_event(format!("Failed to send paint message: {}", e)));
                            
                            return Err(PaintboardError::WebSocket(e.to_string()));
                        }
                    }
                }
                None => {
                    error!("连接不存在，发送失败");
                    
                    // 发送错误事件到事件总线
                    let event_bus = EventBus::global();
                    let _ = event_bus.send(Event::error_event("Connection not available for sending paint message".to_string()));
                    
                    return Err(PaintboardError::ConnectionClosed);
                }
            }
        }
        
        // Wait for the response with a timeout
        debug!("等待绘图结果响应...");
        match tokio::time::timeout(std::time::Duration::from_secs(10), response_rx).await {
            Ok(Ok(result)) => {
                debug!("成功接收到绘图结果");
                Ok(result)
            },
            Ok(Err(_)) => {
                debug!("响应通道关闭");
                Err(PaintboardError::ResponseChannelClosed)
            },
            Err(_) => {
                debug!("等待响应超时");
                // Remove the channel if timeout occurred
                {
                    let mut channels = self.response_channels.lock().await;
                    if channels.remove(&paint_id).is_some() {
                        debug!("已清理超时的响应通道");
                    }
                }
                Err(PaintboardError::Timeout)
            }
        }
    }

    /// Send multiple paint operations together using sticky packet mechanism, without waiting for responses
    pub async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        let ops_count = operations.len(); // 存储操作数量以便后面使用
        debug!("开始发送批量绘图请求，操作数量: {}", ops_count);
        
        if operations.is_empty() {
            return Ok(());
        }
        
        // Check if we have authentication
        let uid = self.uid.ok_or(PaintboardError::Auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::Auth("Token not set".to_string()))?;
        
        let mut all_binary_data = Vec::new(); // For sticky packet mechanism
        
        // Create all operations and collect their binary data
        for (pos, color) in &operations { // Use reference to avoid moving operations
            // Generate a unique paint ID
            let paint_id = rand::random::<u64>();
            
            // Create the paint operation - UPDATE to match the protocol format
            let operation = PaintOperation {
                pos: *pos, // Dereference the position
                color: *color, // Dereference the color
                token_uid: uid,
                token: token.clone(), // Clone token for each operation
                paint_id: paint_id as u32, // Convert to u32 to match protocol
            };
            
            // Get the binary representation and add to the sticky packet
            let binary_data = operation.to_binary();
            all_binary_data.extend(binary_data); // This is the sticky packet mechanism - concatenating all binary data
        }
        
        debug!("批量操作二进制数据总长度: {}, 操作数量: {}", all_binary_data.len(), ops_count);
        
        // Ensure we're connected
        {
            let conn_guard = self.connection.lock().await;
            if conn_guard.is_none() {
                debug!("批量发送 - 连接不存在，建立新连接");
                drop(conn_guard); // 释放锁以调用 connect
                self.connect().await?;
            } else {
                debug!("批量发送 - 连接已存在，复用连接");
            }
        }
        
        // Send all data in one sticky packet
        {
            let mut conn_guard = self.connection.lock().await;
            match conn_guard.as_mut() {
                Some(ws_stream) => {
                    debug!("发送批量消息...");
                    match ws_stream.send(Message::Binary(all_binary_data)).await {
                        Ok(_) => {
                            debug!("批量消息发送成功");
                            
                            // 发送绘图事件到事件总线（为每个操作发送一个事件，表示当前实例已发送请求）
                            let event_bus = EventBus::global();
                            for (pos, color) in &operations {
                                let _ = event_bus.send(Event::own_paint_event(*pos, *color));
                            }
                        }
                        Err(e) => {
                            error!("发送批量消息失败: {}", e);
                            
                            // 发送错误事件到事件总线
                            let event_bus = EventBus::global();
                            let _ = event_bus.send(Event::error_event(format!("Failed to send batch message: {}", e)));
                            
                            return Err(PaintboardError::WebSocket(e.to_string()));
                        }
                    }
                }
                None => {
                    error!("批量发送 - 连接不存在，发送失败");
                    
                    // 发送错误事件到事件总线
                    let event_bus = EventBus::global();
                    let _ = event_bus.send(Event::error_event("Connection not available for sending batch message".to_string()));
                    
                    return Err(PaintboardError::ConnectionClosed);
                }
            }
        }
        
        Ok(())
    }

    /// Send a heartbeat (PONG) response
    pub async fn send_heartbeat_pong(&mut self) -> Result<(), PaintboardError> {
        if self.connection.lock().await.is_none() {
            self.connect().await?;
        }
        
        let pong_message = vec![OpCode::HeartbeatPong as u8];
        
        {
            let mut conn_guard = self.connection.lock().await;
            let ws_stream = conn_guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;
            
            ws_stream
                .send(Message::Binary(pong_message))
                .await
                .map_err(|e| PaintboardError::WebSocket(e.to_string()))?;
        }
        
        Ok(())
    }

    /// Listen for events from the server
    pub async fn listen_for_events(&mut self) -> Result<ProtocolMessage, PaintboardError> {
        if self.connection.lock().await.is_none() {
            self.connect().await?;
        }
        
        {
            let mut conn_guard = self.connection.lock().await;
            let ws_stream = conn_guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;
            
            // Wait for the next message
            if let Some(msg) = ws_stream.next().await {
                let msg = msg.map_err(|e| PaintboardError::WebSocket(e.to_string()))?;
                
                match msg {
                    Message::Binary(data) => {
                        ProtocolMessage::parse(&data)
                    },
                    Message::Text(_) => {
                        Err(PaintboardError::InvalidData)
                    },
                    Message::Close(_) => {
                        Err(PaintboardError::ConnectionClosed)
                    },
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                        // Handle ping/pong and other frame types as invalid for our protocol
                        Err(PaintboardError::InvalidData)
                    }
                }
            } else {
                Err(PaintboardError::ConnectionClosed)
            }
        }
    }

    /// Properly disconnect and clean up the WebSocket connection
    pub async fn disconnect(&mut self) -> Result<(), PaintboardError> {
        // Set the reconnection flag to false to prevent reconnection attempts
        self.should_reconnect.store(false, std::sync::atomic::Ordering::Relaxed);
        
        if let Some(handle) = self.message_task_handle.take() {
            handle.abort();
        }
        
        let mut conn_guard = self.connection.lock().await;
        if let Some(mut ws_stream) = conn_guard.take() {
            use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
            let _ = ws_stream.close(Some(CloseFrame { 
                code: CloseCode::Normal,
                reason: std::borrow::Cow::Borrowed("Client disconnecting")
            })).await;
        }
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_ws_client_creation() {
        let config = Arc::new(Config::default());
        let client = WsClient::new(config).await;
        assert!(client.is_ok());
    }
}