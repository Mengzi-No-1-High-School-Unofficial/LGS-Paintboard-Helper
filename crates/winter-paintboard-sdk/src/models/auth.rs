use serde::{Deserialize, Serialize};

/// Request for getting a token
#[derive(Debug, Serialize)]
pub struct AuthRequest {
    pub uid: u32,
    pub access_key: String,
}

/// Inner data structure for token response
#[derive(Debug, Deserialize)]
pub struct TokenData {
    pub token: String,
}

/// Response for getting a token
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub statusCode: i32,
    pub data: TokenData,
}