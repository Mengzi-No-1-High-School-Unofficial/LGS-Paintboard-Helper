use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use color_eyre::eyre::Result;
use tracing::{debug, info};

use crate::basic_client::ws_provider::ws_connection::WsConnection;

/// Reconnect strategy with exponential backoff and a flag to disable reconnection
#[derive(Debug)]
pub struct WsReconnectStrategy {
    should_reconnect: Arc<AtomicBool>,
    current_delay: Duration,
    max_delay: Duration,
    initial_delay: Duration,
}

impl WsReconnectStrategy {
    /// Create a new strategy with initial and max delays
    pub fn new(initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            should_reconnect: Arc::new(AtomicBool::new(true)),
            current_delay: initial_delay,
            max_delay,
            initial_delay,
        }
    }

    /// Create with defaults: initial 1s, max 60s
    pub fn default() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(60))
    }

    /// Check whether reconnection attempts are allowed
    pub fn should_reconnect(&self) -> bool {
        self.should_reconnect.load(Ordering::Relaxed)
    }

    /// Disable future reconnection attempts
    pub fn disable_reconnect(&self) {
        self.should_reconnect.store(false, Ordering::Relaxed);
    }

    /// Get next delay and advance internal backoff (cap at max_delay)
    pub fn next_delay(&mut self) -> Duration {
        let next = self.current_delay;
        // exponential backoff: double, but cap
        self.current_delay = std::cmp::min(self.current_delay * 2, self.max_delay);
        next
    }

    /// Reset the backoff to initial delay
    pub fn reset(&mut self) {
        self.current_delay = self.initial_delay;
    }

    /// Get current delay without advancing
    pub fn current_delay(&self) -> Duration {
        self.current_delay
    }
}

/// Reconnect manager that wraps the strategy and provides async methods
pub struct WsReconnectManager {
    strategy: WsReconnectStrategy,
}

impl WsReconnectManager {
    pub fn new(initial_delay: Duration, max_delay: Duration) -> Self {
        Self {
            strategy: WsReconnectStrategy::new(initial_delay, max_delay),
        }
    }

    pub fn default() -> Self {
        Self {
            strategy: WsReconnectStrategy::default(),
        }
    }

    pub fn should_reconnect(&self) -> bool {
        self.strategy.should_reconnect()
    }

    pub fn disable_reconnect(&self) {
        self.strategy.disable_reconnect();
    }

    pub async fn next_delay(&mut self) -> Duration {
        self.strategy.next_delay()
    }

    pub async fn reset(&mut self) {
        self.strategy.reset();
    }

    pub fn current_delay(&self) -> Duration {
        self.strategy.current_delay()
    }

    pub async fn reconnect(&mut self, connection: Arc<WsConnection>) -> Result<()> {
        if self.should_reconnect() {
            let delay = self.next_delay().await;
            info!("正在重连，请稍候...");
            tokio::time::sleep(delay).await;
            connection.connect().await?;
        } else {
            debug!("重连已禁用");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_and_backoff() {
        let mut s = WsReconnectStrategy::default();
        assert!(s.should_reconnect());
        let d1 = s.next_delay();
        assert_eq!(d1, Duration::from_secs(1));
        let d2 = s.next_delay();
        assert_eq!(d2, Duration::from_secs(2));
        // reset
        s.reset();
        assert_eq!(s.current_delay(), Duration::from_secs(1));
        s.disable_reconnect();
        assert!(!s.should_reconnect());
    }
}
