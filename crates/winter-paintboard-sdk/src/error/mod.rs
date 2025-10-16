use std::fmt;
use thiserror::Error;

/// 详细的协议违规类型。
/// 用于表示在 WebSocket 通信中发生的各种协议级别错误。
#[derive(Error, Debug, Clone)]
pub enum ProtocolViolation {
    /// 收到意外的 Pong 响应。
    #[error("协议违规: 收到意外的Pong响应")]
    UnexpectedPong,
    
    /// 遇到未知的数据包类型。
    #[error("协议违规: 遇到未知的数据包类型")]
    UnknownPacketType,
    
    /// 检测到重复的 Ping 状态。
    #[error("协议违规: 检测到重复的Ping状态")]
    DuplicatePingState,
}

/// WebSocket 连接关闭码。
/// 定义了 WebSocket 连接因何种原因关闭的标准化代码。
#[derive(Debug, Clone, Copy)]
pub enum ConnectionCloseCode {
    /// 正常关闭，表示连接已成功完成其目的。
    Normal = 1000,
    /// 服务器正在关闭或正在切换到另一个 URL。
    GoingAway = 1001,
    /// 协议错误，表示端点收到无法解释的数据。
    ProtocolViolation = 1002,
    /// IP 速率限制，表示请求过于频繁。
    IPRateLimitExceeded = 1008,
    /// 服务器内部错误。
    ServerError = 1011,
    /// 网络错误，通常表示底层连接中断。
    NetworkError = 1006,
}

impl fmt::Display for ConnectionCloseCode {
    /// 将 `ConnectionCloseCode` 格式化为字符串。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", *self as u16)
    }
}

/// Winter Paintboard SDK 的主要错误类型。
/// 封装了所有可能发生的错误，并提供了详细的错误信息和上下文。
#[derive(Error, Debug)]
pub enum PaintboardError {
    /// 网络错误，包含详细描述。
    #[error("网络错误: {0}")]
    Network(String),
    
    /// WebSocket 连接错误。
    #[error("WebSocket错误: {0}")]
    WebSocket(String),
    
    /// JSON 解析错误。
    #[error("JSON解析错误: {0}")]
    JsonParse(String),
    
    /// 提供的坐标无效。
    #[error("无效的坐标: x={x}, y={y}")]
    InvalidCoordinate { x: i32, y: i32 },
    
    /// 访问的索引超出了有效范围。
    #[error("索引越界: 当前={current}, 最大={max}")]
    IndexOutOfRange { current: usize, max: usize },
    
    /// 数据格式不正确。
    #[error("数据格式错误: {0}")]
    InvalidData(String),
    
    /// HTTP 请求返回了错误状态码。
    #[error("HTTP错误: 状态码 {0}")]
    Http(u16),
    
    /// 提供的 URL 无效。
    #[error("无效的URL: {0}")]
    InvalidUrl(String),
    
    /// 客户端在使用前未进行初始化。
    #[error("客户端未正确初始化")]
    ClientNotInitialized,
    
    /// 操作超时。
    #[error("操作超时")]
    Timeout,
    
    /// 认证失败。
    #[error("认证失败: {0}")]
    Auth(String),
    
    /// 请求频率超过了速率限制。
    #[error("超过速率限制: 请求过于频繁")]
    RateLimit,
    
    /// 连接已关闭，无法执行操作。
    #[error("连接已关闭")]
    ConnectionClosed,
    
    /// 响应通道已关闭，无法接收响应。
    #[error("响应通道已关闭")]
    ResponseChannelClosed,
    
    /// SDK 内部发生的错误。
    #[error("内部错误: {0}")]
    Internal(String),
    
