use color_eyre::Report;
use log::{debug, error, info, warn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock};

use super::paint_request::PaintRequest;
use crate::app::board_sync::local_board::{LocalBoard, PixelSource};
use winter_paintboard_sdk::PoolClient;
use winter_paintboard_sdk::{models::PaintStatus, PaintboardClientTrait};

/// 绘制请求队列
pub struct PaintRequestQueue {
    sender: mpsc::UnboundedSender<PaintRequest>,
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<PaintRequest>>>,
}

impl PaintRequestQueue {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    pub fn send(&self, request: PaintRequest) -> Result<(), String> {
        self.sender
            .send(request)
            .map_err(|e| format!("发送绘制请求失败: {}", e))
    }

    pub async fn recv(&self) -> Option<PaintRequest> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }
}

/// 单线程绘制执行器
#[derive(Clone)]
pub struct PaintExecutor {
    client: Arc<PoolClient>,
    request_queue: Arc<PaintRequestQueue>,
    local_board: Arc<RwLock<LocalBoard>>,
    pixel_queue: Arc<crate::app::multi_token::pixel_queue::PixelQueue>,
}

impl PaintExecutor {
    pub fn new(
        client: Arc<PoolClient>,
        request_queue: Arc<PaintRequestQueue>,
        local_board: Arc<RwLock<LocalBoard>>,
        pixel_queue: Arc<crate::app::multi_token::pixel_queue::PixelQueue>,
    ) -> Self {
        Self {
            client,
            request_queue,
            local_board,
            pixel_queue,
        }
    }

    /// 启动执行循环
    pub async fn run(&self, stop_signal: Arc<AtomicBool>) {
        info!("PaintExecutor 启动");

        while !stop_signal.load(Ordering::Acquire) {
            let request = match self.request_queue.recv().await {
                Some(req) => req,
                None => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            // self.process_request(request).await;
            let self_clone = self.clone();
            tokio::spawn(async move { self_clone.process_request(request).await });
        }

        info!("PaintExecutor 停止");
    }

    /// 处理单个绘制请求
    async fn process_request(&self, mut request: PaintRequest) {
        debug!(
            "处理绘制请求: ({}, {}), token_uid: {}",
            request.pixel.pos.x,
            request.pixel.pos.y,
            request.token_lease.uid()
        );

        let result = {
            let client = &self.client;
            let conn = client.get_write_conn().await;

            if let Err(e) = conn {
                error!("无法获取连接，绘制请求取消并等待重试: {:?}", e);
                return;
            }

            let mut conn = conn.unwrap();

            // 请求发送后就可以视为 Token 已被使用（不管服务端是否接受其，我们都将其视作进入一次 CD）
            request.token_lease.mark_success();

            debug!("获取客户端成功，开始绘制");

            // 添加超时机制，确保即使WsProvider操作挂起，连接也能被释放
            let paint_result = tokio::time::timeout(
                Duration::from_secs(60),
                conn.paint_with_token(
                    request.pixel.pos,
                    request.pixel.color,
                    request.token_lease.uid(),
                    request.token_lease.token().to_string(),
                ),
            )
            .await;

            let paint_result = match paint_result {
                Ok(result) => {
                    debug!(
                        "绘制操作完成: ({}, {}), 结果: {:?}",
                        request.pixel.pos.x, request.pixel.pos.y, result
                    );

                    Ok(result.unwrap())
                }
                Err(_) => Err(Report::msg("绘制操作超时（RX 长期未被 WsProvider 释放）")),
            };

            paint_result
        };

        match result {
            Ok(paint_result) => {
                match paint_result.status {
                    PaintStatus::Success => {
                        // 更新本地绘版
                        {
                            let mut board = self.local_board.write().await;
                            board.update_pixel(
                                request.pixel.pos.x,
                                request.pixel.pos.y,
                                request.pixel.color,
                                PixelSource::Own,
                            );

                            info!(
                                "成功在 ({}, {}) 使用 Token {} 绘制像素",
                                request.pixel.pos.x,
                                request.pixel.pos.y,
                                request.token_lease.uid()
                            );
                        }

                        // 标记成功，启动 CD
                        request.token_lease.mark_success();

                        debug!(
                            "绘制成功: ({}, {})",
                            request.pixel.pos.x, request.pixel.pos.y
                        );
                    }
                    PaintStatus::Cooldown => {
                        // 不必处理重试，循环比对会忽略
                        info!("Token {} 仍在 CD 中", request.token_lease.uid());
                    }
                    _ => {
                        warn!("绘制失败: {:?}", paint_result.status);
                    }
                }
            }
            Err(e) => {
                error!("绘制错误: {:?}", e);
            }
        }
    }
}
