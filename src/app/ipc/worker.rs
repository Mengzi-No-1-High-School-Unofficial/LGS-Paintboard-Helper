//! Sync Worker 实现
//!
//! 从 Master 接收画板数据,使用 writeonly WebSocket 发送绘制请求

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use color_eyre::eyre::{eyre, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

use crate::app::board_sync::LocalBoard;
use winter_paintboard_sdk::Rgb;

use super::metrics::MetricsCollector;
use super::protocol::{MasterMessage, WorkerMessage};
use crate::app::multi_token::pixel_queue::PixelQueue;
use crate::app::multi_token::token_manager::TokenManager;

/// Sync Worker
pub struct SyncWorker {
    socket_path: PathBuf,
    worker_id: String,
}

/// Worker 监控上下文
#[derive(Clone)]
pub struct WorkerMetricsContext {
    pub metrics_collector: Arc<MetricsCollector>,
    pub token_manager: Arc<TokenManager>,
    pub pixel_queue: Arc<PixelQueue>,
}

impl SyncWorker {
    /// 创建新的 Worker
    ///
    /// # 参数
    ///
    /// * `socket_path` - Master 的 Unix socket 路径
    /// * `image_path` - 图片文件路径
    /// * `x` - 绘制起始 X 坐标
    /// * `y` - 绘制起始 Y 坐标
    pub fn new(socket_path: PathBuf, image_path: &std::path::Path, x: u32, y: u32) -> Self {
        // 提取图片名（去除扩展名和路径）
        let image_name = image_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // 清理图片名（移除特殊字符，只保留字母数字和-_）
        let clean_name: String = image_name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();

        // 计算位置哈希（x, y 的组合哈希）
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        x.hash(&mut hasher);
        y.hash(&mut hasher);
        let position_hash = hasher.finish();

        // 生成 Worker ID: {图片名}-{位置哈希}
        let worker_id = format!("{}-{:08x}", clean_name, position_hash as u32);

        Self {
            socket_path,
            worker_id,
        }
    }

    /// 获取 Worker ID
    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    /// 连接到 Master 并返回共享的 LocalBoard (带重试)
    pub async fn connect(&self) -> Result<Arc<LocalBoard>> {
        self.connect_with_retry(5, None).await
    }

    /// 连接到 Master 并启用监控（带重试）
    pub async fn connect_with_metrics(
        &self,
        metrics_ctx: WorkerMetricsContext,
    ) -> Result<Arc<LocalBoard>> {
        self.connect_with_retry(5, Some(metrics_ctx)).await
    }

    /// 带重试的连接方法
    ///
    /// # 参数
    ///
    /// * `max_retries` - 最大重试次数
    /// * `metrics_ctx` - 可选的监控上下文
    pub async fn connect_with_retry(
        &self,
        max_retries: u32,
        metrics_ctx: Option<WorkerMetricsContext>,
    ) -> Result<Arc<LocalBoard>> {
        let mut retry_count = 0;

        loop {
            match self.try_connect(metrics_ctx.clone()).await {
                Ok(board) => {
                    if retry_count > 0 {
                        info!(
                            "Worker connected successfully after {} retries",
                            retry_count
                        );
                    } else {
                        info!("Worker connected successfully");
                    }
                    return Ok(board);
                }
                Err(e) => {
                    retry_count += 1;

                    if retry_count > max_retries {
                        error!("Failed to connect to Master after {} attempts", max_retries);
                        return Err(e);
                    }

                    // 指数退避: 1s, 2s, 4s, 8s, 16s, 最多 30s
                    let backoff_secs = (2u64.pow((retry_count - 1).min(5))).min(30);
                    let backoff = Duration::from_secs(backoff_secs);

                    warn!(
                        "Connection attempt {}/{} failed: {}. Retrying in {:?}...",
                        retry_count, max_retries, e, backoff
                    );

                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    /// 尝试连接(不重试)
    ///
    /// 成功连接后,会启动后台任务持续监控连接状态并在断开后自动重连
    async fn try_connect(
        &self,
        metrics_ctx: Option<WorkerMetricsContext>,
    ) -> Result<Arc<LocalBoard>> {
        info!("Connecting to Master at {:?}...", self.socket_path);

        let stream = UnixStream::connect(&self.socket_path).await?;
        let (reader, writer) = tokio::io::split(stream);

        let local_board = Arc::new(LocalBoard::new(1000, 600));

        // 启动连接管理任务(负责接收数据和断线重连)
        let board = local_board.clone();
        let worker_id = self.worker_id.clone();
        let socket_path = self.socket_path.clone();

        // 第一次直接运行接收循环(使用已建立的连接)
        tokio::spawn(async move {
            info!("Worker connection manager started");

            // 运行首次连接
            if let Err(e) = Self::receive_loop(
                reader,
                writer,
                board.clone(),
                worker_id.clone(),
                metrics_ctx,
            )
            .await
            {
                error!("Initial connection receive loop exited: {}", e);
            }

            // 进入重连循环
            info!("Connection lost. Entering reconnection loop...");

            let mut retry_count = 0;
            loop {
                // 等待后重试
                let backoff_secs = (2u64.pow((retry_count).min(5))).min(30);
                let backoff = Duration::from_secs(backoff_secs);

                info!(
                    "Reconnecting in {:?} (attempt {})...",
                    backoff,
                    retry_count + 1
                );
                tokio::time::sleep(backoff).await;

                match UnixStream::connect(&socket_path).await {
                    Ok(stream) => {
                        info!("Reconnected to Master!");
                        retry_count = 0; // 重置重试计数

                        let (reader, writer) = tokio::io::split(stream);
                        if let Err(e) = Self::receive_loop(
                            reader,
                            writer,
                            board.clone(),
                            worker_id.clone(),
                            None,
                        )
                        .await
                        {
                            error!("Receive loop exited: {}", e);
                        } else {
                            // 正常退出(Master关闭或主动断开)
                            info!("Connection closed normally.");
                            // 如果是 Master 关闭,我们可能也应该退出?
                            // 或者继续等待 Master 重启?
                            // 这里假设继续等待 Master 重启
                        }
                    }
                    Err(e) => {
                        retry_count += 1;
                        warn!("Reconnection failed: {}", e);
                    }
                }
            }
        });

        // 等待初始化完成(仅针对首次连接)
        let timeout = Duration::from_secs(30);
        let start = std::time::Instant::now();
        while !local_board.is_initialized() {
            if start.elapsed() > timeout {
                return Err(eyre!(
                    "Timeout waiting for board initialization from Master"
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        info!("Worker connected and initialized from Master");
        Ok(local_board)
    }

    /// 接收循环
    async fn receive_loop(
        mut reader: tokio::io::ReadHalf<UnixStream>,
        mut writer: tokio::io::WriteHalf<UnixStream>,
        local_board: Arc<LocalBoard>,
        worker_id: String,
        metrics_ctx: Option<WorkerMetricsContext>,
    ) -> Result<()> {
        // 接收第一条消息 (FullBoard)
        let msg = Self::receive_message(&mut reader).await?;
        match msg {
            MasterMessage::FullBoard { pixels, .. } => {
                // 直接从字节更新画板
                local_board.update_from_bytes(&pixels).await;
                info!("Worker received full board from Master");

                // 发送注册消息
                let register = WorkerMessage::Register {
                    worker_id: worker_id.clone(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                };
                Self::send_message(&mut writer, &register).await?;
            }
            _ => {
                return Err(eyre!("Expected FullBoard message"));
            }
        }

        // 启动监控上报任务（如果启用）
        let mut metrics_rx = if let Some(ctx) = metrics_ctx {
            let collector = ctx.metrics_collector.clone();
            let token_manager = ctx.token_manager.clone();
            let pixel_queue = ctx.pixel_queue.clone();

            // 使用 channel 发送监控消息
            let (metrics_tx, metrics_rx) = tokio::sync::mpsc::unbounded_channel();

            // 监控数据采集任务
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut minute_interval = tokio::time::interval(Duration::from_secs(60));

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let metrics = collector.collect_metrics(
                                token_manager.get_total_tokens(),
                                token_manager.get_available_tokens_count(),
                                token_manager.get_cooldown_tokens_count(),
                                pixel_queue.len(),
                                0, // diff_count 暂时为 0
                            );

                            let msg = WorkerMessage::MetricsReport { metrics };
                            let _ = metrics_tx.send(msg);
                        }
                        _ = minute_interval.tick() => {
                            collector.reset_minute_counters();
                        }
                    }
                }
            });

            Some(metrics_rx)
        } else {
            None
        };

        // 持续接收更新
        let mut consecutive_errors = 0;
        const MAX_CONSECUTIVE_ERRORS: u32 = 5;

        loop {
            tokio::select! {
                // 处理来自 Master 的消息
                result = Self::receive_message(&mut reader) => {
                    match result {
                        Ok(msg) => {
                            // 重置错误计数
                            consecutive_errors = 0;

                            match msg {
                                MasterMessage::PixelUpdate { x, y, r, g, b } => {
                                    local_board.update_pixel(x, y, Rgb::new(r, g, b));
                                }
                                MasterMessage::BatchUpdate { updates } => {
                                    let count = updates.len();
                                    for update in updates {
                                        local_board.update_pixel(
                                            update.x,
                                            update.y,
                                            Rgb::new(update.r, update.g, update.b),
                                        );
                                    }
                                    debug!("Worker received batch update with {} pixels", count);
                                }
                                MasterMessage::Heartbeat { timestamp } => {
                                    let ack = WorkerMessage::HeartbeatAck { timestamp };
                                    if let Err(e) = Self::send_message(&mut writer, &ack).await {
                                        warn!("Failed to send heartbeat ack: {}", e);
                                    }
                                }
                                MasterMessage::Shutdown => {
                                    info!("Master shutting down, disconnecting gracefully");
                                    break;
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            consecutive_errors += 1;

                            // 判断错误类型
                            let error_str = e.to_string();
                            let is_connection_lost = error_str.contains("EOF")
                                || error_str.contains("Connection reset")
                                || error_str.contains("Broken pipe");

                            if is_connection_lost {
                                warn!("Connection to Master lost: {}", e);
                                break;
                            } else if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                                error!(
                                    "Too many consecutive errors ({}), disconnecting",
                                    consecutive_errors
                                );
                                break;
                            } else {
                                warn!(
                                    "Recoverable error (attempt {}/{}): {}",
                                    consecutive_errors, MAX_CONSECUTIVE_ERRORS, e
                                );
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                }

                // 处理监控消息上报
                metrics_msg = async {
                    match &mut metrics_rx {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(msg) = metrics_msg {
                        if let Err(e) = Self::send_message(&mut writer, &msg).await {
                            debug!("Failed to send metrics: {}", e);
                        }
                    }
                }
            }
        }

        // 优雅断开连接
        info!("Worker {} disconnecting", worker_id);
        let disconnect = WorkerMessage::Disconnect {
            reason: "Normal shutdown".to_string(),
        };
        let _ = Self::send_message(&mut writer, &disconnect).await;

        Ok(())
    }

    /// 发送消息
    async fn send_message(
        writer: &mut tokio::io::WriteHalf<UnixStream>,
        msg: &WorkerMessage,
    ) -> Result<()> {
        let payload = bincode::serialize(msg)?;
        let len = payload.len() as u32;

        writer.write_u32(len).await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;

        Ok(())
    }

    /// 接收消息
    async fn receive_message(
        reader: &mut tokio::io::ReadHalf<UnixStream>,
    ) -> Result<MasterMessage> {
        let len = reader.read_u32().await?;
        let mut buf = vec![0u8; len as usize];
        reader.read_exact(&mut buf).await?;

        let msg = bincode::deserialize(&buf)?;
        Ok(msg)
    }
}
