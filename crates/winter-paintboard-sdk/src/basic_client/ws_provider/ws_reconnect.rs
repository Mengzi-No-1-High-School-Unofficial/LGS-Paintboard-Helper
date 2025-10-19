use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Reconnect strategy with exponential backoff and a flag to disable reconnection
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
