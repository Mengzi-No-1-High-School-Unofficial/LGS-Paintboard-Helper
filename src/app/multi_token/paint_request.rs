use crate::app::multi_token::config::PriorityPixel;
use crate::app::multi_token::token_lease::TokenLease;

/// 绘制请求（携带 Token 租约）
#[derive(Debug)]
pub struct PaintRequest {
    pub pixel: PriorityPixel,
    pub token_lease: TokenLease,
    pub retry_count: u32,
    pub max_retries: u32,
}

impl PaintRequest {
    pub fn new(pixel: PriorityPixel, token_lease: TokenLease) -> Self {
        Self {
            pixel,
            token_lease,
            retry_count: 0,
            max_retries: 3,
        }
    }

    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}