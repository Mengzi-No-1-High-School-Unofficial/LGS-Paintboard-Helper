//! Sync Master 实现
//!
//! 负责同步画板数据并通过 Unix Socket 分发给 Worker

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use color_eyre::eyre::{eyre, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::app::board_sync::{BoardSyncManager, LocalBoard};
use winter_paintboard_sdk::event::{self, PaintEvent};

use super::metrics::MetricsAggregator;
use super::protocol::{MasterMessage, WorkerMessage};

/// Worker 连接信息
struct WorkerConnection {
    writer: Arc<RwLock<tokio::io::WriteHalf<UnixStream>>>,
    last_heartbeat: Instant,
}

/// Sync Master
pub struct SyncMaster {
    local_board: Arc<LocalBoard>,
    socket_path: PathBuf,
    workers: Arc<RwLock<HashMap<String, WorkerConnection>>>,
    sync_interval: Duration,
    /// 监控数据聚合器（可选）
    metrics_aggregator: Option<Arc<MetricsAggregator>>,
    /// HTTP API 端口（可选）
    api_port: Option<u16>,
}

impl SyncMaster {
    pub fn new(socket_path: PathBuf, sync_interval: Duration) -> Self {
        Self {
            local_board: Arc::new(LocalBoard::new(1000, 600)),
            socket_path,
            workers: Arc::new(RwLock::new(HashMap::new())),
            sync_interval,
            metrics_aggregator: None,
            api_port: None,
        }
    }

    /// 启用监控功能
    ///
    /// # 参数
    ///
    /// * `db_path` - SurrealDB 数据库路径
    /// * `api_port` - HTTP API 端口
    pub async fn with_metrics(mut self, db_path: &str, api_port: u16) -> Result<Self> {
        let aggregator = MetricsAggregator::new(db_path)
            .await
            .map_err(|e| eyre!("Failed to create metrics aggregator: {}", e))?;
        self.metrics_aggregator = Some(Arc::new(aggregator));
        self.api_port = Some(api_port);
        Ok(self)
    }

    /// 启动 Master
    pub async fn start(
        &self,
        client: Arc<dyn winter_paintboard_sdk::PaintboardClientTrait + Send + Sync>,
    ) -> Result<()> {
        info!("Starting Sync Master...");

        // 启动同步循环(带错误恢复)
        let sync_manager = BoardSyncManager::with_board(self.local_board.clone());
        let sync_client = client.clone();
        let sync_interval = self.sync_interval;

        // 准备广播回调
        let master_for_sync = self.clone_inner();
        let on_diff = Arc::new(
            move |changes: Vec<(u32, u32, winter_paintboard_sdk::Rgb)>| {
                if changes.is_empty() {
                    return;
                }

                let master = master_for_sync.clone();

                tokio::spawn(async move {
                    let updates_len = changes.len();
                    if updates_len > 5000 {
                        tracing::warn!("Large diff detected ({}), broadcasting...", updates_len);
                    }

                    let updates: Vec<crate::app::ipc::protocol::PixelUpdateData> = changes
                        .into_iter()
                        .map(|(x, y, rgb)| crate::app::ipc::protocol::PixelUpdateData {
                            x: x as u16,
                            y: y as u16,
                            r: rgb.r,
                            g: rgb.g,
                            b: rgb.b,
                        })
                        .collect();

                    let msg = crate::app::ipc::protocol::MasterMessage::BatchUpdate { updates };

                    // 复用 broadcast_message
                    master.broadcast_message(&msg).await;
                });
            },
        );

        tokio::spawn(async move {
            loop {
                match sync_manager
                    .start_sync_loop(sync_client.clone(), sync_interval, Some(on_diff.clone()))
                    .await
                {
                    Ok(_) => {
                        warn!("Sync loop exited normally. Restarting in 5s...");
                    }
                    Err(e) => {
                        error!("Sync loop error: {}. Restarting in 5s...", e);
                    }
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        // 启动事件监听和广播
        let master = self.clone_inner();
        debug!("SyncMaster: Spawning event broadcast loop");
        tokio::spawn(async move {
            loop {
                if let Err(e) = master.run_event_broadcast().await {
                    error!("Event broadcast error: {}. Restarting in 3s...", e);
                    tokio::time::sleep(Duration::from_secs(3)).await;
                } else {
                    warn!("Event broadcast exited. Restarting in 3s...");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        });

        // 启动心跳循环
        let master = self.clone_inner();
        tokio::spawn(async move {
            master.run_heartbeat_loop().await;
        });

        // 启动 HTTP API 服务器（如果启用）
        if let (Some(aggregator), Some(port)) = (&self.metrics_aggregator, self.api_port) {
            let router = crate::app::api::create_metrics_router(aggregator.clone());
            let addr = format!("0.0.0.0:{}", port);
            info!("Starting HTTP API server on {}", addr);

            let listener = tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| eyre!("Failed to bind API server: {}", e))?;

            tokio::spawn(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    error!("API server error: {}", e);
                }
            });
        }

        // 启动 Socket 服务器 (阻塞)
        self.run_socket_server().await
    }

    fn clone_inner(&self) -> SyncMasterInner {
        SyncMasterInner {
            local_board: self.local_board.clone(),
            workers: self.workers.clone(),
            metrics_aggregator: self.metrics_aggregator.clone(),
        }
    }

    /// 运行 Unix Socket 服务器
    async fn run_socket_server(&self) -> Result<()> {
        // 删除旧的 socket 文件
        let _ = std::fs::remove_file(&self.socket_path);

        let listener = UnixListener::bind(&self.socket_path)?;
        info!("Master listening on {:?}", self.socket_path);

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let master = self.clone_inner();
                    tokio::spawn(async move {
                        if let Err(e) = master.handle_worker(stream).await {
                            error!("Worker handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Accept error: {}", e);
                }
            }
        }
    }
}

impl Drop for SyncMaster {
    fn drop(&mut self) {
        // 清理 socket 文件
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Master 内部结构,用于异步任务
#[derive(Clone)]
struct SyncMasterInner {
    local_board: Arc<LocalBoard>,
    workers: Arc<RwLock<HashMap<String, WorkerConnection>>>,
    metrics_aggregator: Option<Arc<MetricsAggregator>>,
}

impl SyncMasterInner {
    /// 处理 Worker 连接
    async fn handle_worker(&self, stream: UnixStream) -> Result<()> {
        let (reader, writer) = tokio::io::split(stream);
        let reader = Arc::new(RwLock::new(reader));
        let writer = Arc::new(RwLock::new(writer));

        // 1. 发送完整画板
        let board_data = self.local_board.to_bytes().await;
        let msg = MasterMessage::FullBoard {
            width: 1000,
            height: 600,
            pixels: board_data,
        };
        self.send_message(&writer, &msg).await?;
        info!("Sent full board to new worker");

        // 2. 等待注册
        let register_msg = self.receive_message(&reader).await?;
        let worker_id = match register_msg {
            WorkerMessage::Register { worker_id, version } => {
                info!("Worker {} (v{}) registered", worker_id, version);
                worker_id
            }
            _ => {
                return Err(eyre!("Expected Register message"));
            }
        };

        // 3. 添加到 Worker 列表
        {
            let mut workers = self.workers.write().await;
            workers.insert(
                worker_id.clone(),
                WorkerConnection {
                    writer: writer.clone(),
                    last_heartbeat: Instant::now(),
                },
            );
        }

        // 4. 接收 Worker 消息
        loop {
            match self.receive_message(&reader).await {
                Ok(WorkerMessage::HeartbeatAck { .. }) => {
                    let mut workers = self.workers.write().await;
                    if let Some(worker) = workers.get_mut(&worker_id) {
                        worker.last_heartbeat = Instant::now();
                    }
                }
                Ok(WorkerMessage::MetricsReport { metrics }) => {
                    // 处理监控数据上报
                    if let Some(aggregator) = &self.metrics_aggregator {
                        if let Err(e) = aggregator.record_metrics(metrics).await {
                            warn!("Failed to record metrics from {}: {}", worker_id, e);
                        }
                    }
                }
                Ok(WorkerMessage::Disconnect { reason }) => {
                    info!("Worker {} disconnecting: {}", worker_id, reason);
                    break;
                }
                Err(e) => {
                    debug!("Worker {} error: {}", worker_id, e);
                    break;
                }
                _ => {}
            }
        }

        // 5. 清理
        self.workers.write().await.remove(&worker_id);
        if let Some(aggregator) = &self.metrics_aggregator {
            aggregator.remove_worker(&worker_id);
        }
        info!("Worker {} disconnected", worker_id);

        Ok(())
    }

    /// 运行事件广播循环
    async fn run_event_broadcast(&self) -> Result<()> {
        let mut receiver = event::subscribe();
        debug!("SyncMaster: Event broadcast loop started and subscribed to global event bus");

        loop {
            match receiver.recv().await {
                Ok(event) => match event {
                    PaintEvent::PixelUpdate { pos, color } => {
                        debug!(
                            "Master: 收到实时像素更新 ({}, {}) -> {:?}",
                            pos.x, pos.y, color
                        );
                        let msg = MasterMessage::PixelUpdate {
                            x: pos.x,
                            y: pos.y,
                            r: color.r,
                            g: color.g,
                            b: color.b,
                        };
                        self.broadcast_message(&msg).await;
                    }
                    PaintEvent::Success { pos, color, uid } => {
                        debug!(
                            "Master: 收到成功像素更新 ({}, {}) -> {:?}",
                            pos.x, pos.y, color
                        );
                        let msg = MasterMessage::PixelUpdate {
                            x: pos.x,
                            y: pos.y,
                            r: color.r,
                            g: color.g,
                            b: color.b,
                        };
                        self.broadcast_message(&msg).await;
                    }
                    _ => {}
                },
                Err(e) => {
                    warn!("Event receive error: {:?}", e);
                }
            }
        }
    }

    /// 运行心跳循环
    async fn run_heartbeat_loop(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let msg = MasterMessage::Heartbeat { timestamp };
            self.broadcast_message(&msg).await;

            // 清理超时的 Worker
            let mut workers = self.workers.write().await;
            let timeout = Duration::from_secs(90);
            workers.retain(|id, worker| {
                if worker.last_heartbeat.elapsed() > timeout {
                    warn!("Worker {} heartbeat timeout, removing", id);
                    false
                } else {
                    true
                }
            });
        }
    }

    /// 广播消息给所有 Worker
    async fn broadcast_message(&self, msg: &MasterMessage) {
        let workers = self.workers.read().await;
        for (id, worker) in workers.iter() {
            if let Err(e) = self.send_message(&worker.writer, msg).await {
                warn!("Failed to send to {}: {}", id, e);
            }
        }
    }

    /// 发送消息
    async fn send_message(
        &self,
        writer: &Arc<RwLock<tokio::io::WriteHalf<UnixStream>>>,
        msg: &MasterMessage,
    ) -> Result<()> {
        let payload = bincode::serialize(msg)?;
        let len = payload.len() as u32;

        let mut writer = writer.write().await;
        writer.write_u32(len).await?;
        writer.write_all(&payload).await?;
        writer.flush().await?;

        Ok(())
    }

    /// 接收消息
    async fn receive_message(
        &self,
        reader: &Arc<RwLock<tokio::io::ReadHalf<UnixStream>>>,
    ) -> Result<WorkerMessage> {
        let mut reader = reader.write().await;

        let len = reader.read_u32().await?;
        let mut buf = vec![0u8; len as usize];
        reader.read_exact(&mut buf).await?;

        let msg = bincode::deserialize(&buf)?;
        Ok(msg)
    }
}
