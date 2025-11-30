//! Token租约模块
//!
//! 该模块实现了Token的RAII租约管理，确保Token的正确分配和释放，
//! 包括状态跟踪和冷却时间管理。

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Token 状态枚举
///
/// 表示Token的当前状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenState {
    /// 可用状态
    Available,
    /// 已获取状态
    Acquired,
    /// 冷却中状态
    InCooldown,
}

/// Token 租约（RAII 守卫）
///
/// 管理Token的生命周期，确保在使用后正确释放或设置冷却时间
#[derive(Debug)]
pub struct TokenLease {
    /// Token在管理器中的索引
    index: usize,
    /// 用户ID
    uid: u32,
    /// Token字符串
    token: String,
    /// Token管理器引用
    manager: Arc<Mutex<Vec<TokenData>>>,
    /// 冷却时间持续时间
    cd_duration: Duration,
    /// 是否已消费（防止重复消费）
    consumed: bool,
}

impl TokenLease {
    /// 创建新的Token租约
    ///
    /// # 参数
    ///
    /// * `index` - Token在管理器中的索引
    /// * `uid` - 用户ID
    /// * `token` - Token字符串
    /// * `manager` - Token管理器引用
    /// * `cd_duration` - 冷却时间持续时间
    ///
    /// # 返回值
    ///
    /// 返回初始化的TokenLease实例
    pub fn new(
        index: usize,
        uid: u32,
        token: String,
        manager: Arc<Mutex<Vec<TokenData>>>,
        cd_duration: Duration,
    ) -> Self {
        // 网络延迟问题
        // let cd_duration = Duration::from_millis(
        //     (cd_duration.as_millis() + 300).try_into().unwrap_or(
        //         cd_duration
        //             .as_millis()
        //             .try_into()
        //             .expect("这个错误理论上不可能发生"),
        //     ),
        // );

        Self {
            index,
            uid,
            token,
            manager,
            cd_duration,
            consumed: false,
        }
    }

    /// 获取用户ID
    ///
    /// # 返回值
    ///
    /// 返回Token关联的用户ID
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// 获取Token字符串
    ///
    /// # 返回值
    ///
    /// 返回Token字符串的引用
    pub fn token(&self) -> &str {
        &self.token
    }

    /// 标记绘制成功，启动冷却时间
    ///
    /// 将Token状态设置为冷却中，并记录冷却结束时间
    pub fn mark_success(&mut self) {
        self.consumed = true;
        let mut tokens = self.manager.lock();
        if let Some(token) = tokens.get_mut(self.index) {
            token.state = TokenState::InCooldown;
            token.cd_end_time = Some(Instant::now() + self.cd_duration);
        }
    }

    /// 标记绘制失败，释放 Token
    ///
    /// 将Token状态设置为可用，使其可以被重新获取
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
/// Token数据结构
///
/// 存储Token的详细信息和状态
pub(crate) struct TokenData {
    /// 用户ID
    pub uid: u32,
    /// Token字符串
    pub token: String,
    /// Token状态
    pub state: TokenState,
    /// 冷却结束时间（如果在冷却中）
    pub cd_end_time: Option<Instant>,
}
