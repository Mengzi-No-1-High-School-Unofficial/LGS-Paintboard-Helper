use crate::app::multi_token::token_lease::{TokenData, TokenLease, TokenState};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Token 信息（保持兼容性）
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub uid: u32,
    pub token: String,
    pub last_paint_time: Option<Instant>,
    pub is_available: bool,
}

impl TokenInfo {
    /// 创建新的 TokenInfo
    pub fn new(uid: u32, token: String) -> Self {
        Self {
            uid,
            token,
            last_paint_time: None,
            is_available: true,
        }
    }
}

/// 非阻塞 Token 管理器
#[derive(Clone)]
pub struct TokenManager {
    tokens: Arc<Mutex<Vec<TokenData>>>,
    cd_duration: Duration,
}

impl TokenManager {
    /// 创建新的 TokenManager
    pub fn new(tokens: Vec<TokenInfo>, cd_time_ms: u64) -> Self {
        let token_data = tokens
            .into_iter()
            .map(|t| TokenData {
                uid: t.uid,
                token: t.token,
                state: TokenState::Available,
                cd_end_time: None,
            })
            .collect();

        Self {
            tokens: Arc::new(Mutex::new(token_data)),
            cd_duration: Duration::from_millis(cd_time_ms),
        }
    }

    /// 非阻塞获取 Token
    pub fn try_acquire(&self) -> Option<TokenLease> {
        let mut tokens = self.tokens.lock();
        let now = Instant::now();

        // 1. 更新 CD 状态
        for token in tokens.iter_mut() {
            if token.state == TokenState::InCooldown {
                if let Some(end_time) = token.cd_end_time {
                    if now >= end_time {
                        token.state = TokenState::Available;
                        token.cd_end_time = None;
                    }
                }
            }
        }

        // 2. 查找可用 Token
        for (index, token) in tokens.iter_mut().enumerate() {
            if token.state == TokenState::Available {
                token.state = TokenState::Acquired;
                return Some(TokenLease::new(
                    index,
                    token.uid,
                    token.token.clone(),
                    Arc::clone(&self.tokens),
                    self.cd_duration,
                ));
            }
        }

        None
    }

    /// 获取下一个 Token 可用的最短等待时间
    pub fn next_available_in(&self) -> Option<Duration> {
        let tokens = self.tokens.lock();
        let now = Instant::now();

        tokens
            .iter()
            .filter_map(|token| match token.state {
                TokenState::Available => Some(Duration::ZERO),
                TokenState::InCooldown => token
                    .cd_end_time
                    .and_then(|end| end.checked_duration_since(now)),
                TokenState::Acquired => None,
            })
            .min()
    }

    /// 获取 Token 数量
    pub fn len(&self) -> usize {
        self.tokens.lock().len()
    }

    /// 获取 CD 时间
    pub fn cd_duration(&self) -> Duration {
        self.cd_duration
    }

    /// 获取下一个 Token 可用的时间（兼容旧接口）
    pub fn next_available_time(&self) -> Option<Duration> {
        self.next_available_in()
    }

    /// 获取 Token 信息的引用（兼容旧接口）
    pub fn get_token(&self, index: usize) -> Option<&TokenInfo> {
        // 这里为了兼容性，返回一个临时的 TokenInfo
        // 实际使用中应该直接使用新接口
        None
    }

    /// 获取所有 Token 信息（兼容旧接口）
    pub fn get_all_tokens(&self) -> Vec<TokenInfo> {
        let tokens = self.tokens.lock();
        tokens
            .iter()
            .map(|t| TokenInfo {
                uid: t.uid,
                token: t.token.clone(),
                last_paint_time: None,
                is_available: t.state == TokenState::Available,
            })
            .collect()
    }
}
