mod http_client;
mod ws_client;
mod batch_client;

pub use http_client::HttpClient;
pub use ws_client::WsClient;
pub use batch_client::BatchClient;

use crate::{
    error::PaintboardError, 
    models::{Board, Rgb, Pos}, 
    config::Config
};
use std::sync::Arc;

/// Main client for interacting with the Winter Paintboard API
pub struct PaintboardClient {
    http_client: HttpClient,
    ws_client: Option<WsClient>, // WebSocket client is optional and created on demand
    config: Arc<Config>,
}

impl PaintboardClient {
    /// Create a new PaintboardClient with the given configuration
    pub async fn new(config: Config) -> Result<Self, PaintboardError> {
        let config = Arc::new(config);
        let http_client = HttpClient::new(config.clone())?;
        
        Ok(Self {
            http_client,
            ws_client: None,
            config,
        })
    }

    /// Get the current board data
    pub async fn get_board(&self) -> Result<Board, PaintboardError> {
        self.http_client.get_board().await
    }

    /// Get a token using UID and access key
    pub async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        self.http_client.get_token(uid, access_key).await
    }

    /// Paint a pixel at the given position with the specified color
    pub async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<crate::models::PaintResult, PaintboardError> {
        // Initialize WebSocket client if not already created
        if self.ws_client.is_none() {
            let ws_client = WsClient::new(self.config.clone()).await?;
            self.ws_client = Some(ws_client);
        }
        
        // Get a mutable reference to the WebSocket client and call paint
        if let Some(ref mut ws_client) = self.ws_client {
            // For now, we'll return a dummy result; actual implementation will be in WsClient
            ws_client.paint(pos, color).await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        }
    }
}