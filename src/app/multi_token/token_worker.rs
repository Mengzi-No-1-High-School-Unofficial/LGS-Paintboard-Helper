use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use log::{debug, error, warn};

use winter_paintboard_sdk::{PaintboardClientTrait, Pos, Rgb, models::{PaintResult, PaintStatus}};
use crate::app::board_sync::local_board::{LocalBoard, PixelSource};
use crate::app::multi_token::config::PriorityPixel;
use crate::app::multi_token::pixel_queue::PixelQueue;
use crate::app::multi_token::token_manager::TokenManager;

/// Token 工作器
pub struct TokenWorker {
    worker_id: usize,
    token_manager: Arc<TokenManager>,
    pixel_queue: Arc<PixelQueue>,
    request_queue: Arc<crate::app::multi_token::paint_executor::PaintRequestQueue>,
}

impl TokenWorker {
    /// 创建新的 Worker
    pub fn new(
        worker_id: usize,
        token_manager: Arc<TokenManager>,
        pixel_queue: Arc<PixelQueue>,
        request_queue: Arc<crate::app::multi_token::paint_executor::PaintRequestQueue>,
    ) -> Self {
        Self {
            worker_id,
            token_manager,
            pixel_queue,
            request_queue,
        }
    }

    /// 启动 Worker 主循环
    pub async fn run(&self, stop_signal: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
        debug!("TokenWorker {} 启动", self.worker_id);

        loop {
            if stop_signal.load(Ordering::Acquire) {
                debug!("TokenWorker {} 收到停止信号", self.worker_id);
                break;
            }

            // 1. 尝试获取 Token（非阻塞）
            let mut token_lease = match self.token_manager.try_acquire() {
                Some(lease) => lease,
                None => {
                    // 等待最短 CD
                    if let Some(wait) = self.token_manager.next_available_in() {
                        let wait = wait.min(Duration::from_millis(100));
                        tokio::time::sleep(wait).await;
                    } else {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    continue;
                }
            };

            // 2. 获取像素任务
            let pixel = match self.pixel_queue.try_pop().await {
                Some(p) => p,
                None => {
                    // 没有任务，释放 Token
                    token_lease.mark_failed();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };

            // 3. 组装并发送绘制请求
            let request = crate::app::multi_token::paint_request::PaintRequest::new(pixel, token_lease);
            if let Err(e) = self.request_queue.send(request) {
                error!("Worker {}: 发送请求失败: {}", self.worker_id, e);
            }
        }

        debug!("TokenWorker {} 停止", self.worker_id);
        Ok(())
    }
}