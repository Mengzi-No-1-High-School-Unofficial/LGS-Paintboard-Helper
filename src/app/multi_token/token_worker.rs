//! Token工作器模块
//!
//! 该模块实现了Token工作器，负责从像素队列获取任务并使用可用的Token
//! 发送绘制请求。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error};

use crate::app::multi_token::pixel_queue::PixelQueue;
use crate::app::multi_token::token_manager::TokenManager;
use rand;
use tokio::sync::mpsc;
use winter_paintboard_sdk::models::PaintOperation;

/// Token 工作器
///
/// 负责从像素队列获取任务并使用可用的Token发送绘制请求
pub struct TokenWorker {
    /// 工作器ID
    worker_id: usize,
    /// Token管理器
    token_manager: Arc<TokenManager>,
    /// 像素队列
    pixel_queue: Arc<PixelQueue>,
    /// 用于将 `PaintOperation` 发送到 `PaintBatcher` 的通道。
    batch_sender: mpsc::UnboundedSender<PaintOperation>,
}

impl TokenWorker {
    /// 创建新的 Worker
    ///
    /// # 参数
    ///
    /// * `worker_id` - 工作器ID
    /// * `token_manager` - Token管理器
    /// * `pixel_queue` - 像素队列
    /// * `batch_sender` - 用于发送 `PaintOperation` 的通道发送端。
    ///
    /// # 返回值
    ///
    /// 返回初始化的TokenWorker实例
    pub fn new(
        worker_id: usize,
        token_manager: Arc<TokenManager>,
        pixel_queue: Arc<PixelQueue>,
        batch_sender: mpsc::UnboundedSender<PaintOperation>,
    ) -> Self {
        Self {
            worker_id,
            token_manager,
            pixel_queue,
            batch_sender,
        }
    }

    /// 启动 Worker 主循环
    ///
    /// 启动工作器主循环，持续从像素队列获取任务并使用可用Token发送绘制请求
    ///
    /// # 参数
    ///
    /// * `stop_signal` - 停止信号
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 正常停止
    /// * `Err` - 运行过程中发生错误
    pub async fn run(
        &self,
        stop_signal: Arc<AtomicBool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("TokenWorker {} 启动", self.worker_id);

        loop {
            if stop_signal.load(Ordering::Acquire) {
                debug!("TokenWorker {} 收到停止信号", self.worker_id);
                break;
            }

            // 1. 尝试获取 Token（非阻塞）
            let mut token_lease = match self.token_manager.clone().try_acquire() {
                Some(lease) => lease,
                None => {
                    // 如果没有可用的 Token，则等待一小段时间或直到下一个 Token 可用
                    if let Some(wait_duration) = self.token_manager.next_available_in() {
                        // 优化：将等待时间从50ms降至5ms，大幅提高token利用率
                        tokio::time::sleep(wait_duration.min(Duration::from_millis(5))).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    continue;
                }
            };

            // 2. 获取像素任务
            let pixel = match self.pixel_queue.try_pop() {
                Some(p) => p,
                None => {
                    // 没有任务，立即释放 Token，避免占用
                    token_lease.mark_failed();

                    // 等待新像素或超时（事件驱动）
                    tokio::select! {
                        _ = self.pixel_queue.wait_for_items() => {
                            // 被唤醒，重新尝试获取像素
                        }
                        _ = tokio::time::sleep(Duration::from_millis(20)) => {
                            // 优化：将超时从100ms降至20ms，减少延迟
                            // 超时保护，防止信号丢失
                        }
                    }
                    continue;
                }
            };

            // 3. 创建 PaintOperation 并发送到 PaintBatcher
            let operation = PaintOperation {
                pos: pixel.pos,
                color: pixel.color,
                token_uid: token_lease.uid(),
                token: token_lease.token().to_string(),
                paint_id: rand::random(), // 在批量模式下，paint_id 主要用于日志和追踪
            };

            if let Err(e) = self.batch_sender.send(operation) {
                error!("Worker {}: 发送操作到 Batcher 失败: {}", self.worker_id, e);
                // 发送失败，意味着 Batcher 已关闭，将 Token 标记为失败以立即释放
                token_lease.mark_failed();
            } else {
                // 发送成功后，立即将 Token 标记为成功使用，以启动其 CD 计时
                token_lease.mark_success();
            }
        }

        debug!("TokenWorker {} 停止", self.worker_id);
        Ok(())
    }
}
