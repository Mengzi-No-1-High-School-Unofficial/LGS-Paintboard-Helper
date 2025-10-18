use std::time::{Duration, Instant};
use std::sync::Arc;
use parking_lot::Mutex;

/// Token 状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenState {
    Available,
    Acquired,
    InCooldown,
}

/// Token 租约（RAII 守卫）
#[derive(Debug)]
pub struct TokenLease {
    index: usize,
    uid: u32,
    token: String,
    manager: Arc<Mutex<Vec<TokenData>>>,
    cd_duration: Duration,
    consumed: bool,  // 防止重复消费
}

impl TokenLease {
    pub fn new(
        index: usize,
        uid: u32,
        token: String,
        manager: Arc<Mutex<Vec<TokenData>>>,
        cd_duration: Duration,
    ) -> Self {
        Self {
            index,
            uid,
            token,
            manager,
            cd_duration,
            consumed: false,
        }
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// 标记绘制成功，启动 CD
    pub fn mark_success(&mut self) {
        self.consumed = true;
        let mut tokens = self.manager.lock();
        if let Some(token) = tokens.get_mut(self.index) {
            token.state = TokenState::InCooldown;
            token.cd_end_time = Some(Instant::now() + self.cd_duration);
        }
    }

    /// 标记绘制失败，释放 Token
    pub fn mark_failed(&mut self) {
        self.consumed = true;
        let mut tokens = self.manager.lock();
        if let Some(token) = tokens.get_mut(self.index) {
            token.state = TokenState::Available;
        }
    }
}

impl Drop for TokenLease {
    fn drop(&mut self) {
        if !self.consumed {
            // 未消费，默认释放
            let mut tokens = self.manager.lock();
            if let Some(token) = tokens.get_mut(self.index) {
                if token.state == TokenState::Acquired {
                    token.state = TokenState::Available;
                }
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct TokenData {
    pub uid: u32,
    pub token: String,
    pub state: TokenState,
    pub cd_end_time: Option<Instant>,
}