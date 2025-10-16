

//! `config` 模块定义了 Winter Paintboard SDK 的各种配置选项。

/// WebSocket 连接模式。
/// 决定了客户端是只读、只写还是读写模式。
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionMode {
    /// 允许读写操作。
    ReadWrite,
    /// 只允许读取操作。
    ReadOnly,
    /// 只允许写入操作。
    WriteOnly,
}

/// Paintboard 客户端的配置结构。
/// 包含了连接、重试、批量操作等各种参数。
#[derive(Debug, Clone)]
pub struct Config {
    /// Paintboard API 的基础 URL，用于 HTTP 请求。
    pub api_base_url: String,
    /// Paintboard WebSocket API 的 URL。
    pub ws_url: String,
    /// WebSocket 心跳间隔时间。
    pub heartbeat_interval: std::time::Duration,
    /// 最大重试次数。
    pub max_retries: u32,
    /// 每次重试之间的延迟。
    pub retry_delay: std::time::Duration,
    /// 批量操作的超时时间。
    pub batch_timeout: std::time::Duration,
    /// 批量操作的最大尺寸（像素数量）。
    pub max_batch_size: usize,
    /// WebSocket 连接模式。
    pub connection_mode: ConnectionMode,
}

impl Config {
    /// 创建一个新的 `Config` 实例。
    ///
    /// # 参数
    /// - `api_base_url`: API 的基础 URL。
    /// - `ws_url`: WebSocket 的 URL。
    /// - `heartbeat_interval`: 心跳间隔。
    /// - `max_retries`: 最大重试次数。
    /// - `retry_delay`: 重试延迟。
    /// - `batch_timeout`: 批量操作超时。
    /// - `max_batch_size`: 最大批量大小。
    /// - `connection_mode`: 连接模式。
    pub fn new(
        api_base_url: String,
        ws_url: String,
        heartbeat_interval: std::time::Duration,
        max_retries: u32,
        retry_delay: std::time::Duration,
        batch_timeout: std::time::Duration,
        max_batch_size: usize,
        connection_mode: ConnectionMode,
    ) -> Self {
        Self {
            api_base_url,
            ws_url,
            heartbeat_interval,
            max_retries,
            retry_delay,
            batch_timeout,
            max_batch_size,
            connection_mode,
        }
    }
}

impl Default for Config {
    /// 返回一个默认的 `Config` 实例。
    ///
    /// 默认值包括：
    /// - `api_base_url`: "https://paintboard.luogu.me"
    /// - `ws_url`: "wss://paintboard.luogu.me/api/paintboard/ws"
    /// - `heartbeat_interval`: 30 秒
    /// - `max_retries`: 3 次
    /// - `retry_delay`: 1 秒
    /// - `batch_timeout`: 20 毫秒
    /// - `max_batch_size`: 100 像素
    /// - `connection_mode`: `ConnectionMode::ReadWrite`
    fn default() -> Self {
        Self {
            api_base_url: "https://paintboard.luogu.me".to_string(),
            ws_url: "wss://paintboard.luogu.me/api/paintboard/ws".to_string(),
            heartbeat_interval: std::time::Duration::from_secs(30),
            max_retries: 3,
            retry_delay: std::time::Duration::from_secs(1),
            batch_timeout: std::time::Duration::from_millis(20),
            max_batch_size: 100,
            connection_mode: ConnectionMode::ReadWrite,
        }
    }
}