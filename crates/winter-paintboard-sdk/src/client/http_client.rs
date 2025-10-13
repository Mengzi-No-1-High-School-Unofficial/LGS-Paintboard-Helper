use crate::{
    error::PaintboardError, 
    models::{Board, TokenResponse, AuthRequest},
    config::Config
};
use std::sync::Arc;

/// HTTP client for Winter Paintboard API
pub struct HttpClient {
    client: reqwest::Client,
    config: Arc<Config>,
}

impl HttpClient {
    /// Create a new HTTP client
    pub fn new(config: Arc<Config>) -> Result<Self, PaintboardError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|e| PaintboardError::Network(e.to_string()))?;

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
            .map_err(|e| PaintboardError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(PaintboardError::Http(response.status().as_u16()));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| PaintboardError::Network(e.to_string()))?;

        // Validate the size: 1000 * 600 * 3 = 1,800,000 bytes
        if bytes.len() != 1_800_000 {
            return Err(PaintboardError::InvalidData);
        }

        Ok(Board::from_bytes(bytes.to_vec()))
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
            .map_err(|e| PaintboardError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(PaintboardError::Http(response.status().as_u16()));
        }

        let token_response: TokenResponse = response
            .json()
            .await
            .map_err(|e| PaintboardError::JsonParse(e.to_string()))?;

        Ok(token_response.data)
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
        let client = HttpClient::new(config).unwrap();
        
        // This test would require actual credentials to work
        // let token = client.get_token(12345, "test_access_key").await;
        // assert!(token.is_ok());
    }
}