    /// 协议违规错误，由 `ProtocolViolation` 转换而来。
    #[error("协议违规: {0}")]
    ProtocolViolation(#[from] ProtocolViolation),
    
    /// WebSocket 连接因特定关闭码而关闭。
    #[error("连接因 {0} 关闭: {1}")]
    ConnectionClosedWithCode(u16, String),

    /// 封装了另一个错误并添加了上下文信息。
    #[error("上下文错误: {0}, 源错误: {1}")]
    ContextualError(String, Box<PaintboardError>),
}

impl PaintboardError {
    /// 创建带有上下文信息的错误。
    ///
    /// # 参数
    /// - `context`: 错误的上下文描述。
    /// - `source`: 原始的 `PaintboardError`。
    pub fn contextual<C>(context: C, source: PaintboardError) -> Self
    where
        C: Into<String>,
    {
        PaintboardError::ContextualError(context.into(), Box::new(source))
    }

    /// 创建网络错误的便捷方法。
    ///
    /// # 参数
    /// - `message`: 错误描述。
    pub fn network(message: impl Into<String>) -> Self {
        PaintboardError::Network(message.into())
    }

    /// 创建 WebSocket 错误的便捷方法。
    ///
    /// # 参数
    /// - `message`: 错误描述。
    pub fn websocket(message: impl Into<String>) -> Self {
        PaintboardError::WebSocket(message.into())
    }

    /// 创建认证错误的便捷方法。
    ///
    /// # 参数
    /// - `message`: 错误描述。
    pub fn auth(message: impl Into<String>) -> Self {
        PaintboardError::Auth(message.into())
    }

    /// 创建超时错误的便捷方法。
    pub fn timeout() -> Self {
        PaintboardError::Timeout
    }

    /// 创建无效数据错误的便捷方法。
    ///
    /// # 参数
    /// - `message`: 错误描述。
    pub fn invalid_data(message: impl Into<String>) -> Self {
        PaintboardError::InvalidData(message.into())
    }

    /// 创建无效坐标错误的便捷方法。
    ///
    /// # 参数
    /// - `x`: 无效坐标的 x 值。
    /// - `y`: 无效坐标的 y 值。
    pub fn invalid_coordinate(x: i32, y: i32) -> Self {
        PaintboardError::InvalidCoordinate { x, y }
    }

    /// 创建索引越界错误的便捷方法。
    ///
    /// # 参数
    /// - `current`: 当前尝试访问的索引。
    /// - `max`: 最大允许的索引（通常是集合的大小）。
    pub fn index_out_of_range(current: usize, max: usize) -> Self {
        PaintboardError::IndexOutOfRange { current, max }
    }

    /// 创建速率限制错误的便捷方法。
    pub fn rate_limit() -> Self {
        PaintboardError::RateLimit
    }
}

/// 实现从标准 IO 错误到 `PaintboardError` 的转换。
impl From<std::io::Error> for PaintboardError {
    /// 将 `std::io::Error` 转换为 `PaintboardError::Network`。
    fn from(err: std::io::Error) -> Self {
        PaintboardError::Network(err.to_string())
    }
}

/// 实现从 `reqwest` 错误到 `PaintboardError` 的转换。
#[cfg(feature = "reqwest")]
impl From<reqwest::Error> for PaintboardError {
    /// 将 `reqwest::Error` 转换为 `PaintboardError::Network`。
    fn from(err: reqwest::Error) -> Self {
        PaintboardError::Network(err.to_string())
    }
}

/// 辅助方法，用于直接创建特定类型的 `PaintboardError`，通常用于内部或测试场景。
impl PaintboardError {
    /// 直接创建 `InvalidData` 错误，带有默认消息。
    pub fn invalid_data_direct() -> Self {
        PaintboardError::InvalidData("Invalid data".to_string())
    }

    /// 直接创建 `InvalidCoordinate` 错误，带有默认无效坐标。
    pub fn invalid_coordinate_direct() -> Self {
        PaintboardError::InvalidCoordinate { x: -1, y: -1 }
    }

    /// 直接创建 `IndexOutOfRange` 错误，带有默认越界信息。
    pub fn index_out_of_range_direct() -> Self {
        PaintboardError::IndexOutOfRange { current: 0, max: 0 }
    }
}