use governor::{clock::DefaultClock, state::{InMemoryState, NotKeyed}, Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::Arc;

/// 简单封装 governor 速率限制器
/// 提供每秒请求速率的检查方法
pub struct WsRateLimiter {
    limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
}

impl WsRateLimiter {
    /// 创建速率限制器，rps 为每秒允许的请求数（必须 >=1）
    pub fn new(rps: u32) -> Self {
        let rps = if rps == 0 { 1 } else { rps };
        let quota = Quota::per_second(NonZeroU32::new(rps).unwrap());
        let limiter = RateLimiter::direct(quota);
        Self {
            limiter: Arc::new(limiter),
        }
    }

    /// 检查是否允许发送请求。返回 true 表示允许，false 表示被限流
    pub fn check(&self) -> bool {
        self.limiter.check().is_ok()
    }
}