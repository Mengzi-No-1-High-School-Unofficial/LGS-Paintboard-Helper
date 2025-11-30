//! 绘制请求模块
//!
//! 该模块定义了绘制请求结构，包含待绘制的像素、Token租约和重试机制。

use crate::app::multi_token::config::PriorityPixel;
use crate::app::multi_token::token_lease::TokenLease;

/// 绘制请求（携带 Token 租约）
///
/// 包含待绘制的像素信息、Token租约和重试计数
#[derive(Debug)]
pub struct PaintRequest {
    /// 要绘制的优先级像素
    pub pixel: PriorityPixel,
    /// Token租约
    pub token_lease: TokenLease,
    /// 重试次数
    pub retry_count: u32,
    /// 最大重试次数
    pub max_retries: u32,
}

impl PaintRequest {
    /// 创建新的绘制请求
    ///
    /// # 参数
    ///
    /// * `pixel` - 优先级像素
    /// * `token_lease` - Token租约
    ///
    /// # 返回值
    ///
    /// 返回初始化的绘制请求实例
    pub fn new(pixel: PriorityPixel, token_lease: TokenLease) -> Self {
        Self {
            pixel,
            token_lease,
            retry_count: 0,
            max_retries: 3,
        }
    }

    /// 检查是否可以重试
    ///
    /// # 返回值
    ///
    /// 如果重试次数未达到最大值返回true，否则返回false
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.max_retries
    }

    /// 增加重试次数
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }
}
