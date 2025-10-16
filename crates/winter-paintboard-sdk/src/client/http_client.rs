use crate::{
    error::PaintboardError, 
    models::{Board, TokenResponse, AuthRequest},
    config::Config
};
use std::sync::Arc;

/// HTTP client for Winter Paintboard API
pub struct HttpProvider {
    client: reqwest::Client,
    config: Arc<Config>,
}

impl HttpProvider {
    /// Create a new HTTP client
    pub fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| PaintboardError::network(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// Get the current board data (1000x600 pixels as RGB bytes)
    pub async fn get_board(&self) -> Result<Board, PaintboardError> {
        let url = format!("{}/api/paintboard/getboard", self.config.api_base_url);
        
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| PaintboardError::network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(PaintboardError::Http(response.status().as_u16()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| PaintboardError::network(e.to_string()))?;

        // Validate the size: 1000 * 600 * 3 = 1,800,000 bytes
        if bytes.len() != 1_800_000 {
            return Err(PaintboardError::invalid_data(format!("Board data size mismatch: expected 1,800,000 bytes, got {}", bytes.len())));
        }

        // Perform potentially blocking operation in spawn_blocking to avoid blocking the async runtime
        let bytes_vec = bytes.to_vec();
        let board = tokio::task::spawn_blocking(move || {
            Board::from_bytes(bytes_vec)
        }).await
        .map_err(|e| PaintboardError::Internal(format!("Task execution failed: {}", e)))?;

        Ok(board)
    }

    /// Get a token using UID and access key
    pub async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        let url = format!("{}/api/auth/gettoken", self.config.api_base_url);
        
        let auth_request = AuthRequest {
            uid,
            access_key: access_key.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&auth_request)
            .send()
            .await
            .map_err(|e| PaintboardError::network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(PaintboardError::Http(response.status().as_u16()));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| PaintboardError::JsonParse(e.to_string()))?;

        if token_response.status_code != 200 {
            return Err(PaintboardError::auth(format!("API returned error status {}: token data", token_response.status_code)));
        }

        Ok(token_response.data.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[tokio::test]
    #[ignore] // Ignore this test as it requires a real API endpoint
    async fn test_get_token() {
        let config = Arc::new(Config::default());
        let client = HttpProvider::new(config).unwrap();
        
        // This test would require actual credentials to work
        // let token = client.get_token(12345, "test_access_key").await;
        // assert!(token.is_ok());
    }
}