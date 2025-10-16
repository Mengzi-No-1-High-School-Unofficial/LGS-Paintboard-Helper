use thiserror::Error;

/// Types of protocol violations
#[derive(Error, Debug)]
pub enum ProtocolViolation {
    #[error("Protocol violation: unexpected pong")]
    UnexpectedPong,
    #[error("Protocol violation: unknown packet type")]
    UnknownPacketType,
    #[error("Protocol violation: duplicate ping state")]
    DuplicatePingState,
}

/// WebSocket connection close codes
#[derive(Debug, Clone, Copy)]
pub enum ConnectionCloseCode {
    Normal = 1000,
    GoingAway = 1001, // Ping timeout (文档中称为 1001)
    ProtocolViolation = 1002,
    IPRateLimitExceeded = 1008, // IP connection limit exceeded (文档中称为 1008)
    ServerError = 1011,
    NetworkError = 1006, // 1006 is not actually sent by server but indicates network issues
}

impl std::fmt::Display for ConnectionCloseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", *self as u16)
    }
}

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
    
    #[error("Protocol violation: {0}")]
    ProtocolViolation(#[from] ProtocolViolation),
    
    #[error("Connection closed with code {0}: {1}")]
    ConnectionClosedWithCode(u16, String),
}