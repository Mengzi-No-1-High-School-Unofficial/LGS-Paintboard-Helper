use serde::{Deserialize, Serialize};

/// Request for getting a token
#[derive(Debug, Serialize)]
pub struct AuthRequest {
    pub uid: u32,
    pub access_key: String,
}

/// Response for getting a token
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub data: String,
}