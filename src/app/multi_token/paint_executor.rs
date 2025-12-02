//! 绘制执行器模块
//!
//! 该模块实现了绘制请求的队列管理和执行功能，包括请求队列、
//! 绘制执行器和相关的处理逻辑。

use clap::error;
use color_eyre::Report;
use log::{debug, error, info, warn};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::Instant;
use winter_paintboard_sdk::Pos;
use parking_lot::RwLock;

use super::paint_request::PaintRequest;
use crate::app::board_sync::local_board::{LocalBoard, PixelSource};
use crate::app::metrics::Metrics;
use crate::app::multi_token::multi_token_service::MultiTokenService;
use winter_paintboard_sdk::{
    basic_client::AsyncClient, models::PaintStatus, PaintboardClientTrait,
};

/// 绘制请求队列
///
/// 管理绘制请求的发送和接收，使用无界通道实现
pub struct PaintRequestQueue {
    /// 发送端
    sender: mpsc::UnboundedSender<PaintRequest>,
    /// 接收端
    receiver: Arc<Mutex<mpsc::UnboundedReceiver<PaintRequest>>>,
}

impl PaintRequestQueue {
    /// 创建新的绘制请求队列
    ///
    /// # 返回值
    ///
    /// 返回初始化的绘制请求队列实例
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// 发送绘制请求
    ///
    /// # 参数
    ///
    /// * `request` - 绘制请求
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 发送成功
    /// * `Err` - 发送失败及错误信息
    pub fn send(&self, request: PaintRequest) -> Result<(), String> {
        self.sender
            .send(request)
            .map_err(|e| format!("发送绘制请求失败: {}", e))
    }

    /// 接收绘制请求
    ///
    /// # 返回值
    ///
    /// 返回接收到的绘制请求（如果存在）
    pub async fn recv(&self) -> Option<PaintRequest> {
        let mut receiver = self.receiver.lock().await;
        receiver.recv().await
    }
}

/// 单线程绘制执行器
///
/// 负责执行绘制请求，与服务器通信并将结果更新到本地画板
#[derive(Clone)]
pub struct PaintExecutor {
    /// 异步客户端
    client: Arc<AsyncClient>,
    /// 绘制请求队列
    request_queue: Arc<PaintRequestQueue>,
    /// 本地画板引用
    local_board: Arc<LocalBoard>,
    /// 本地绘制历史记录
    local_paint_history: Arc<RwLock<FxHashMap<Pos, Vec<Instant>>>>,
    /// 本地绘制总数
    local_paint_total: Arc<AtomicU64>,
}

impl PaintExecutor {
    /// 创建新的绘制执行器
    ///
    /// # 参数
    ///
    /// * `client` - 异步客户端
    /// * `request_queue` - 绘制请求队列
    /// * `local_board` - 本地画板引用
    /// * `local_paint_history` - 本地绘制历史记录
    /// * `local_paint_total` - 本地绘制总数
    ///
    /// # 返回值
    ///
    /// 返回初始化的绘制执行器实例
    pub fn new(
        client: Arc<AsyncClient>,
        request_queue: Arc<PaintRequestQueue>,
        local_board: Arc<LocalBoard>,
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
    ///
    /// 启动绘制执行循环，持续处理队列中的绘制请求直到收到停止信号
    ///
    /// # 参数
    ///
    /// * `stop_signal` - 停止信号
    pub async fn run(&self, stop_signal: Arc<AtomicBool>) {
        info!("PaintExecutor 启动");

        while !stop_signal.load(Ordering::Acquire) {
            let request = match self.request_queue.recv().await {
                Some(req) => req,
                None => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
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
    ///
    /// 执行具体的绘制操作，处理结果并更新本地数据
    ///
    /// # 参数
    ///
    /// * `request` - 绘制请求
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
                        self.local_board.update_pixel(
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

                        record_success_paint(&request).await;
                        MultiTokenService::record_local_paint(
                            &request.pixel.pos,
                            self.local_paint_history.clone(),
                            self.local_paint_total.clone(),
                        );

                        MultiTokenService::remove_old_paint_histories(
                            self.local_paint_history.clone(),
                            self.local_paint_total.clone(),
                        );

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

/// 记录成功绘制事件
///
/// 更新指标中的成功绘制统计
///
/// # 参数
///
/// * `request` - 绘制请求
async fn record_success_paint(request: &PaintRequest) {
    let metrics = Metrics::get_instance();
    if let Err(e) = metrics {
        error!("获取全局指标存储失败: {:?}", e);
    } else {
        let metrics = metrics.unwrap();
        metrics.record_global_paint_success(request.token_lease.uid(), request.pixel.pos);
    }
}

/// 记录失败绘制事件
///
/// 更新指标中的失败绘制统计
///
/// # 参数
///
/// * `request` - 绘制请求
async fn record_failed_paint(request: &PaintRequest) {
    let metrics = Metrics::get_instance();
    if let Err(e) = metrics {
        error!("获取全局指标存储失败: {:?}", e);
    } else {
        let metrics = metrics.unwrap();
        metrics.record_global_paint_failure(request.token_lease.uid(), request.pixel.pos);
    }
}
