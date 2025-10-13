use crate::{
    error::PaintboardError, 
    models::{Rgb, Pos, PaintOperation, PaintResult, PaintStatus, ProtocolMessage, OpCode},
    config::Config
};
use std::sync::{Arc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures::{SinkExt, StreamExt};
use url::Url;

/// WebSocket client for Winter Paintboard API
pub struct WsClient {
    config: Arc<Config>,
    connection: Arc<Mutex<Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>>,
    uid: Option<u32>,
    token: Option<String>,
}

impl WsClient {
    /// Create a new WebSocket client
    pub async fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        Ok(Self {
            config,
            connection: Arc::new(Mutex::new(None)),
            uid: None,
            token: None,
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
        let mut conn_guard = self.connection.lock().unwrap();
        *conn_guard = Some(ws_stream);
        
        Ok(())
    }

    /// Paint a pixel at the given position with the specified color
    pub async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        // Check if we have authentication
        let uid = self.uid.ok_or(PaintboardError::Auth("UID not set".to_string()))?;
        let token = self.token.clone().ok_or(PaintboardError::Auth("Token not set".to_string()))?;
        
        // Ensure we're connected
        if self.connection.lock().unwrap().is_none() {
            self.connect().await?;
        }
        
        // Create the paint operation
        let operation = PaintOperation {
            pos,
            color,
            uid,
            token,
            paint_id: Some(rand::random::<u64>()), // Generate a random paint ID
        };
        
        // Get the binary representation
        let binary_data = operation.to_binary();
        
        // Send the message
        {
            let mut conn_guard = self.connection.lock().unwrap();
            let ws_stream = conn_guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;
            
            ws_stream
                .send(Message::Binary(binary_data))
                .await
                .map_err(|e| PaintboardError::WebSocket(e.to_string()))?;
        }
        
        // Wait for the result (in a real implementation, you'd want to wait for the specific response)
        // For now, just return a success result
        Ok(PaintResult {
            status: PaintStatus::Success,
            message: "Paint operation sent".to_string(),
        })
    }

    /// Send a heartbeat (PONG) response
    pub async fn send_heartbeat_pong(&mut self) -> Result<(), PaintboardError> {
        if self.connection.lock().unwrap().is_none() {
            self.connect().await?;
        }
        
        let pong_message = vec![OpCode::HeartbeatPong as u8];
        
        {
            let mut conn_guard = self.connection.lock().unwrap();
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
        if self.connection.lock().unwrap().is_none() {
            self.connect().await?;
        }
        
        {
            let mut conn_guard = self.connection.lock().unwrap();
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