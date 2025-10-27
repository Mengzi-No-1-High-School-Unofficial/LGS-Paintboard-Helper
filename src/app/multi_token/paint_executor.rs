use clap::error;
use color_eyre::Report;
use log::{debug, error, info, warn};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use winter_paintboard_sdk::Pos;

use super::paint_request::PaintRequest;
use crate::app::board_sync::local_board::{LocalBoard, PixelSource};
use crate::app::metrics::Metrics;
use crate::app::multi_token::multi_token_service::MultiTokenService;
use winter_paintboard_sdk::{
    basic_client::AsyncClient, models::PaintStatus, PaintboardClientTrait,
};

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
    client: Arc<AsyncClient>,
    request_queue: Arc<PaintRequestQueue>,
    local_board: Arc<RwLock<LocalBoard>>,
    local_paint_history: Arc<RwLock<FxHashMap<Pos, Vec<Instant>>>>,
    local_paint_total: Arc<AtomicU64>,
}

impl PaintExecutor {
    pub fn new(
        client: Arc<AsyncClient>,
        request_queue: Arc<PaintRequestQueue>,
        local_board: Arc<RwLock<LocalBoard>>,
        local_paint_history: Arc<RwLock<FxHashMap<Pos, Vec<Instant>>>>,
        local_paint_total: Arc<AtomicU64>,
    ) -> Self {
        Self {
            client,
            request_queue,
            local_board,
            local_paint_history,
            local_paint_total,
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

        // 请求发送后就可以视为 Token 已被使用（不管服务端是否接受其，我们都将其视作进入一次 CD）
        request.token_lease.mark_success();

        debug!("获取客户端成功，开始绘制");

        // 添加超时机制，确保即使WsProvider操作挂起，连接也能被释放
        let paint_result = tokio::time::timeout(
            Duration::from_secs(60),
            self.client.paint_with_token(
                request.pixel.pos,
                request.pixel.color,
                request.token_lease.uid(),
                request.token_lease.token().to_string(),
            ),
        )
        .await;

        let result = match paint_result {
            Ok(result) => {
                debug!(
                    "绘制操作完成: ({}, {}), 结果: {:?}",
                    request.pixel.pos.x, request.pixel.pos.y, result
                );

                result
            }
            Err(_) => Err(winter_paintboard_sdk::error::PaintboardError::Internal(
                "绘制操作超时（RX 长期未被 WsProvider 释放）".to_string(),
            )),
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
                                "成功在 ({}, {}) 使用 Token {} 绘制像素（优先级 {}）",
                                request.pixel.pos.x,
                                request.pixel.pos.y,
                                request.token_lease.uid(),
                                request.pixel.priority
                            );
                        }

                        record_success_paint(&request).await;
                        MultiTokenService::record_local_paint(
                            &request.pixel.pos,
                            self.local_paint_history.clone(),
                            self.local_paint_total.clone(),
                        )
                        .await;

                        MultiTokenService::remove_old_paint_histories(
                            self.local_paint_history.clone(),
                            self.local_paint_total.clone(),
                        )
                        .await;

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

                        record_failed_paint(&request).await;
                    }
                    PaintStatus::InvalidToken => {
                        warn!("绘制失败: Token {} 无效或已过期", request.token_lease.uid());

                        record_failed_paint(&request).await;
                    }
                    _ => {
                        warn!(
                            "绘制失败(token = {}): {:?}",
                            request.token_lease.uid(),
                            paint_result.status
                        );

                        record_failed_paint(&request).await;
                    }
                }
            }
            Err(e) => {
                error!("绘制错误: {:?}", e);
            }
        }
    }
}

async fn record_success_paint(request: &PaintRequest) {
    let metrics = Metrics::get_instance();
    if let Err(e) = metrics {
        error!("获取全局指标存储失败: {:?}", e);
    } else {
        let metrics = metrics.unwrap();
        let mut metrics = metrics.write().await;
        metrics
            .record_global_paint_success(request.token_lease.uid(), request.pixel.pos)
            .await;
    }
}

async fn record_failed_paint(request: &PaintRequest) {
    let metrics = Metrics::get_instance();
    if let Err(e) = metrics {
        error!("获取全局指标存储失败: {:?}", e);
    } else {
        let metrics = metrics.unwrap();
        let mut metrics = metrics.write().await;
        metrics
            .record_global_paint_failure(request.token_lease.uid(), request.pixel.pos)
            .await;
    }
}
