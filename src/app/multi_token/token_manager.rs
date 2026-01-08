//! Token管理器模块
//!
//! 该模块实现了Token的管理和分配功能，包括Token状态跟踪、
//! 冷却时间管理和并发安全的Token分配。

use crate::app::multi_token::token_lease::{TokenData, TokenLease, TokenState};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Token 信息（保持兼容性）
///
/// 包含Token的基本信息和状态
#[derive(Debug, Clone)]
pub struct TokenInfo {
    /// 用户ID
    pub uid: u32,
    /// Token字符串
    pub token: String,
    /// 最后绘制时间
    #[allow(dead_code)]
    pub last_paint_time: Option<Instant>,
    /// 是否可用
    #[allow(dead_code)]
    pub is_available: bool,
}

impl TokenInfo {
    /// 创建新的 TokenInfo
    ///
    /// # 参数
    ///
    /// * `uid` - 用户ID
    /// * `token` - Token字符串
    ///
    /// # 返回值
    ///
    /// 返回初始化的TokenInfo实例
    #[allow(dead_code)]
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
///
/// 管理多个Token的状态，包括可用性、冷却时间和分配
#[derive(Debug, Clone)]
pub struct TokenManager {
    /// Token数据列表
    tokens: Arc<Mutex<Vec<TokenData>>>,
    /// 可用Token索引队列
    available_indices: Arc<Mutex<VecDeque<usize>>>,
    /// 冷却时间持续时间
    cd_duration: Duration,
}

impl TokenManager {
    /// 创建新的 TokenManager
    ///
    /// # 参数
    ///
    /// * `tokens` - Token信息列表
    /// * `cd_time_ms` - 冷却时间（毫秒）
    ///
    /// # 返回值
    ///
    /// 返回初始化的TokenManager实例
    #[allow(dead_code)]
    pub fn new(tokens: Vec<TokenInfo>, cd_time_ms: u64) -> Self {
        let token_count = tokens.len();
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
            available_indices: Arc::new(Mutex::new((0..token_count).collect())),
            cd_duration: Duration::from_millis(cd_time_ms),
        }
    }

    /// 非阻塞获取 Token
    ///
    /// 尝试获取一个可用的Token，如果存在可用Token则返回TokenLease，
    /// 否则返回None
    ///
    /// # 返回值
    ///
    /// 返回可用Token的租约（如果存在）
    pub fn try_acquire(self: &Arc<Self>) -> Option<TokenLease> {
        let mut tokens = self.tokens.lock();
        let mut available_indices = self.available_indices.lock();
        let now = Instant::now();

        // 1. 更新所有Token的CD状态，并将完成CD的Token索引加入队列
        for (index, token) in tokens.iter_mut().enumerate() {
            if token.state == TokenState::InCooldown {
                if let Some(end_time) = token.cd_end_time {
                    if now >= end_time {
                        token.state = TokenState::Available;
                        token.cd_end_time = None;
                        if !available_indices.contains(&index) {
                            available_indices.push_back(index);
                        }
                    }
                }
            }
        }

        // 2. 尝试从队列获取可用Token
        if let Some(index) = available_indices.pop_front() {
            if let Some(token) = tokens.get_mut(index) {
                if token.state == TokenState::Available {
                    token.state = TokenState::Acquired;
                    return Some(TokenLease::new(
                        index,
                        token.uid,
                        token.token.clone(),
                        self.clone(),
                    ));
                }
            }
        }

        None
    }

    /// 获取下一个 Token 可用的最短等待时间
    ///
    /// # 返回值
    ///
    /// 返回下一个可用Token的最短等待时间（如果存在）
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

    /// 释放一个Token（由TokenLease调用）
    pub(super) fn release(&self, index: usize, success: bool) {
        let mut tokens = self.tokens.lock();
        if let Some(token) = tokens.get_mut(index) {
            if success {
                token.state = TokenState::InCooldown;
                token.cd_end_time = Some(Instant::now() + self.cd_duration);
            } else {
                token.state = TokenState::Available;
                token.cd_end_time = None;
                // 失败或未使用的Lease，立即将Token索引放回可用队列
                let mut available_indices = self.available_indices.lock();
                if !available_indices.contains(&index) {
                    available_indices.push_back(index);
                }
            }
        }
    }

    /// 获取 Token 数量
    ///
    /// # 返回值
    ///
    /// 返回管理器中的Token总数
    pub fn len(&self) -> usize {
        self.tokens.lock().len()
    }

    /// 获取 CD 时间
    ///
    /// # 返回值
    ///
    /// 返回Token的冷却时间
    #[allow(dead_code)]
    pub fn cd_duration(&self) -> Duration {
        self.cd_duration
    }

    /// 获取下一个 Token 可用的时间（兼容旧接口）
    ///
    /// # 返回值
    ///
    /// 返回下一个可用Token的等待时间（如果存在）
    #[allow(dead_code)]
    pub fn next_available_time(&self) -> Option<Duration> {
        self.next_available_in()
    }

    /// 获取 Token 信息的引用（兼容旧接口）
    ///
    /// # 参数
    ///
    /// * `index` - Token索引
    ///
    /// # 返回值
    ///
    /// 返回指定索引的Token信息引用（如果存在）
    #[allow(dead_code)]
    pub fn get_token(&self, _index: usize) -> Option<&TokenInfo> {
        // 这里为了兼容性，返回一个临时的 TokenInfo
        // 实际使用中应该直接使用新接口
        None
    }

    /// 获取所有 Token 信息（兼容旧接口）
    ///
    /// # 返回值
    ///
    /// 返回所有Token信息的列表
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
