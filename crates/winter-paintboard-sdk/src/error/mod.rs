use thiserror::Error;

/// Errors that can occur when using the Winter Paintboard SDK
#[derive(Error, Debug)]
pub enum PaintboardError {
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("WebSocket error: {0}")]
    WebSocket(String),
    
    #[error("JSON parsing error: {0}")]
    JsonParse(String),
    
    #[error("Invalid coordinate")]
    InvalidCoordinate,
    
    #[error("Index out of range")]
    IndexOutOfRange,
    
    #[error("Invalid data format")]
    InvalidData,
    
    #[error("HTTP error: {0}")]
    Http(u16),
    
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    
    #[error("Client not initialized")]
    ClientNotInitialized,
    
    #[error("Timeout error")]
    Timeout,
    
    #[error("Authentication error: {0}")]
    Auth(String),
    
    #[error("Rate limit exceeded")]
    RateLimit,
    
    #[error("Connection closed")]
    ConnectionClosed,
    
    #[error("Response channel closed")]
    ResponseChannelClosed,
    
    #[error("Internal error: {0}")]
    Internal(String),
}