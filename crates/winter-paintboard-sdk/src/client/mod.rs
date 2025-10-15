#[macro_use]
mod connection_pool;
mod http_client;
mod ws_provider;
mod batch_client;
mod paintboard_client_trait;
mod factory;

pub use http_client::HttpProvider;
pub use ws_provider::WsProvider;
pub use batch_client::BatchHelper;
pub use paintboard_client_trait::PaintboardClientTrait;
pub use connection_pool::{PoolClient, ConnectionGuard, PoolMetrics, start_monitoring_task};
pub use factory::{ClientType, create_client_by_type};

use crate::{
    error::PaintboardError, 
    models::{Board, Rgb, Pos}, 
    config::Config
};
use std::sync::Arc;

use async_trait::async_trait;

/// Basic client for interacting with the Winter Paintboard API
pub struct BasicClient {
    http_client: HttpProvider,
    ws_client: Option<WsProvider>, // WebSocket client is optional and created on demand
    config: Arc<Config>,
    uid: Option<u32>,
    token: Option<String>,
}

impl BasicClient {
    /// Create a new BasicClient with the given configuration
    pub async fn new_impl(config: Config) -> Result<Self, PaintboardError> {
        let config = Arc::new(config);
        let http_client = HttpProvider::new(config.clone())?;
        
        Ok(Self {
            http_client,
            ws_client: None,
            config,
            uid: None,
            token: None,
        })
    }

    /// Set the authentication credentials (UID and token)
    pub fn set_auth_impl(&mut self, uid: u32, token: String) {
        self.uid = Some(uid);
        self.token = Some(token);
    }

    /// Get the current board data
    pub async fn get_board_impl(&self) -> Result<Board, PaintboardError> {
        self.http_client.get_board().await
    }

    /// Get a token using UID and access key
    pub async fn get_token_impl(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        self.http_client.get_token(uid, access_key).await
    }

    /// Paint a pixel at the given position with the specified color
    pub async fn paint_impl(&mut self, pos: Pos, color: Rgb) -> Result<crate::models::PaintResult, PaintboardError> {
        // Initialize WebSocket client if not already created
        if self.ws_client.is_none() {
            let mut ws_client = WsProvider::new(self.config.clone()).await?;
            
            // Set authentication if available
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                ws_client.set_auth(uid, token.clone());
            }
            
            self.ws_client = Some(ws_client);
        }
        
        // Get a mutable reference to the WebSocket client and call paint
        if let Some(ref mut ws_client) = self.ws_client {
            ws_client.paint(pos, color).await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        }
    }

    /// Paint multiple pixels at once using batch operation (sticky packet, no response waiting)
    pub async fn paint_batch_impl(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        // Initialize WebSocket client if not already created
        if self.ws_client.is_none() {
            let mut ws_client = WsProvider::new(self.config.clone()).await?;
            
            // Set authentication if available
            if let (Some(uid), Some(token)) = (self.uid, self.token.as_ref()) {
                ws_client.set_auth(uid, token.clone());
            }
            
            self.ws_client = Some(ws_client);
        }
        
        // Get a mutable reference to the WebSocket client and call paint_batch
        if let Some(ref mut ws_client) = self.ws_client {
            ws_client.paint_batch(operations).await
        } else {
            // This should not happen, but added for safety
            Err(PaintboardError::ClientNotInitialized)
        }
    }
}

#[async_trait]
impl PaintboardClientTrait for BasicClient {
    async fn new(config: Config) -> Result<Self, PaintboardError> 
    where 
        Self: Sized 
    {
        Self::new_impl(config).await
    }

    fn set_auth(&mut self, uid: u32, token: String) {
        self.set_auth_impl(uid, token);
    }

    async fn get_board(&self) -> Result<Board, PaintboardError> {
        self.get_board_impl().await
    }

    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        self.get_token_impl(uid, access_key).await
    }

    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<crate::models::PaintResult, PaintboardError> {
        self.paint_impl(pos, color).await
    }

    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        self.paint_batch_impl(operations).await
    }
}