//! `PaintBatcher` 模块实现了绘制操作的批量处理和调度。

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use winter_paintboard_sdk::basic_client::AsyncClient;
use winter_paintboard_sdk::models::PaintOperation;
use winter_paintboard_sdk::PaintboardClientTrait;

use crate::app::ipc::MetricsCollector;

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
    /// 监控数据采集器（可选）
    metrics_collector: Option<Arc<MetricsCollector>>,
}

impl PaintBatcher {
    /// 创建一个新的 `PaintBatcher` 实例。
    ///
    /// # 参数
    /// - `receiver`: 用于接收 `PaintOperation` 的 MPSC 通道接收端。
    /// - `client`: 共享的 `AsyncClient` 实例。
    /// - `batch_size_limit`: 当批次中的操作数达到此值时，将触发发送。
    /// - `time_limit`: 自上次发送以来，若超过此时间，将触发发送。
    /// - `metrics_collector`: 可选的监控数据采集器
    pub fn new(
        receiver: mpsc::UnboundedReceiver<PaintOperation>,
        client: Arc<AsyncClient>,
        batch_size_limit: usize,
        time_limit: Duration,
        metrics_collector: Option<Arc<MetricsCollector>>,
    ) -> Self {
        Self {
            receiver,
            client,
            batch: Vec::with_capacity(batch_size_limit),
            batch_size_limit,
            time_limit,
            metrics_collector,
        }
    }

    /// 启动 `PaintBatcher` 的主事件循环。
    ///
    /// 此方法将持续运行，直到所有发送端都关闭。
    pub async fn run(&mut self) {
        tracing::info!(
            "PaintBatcher 启动，批处理大小限制: {}, 时间限制: {:?}",
            self.batch_size_limit,
            self.time_limit
        );
        let mut interval = tokio::time::interval(self.time_limit);

        loop {
            tokio::select! {
                // 事件 1: 从 Worker 收到一个新的绘制操作
                res = self.receiver.recv() => {
                    match res {
                        Some(op) => {
                            self.batch.push(op);
                            if self.batch.len() >= self.batch_size_limit {
                                self.flush_and_spawn();
                                // 刷新后重置定时器，避免立即因超时而再次刷新
                                interval.reset();
                            }
                        }
                        None => {
                             // 通道关闭，处理最后剩下的批次并退出
                             if !self.batch.is_empty() {
                                 self.flush_and_spawn();
                             }
                             tracing::info!("PaintBatcher 通道已关闭，退出循环");
                             return;
                        }
                    }
                }

                // 事件 2: 定时器触发
                _ = interval.tick() => {
                    if !self.batch.is_empty() {
                        self.flush_and_spawn();
                    }
                }
            }
        }
    }

    /// 将当前批处理任务生成一个独立的 Tokio 任务来执行。
    fn flush_and_spawn(&mut self) {
        if self.batch.is_empty() {
            return;
        }

        // 使用 `std::mem::take` 高效地移出当前批次，为新任务准备数据
        let batch_to_send = std::mem::take(&mut self.batch);
        let client = self.client.clone();
        let metrics_collector = self.metrics_collector.clone();

        // 生成一个新任务来发送批处理
        tokio::spawn(async move {
            let batch_len = batch_to_send.len();
            let start_time = Instant::now();

            tracing::debug!("刷新批处理，操作数: {}", batch_len);

            let result = client.paint_batch_multi_token(batch_to_send).await;

            match result {
                Ok(_) => {
                    let delay_ms = start_time.elapsed().as_millis() as u64;
                    tracing::info!(
                        "成功发送 {} 个绘制操作的批处理，延迟: {}ms",
                        batch_len,
                        delay_ms
                    );

                    // 记录监控数据：批次中的每个操作都算作成功
                    if let Some(collector) = metrics_collector {
                        for _ in 0..batch_len {
                            collector.record_paint_success(delay_ms);
                        }
                    }
                }
                Err(e) => {
                    // 整个批次发送失败，例如网络错误
                    tracing::error!("批量发送失败: {:?}", e);

                    // 记录监控数据：批次中的每个操作都算作失败
                    if let Some(collector) = metrics_collector {
                        for _ in 0..batch_len {
                            collector.record_paint_failure();
                        }
                    }
                }
            }
        });
    }
}
