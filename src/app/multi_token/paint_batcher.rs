//! `PaintBatcher` 模块实现了绘制操作的批量处理和调度。

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use winter_paintboard_sdk::basic_client::AsyncClient;
use winter_paintboard_sdk::models::PaintOperation;
use winter_paintboard_sdk::PaintboardClientTrait;

/// 批量绘制处理器。
///
/// `PaintBatcher` 在其自己的异步任务中运行，负责从多个 `TokenWorker`
/// 收集独立的 `PaintOperation`，将它们聚合成批次，然后通过 SDK
/// 的 `paint_batch_multi_token` 方法一次性发送。
pub struct PaintBatcher {
    /// 从 `TokenWorker` 接收操作的通道。
    receiver: mpsc::UnboundedReceiver<PaintOperation>,
    /// 用于发送网络请求的共享异步客户端。
    client: Arc<AsyncClient>,
    /// 内部的批处理缓冲区。
    batch: Vec<PaintOperation>,
    /// 批处理大小的上限。
    batch_size_limit: usize,
    /// 批处理发送的时间间隔上限。
    time_limit: Duration,
}

impl PaintBatcher {
    /// 创建一个新的 `PaintBatcher` 实例。
    ///
    /// # 参数
    /// - `receiver`: 用于接收 `PaintOperation` 的 MPSC 通道接收端。
    /// - `client`: 共享的 `AsyncClient` 实例。
    /// - `batch_size_limit`: 当批次中的操作数达到此值时，将触发发送。
    /// - `time_limit`: 自上次发送以来，若超过此时间，将触发发送。
    pub fn new(
        receiver: mpsc::UnboundedReceiver<PaintOperation>,
        client: Arc<AsyncClient>,
        batch_size_limit: usize,
        time_limit: Duration,
    ) -> Self {
        Self {
            receiver,
            client,
            batch: Vec::with_capacity(batch_size_limit),
            batch_size_limit,
            time_limit,
        }
    }

    /// 启动 `PaintBatcher` 的主事件循环。
    ///
    /// 此方法将持续运行，直到所有发送端都关闭。
    pub async fn run(&mut self) {
        log::info!("PaintBatcher 启动，批处理大小限制: {}, 时间限制: {:?}", self.batch_size_limit, self.time_limit);
        let mut interval = tokio::time::interval(self.time_limit);

        loop {
            tokio::select! {
                // 事件 1: 从 Worker 收到一个新的绘制操作
                Some(op) = self.receiver.recv() => {
                    self.batch.push(op);
                    if self.batch.len() >= self.batch_size_limit {
                        self.flush().await;
                        // 刷新后重置定时器，避免立即因超时而再次刷新
                        interval.reset();
                    }
                }

                // 事件 2: 定时器触发
                _ = interval.tick() => {
                    if !self.batch.is_empty() {
                        self.flush().await;
                    }
                }

                // 事件 3: 通道关闭 (所有 Worker 已停止)
                else => {
                    if !self.batch.is_empty() {
                        self.flush().await;
                    }
                    log::info!("PaintBatcher 通道关闭，任务结束。");
                    break;
                }
            }
        }
    }

    /// 发送当前批次中的所有绘制操作。
    async fn flush(&mut self) {
        // 使用 `std::mem::take` 高效地移出当前批次，同时清空 `self.batch`
        let batch_to_send = std::mem::take(&mut self.batch);
        let batch_len = batch_to_send.len();

        log::debug!("刷新批处理，操作数: {}", batch_len);

        // 调用 SDK 的新方法
        match self.client.paint_batch_multi_token(batch_to_send).await {
            Ok(_) => {
                log::info!("成功发送 {} 个绘制操作的批处理", batch_len);
                // 批量发送成功，可以在这里更新全局指标
            }
            Err(e) => {
                // 处理整个批次发送失败的情况，例如网络断开
                log::error!("批量发送失败: {:?}", e);
                // 注意：在这种模式下，失败的操作不会被重试。
                // 上层的比对循环会最终重新将这些像素加入队列。
            }
        }
    }
}