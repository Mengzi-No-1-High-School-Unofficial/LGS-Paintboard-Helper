

/// Configuration for the paintboard client
#[derive(Debug, Clone)]
pub struct Config {
    pub api_base_url: String,
    pub ws_url: String,
    pub heartbeat_interval: std::time::Duration,
    pub max_retries: u32,
    pub retry_delay: std::time::Duration,
    pub batch_timeout: std::time::Duration,
    pub max_batch_size: usize,
}

impl Config {
    pub fn new(
        api_base_url: String,
        ws_url: String,
        heartbeat_interval: std::time::Duration,
        max_retries: u32,
        retry_delay: std::time::Duration,
        batch_timeout: std::time::Duration,
        max_batch_size: usize,
    ) -> Self {
        Self {
            api_base_url,
            ws_url,
            heartbeat_interval,
            max_retries,
            retry_delay,
            batch_timeout,
            max_batch_size,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_base_url: "https://paintboard.luogu.me".to_string(),
            ws_url: "wss://paintboard.luogu.me/api/paintboard/ws".to_string(),
            heartbeat_interval: std::time::Duration::from_secs(30),
            max_retries: 3,
            retry_delay: std::time::Duration::from_secs(1),
            batch_timeout: std::time::Duration::from_millis(20),
            max_batch_size: 100,
        }
    }
}