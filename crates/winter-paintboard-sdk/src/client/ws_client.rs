use crate::{
    error::PaintboardError, 
    models::{Rgb, Pos, PaintOperation, PaintResult, PaintStatus, ProtocolMessage, OpCode},
    config::Config
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures::{SinkExt, StreamExt};
use url::Url;

/// WebSocket client for Winter Paintboard API
pub struct WsClient {
    config: Arc<Config>,
    connection: Arc<TokioMutex<Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>>,
    uid: Option<u32>,
    token: Option<String>,
    // 用于匹配请求和响应的通道映射
    response_channels: Arc<TokioMutex<HashMap<u64, oneshot::Sender<PaintResult>>>>,
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
        })
    }

    /// Set the user ID and token for authentication
    pub fn set_auth(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }

    /// Connect to the WebSocket server
    pub async fn connect(&mut self) -> Result<(), PaintboardError> {
        let url = Url::parse(&self.config.ws_url)
            .map_err(|e| PaintboardError::InvalidUrl(e.to_string()))?;
        
        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| PaintboardError::WebSocket(e.to_string()))?;
        
        // Store the connection
        let mut conn_guard = self.connection.lock().await;
        *conn_guard = Some(ws_stream);
        
        Ok(())
    }

    /// Paint a pixel at the given position with the specified color
    pub async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        // Check if we have authentication
        let uid = self.uid.ok_or(PaintboardError::Auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::Auth("Token not set".to_string()))?;
        
        // Ensure we're connected
        if self.connection.lock().await.is_none() {
            self.connect().await?;
        }
        
        // Generate a unique paint ID
        let paint_id = rand::random::<u64>();
        
        // Create the paint operation - UPDATE to match the protocol format
        let operation = PaintOperation {
            pos,
            color,
            token_uid: uid,
            token,
            paint_id: paint_id as u32, // Convert to u32 to match protocol
        };
        
        // Create a one-shot channel to receive the response
        let (response_tx, response_rx) = oneshot::channel();
        
        // Store the response channel for this paint ID
        {
            let mut channels = self.response_channels.lock().await;
            channels.insert(paint_id, response_tx);
        }
        
        // Get the binary representation
        let binary_data = operation.to_binary();
        
        // Send the message
        {
            let mut conn_guard = self.connection.lock().await;
            let ws_stream = conn_guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;
            
            ws_stream
                .send(Message::Binary(binary_data))
                .await
                .map_err(|e| PaintboardError::WebSocket(e.to_string()))?;
        }
        
        // Wait for the response with a timeout
        match tokio::time::timeout(std::time::Duration::from_secs(10), response_rx).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(_)) => Err(PaintboardError::ResponseChannelClosed),
            Err(_) => {
                // Remove the channel if timeout occurred
                {
                    let mut channels = self.response_channels.lock().await;
                    channels.remove(&paint_id);
                }
                Err(PaintboardError::Timeout)
            }
        }
    }

    /// Send multiple paint operations together using sticky packet mechanism, without waiting for responses
    pub async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        if operations.is_empty() {
            return Ok(());
        }
        
        // Check if we have authentication
        let uid = self.uid.ok_or(PaintboardError::Auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::Auth("Token not set".to_string()))?;
        
        let mut all_binary_data = Vec::new(); // For sticky packet mechanism
        
        // Create all operations and collect their binary data
        for (pos, color) in operations {
            // Generate a unique paint ID
            let paint_id = rand::random::<u64>();
            
            // Create the paint operation - UPDATE to match the protocol format
            let operation = PaintOperation {
                pos,
                color,
                token_uid: uid,
                token: token.clone(), // Clone token for each operation
                paint_id: paint_id as u32, // Convert to u32 to match protocol
            };
            
            // Get the binary representation and add to the sticky packet
            let binary_data = operation.to_binary();
            all_binary_data.extend(binary_data); // This is the sticky packet mechanism - concatenating all binary data
        }
        
        // Send all data in one sticky packet
        {
            let mut conn_guard = self.connection.lock().await;
            let ws_stream = conn_guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;
            
            ws_stream
                .send(Message::Binary(all_binary_data))
                .await
                .map_err(|e| PaintboardError::WebSocket(e.to_string()))?;
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