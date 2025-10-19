use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tokio::time::{interval, Duration};
use winter_paintboard_sdk::event::Event;
use winter_paintboard_sdk::PaintboardClientTrait;

use super::local_board::{LocalBoard, PixelSource, SyncStatus};

/// 绘版同步管理器
#[derive(Clone)]
pub struct BoardSyncManager {
    local_board: Arc<RwLock<LocalBoard>>,
    event_bus: Arc<winter_paintboard_sdk::event::EventBus>, // 使用EventBus而不是Receiver
    should_stop: Arc<RwLock<bool>>,
    sync_in_progress: Arc<RwLock<bool>>, // 用于在同步期间暂停事件处理的标志
    pending_events: Arc<Mutex<Vec<Event>>>, // 缓存同步期间收到的事件
}

impl BoardSyncManager {
    /// 创建新的同步管理器
    pub fn new(event_bus: &winter_paintboard_sdk::event::EventBus) -> Self {
        Self {
            local_board: Arc::new(RwLock::new(LocalBoard::new(1000, 600))),
            event_bus: Arc::new(event_bus.clone()),
            should_stop: Arc::new(RwLock::new(false)),
            sync_in_progress: Arc::new(RwLock::new(false)),
            pending_events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// 获取本地绘版数据的Arc引用
    pub fn local_board(&self) -> Arc<RwLock<LocalBoard>> {
        self.local_board.clone()
    }

    /// 开始全量同步循环
    pub async fn start_sync_loop(
        &self,
        client: Box<dyn winter_paintboard_sdk::PaintboardClientTrait + Send>,
        sync_interval: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sync_manager = self.clone(); // 将self包装为Arc以在异步任务中使用
        let client = client; // 确保client是可变的

        tokio::spawn(async move {
            let mut interval_timer = interval(sync_interval);

            loop {
                // 检查是否需要停止
                {
                    let stop = sync_manager.should_stop.read().await;
                    if *stop {
                        break;
                    }
                    drop(stop); // 释放锁
                }

                // 等待下一个同步时间点
                interval_timer.tick().await;

                // 开始同步前，标记同步进行中
                {
                    let mut sync_flag = sync_manager.sync_in_progress.write().await;
                    *sync_flag = true;
                    info!("开始全量同步绘版数据...");
                }

                // 尝试同步，带重试机制
                let mut attempt = 0;
                let max_retries = 3;
                let mut success = false;

                while attempt < max_retries && !success {
                    match client.get_board().await {
                        Ok(board_data) => {
                            {
                                let mut board = sync_manager.local_board.write().await;
                                board.update_from_board(&board_data);
                                board.set_sync_status(SyncStatus::Idle);
                                info!(
                                    "全量同步完成，获取到 {} 个像素数据",
                                    board_data.width as u32 * board_data.height as u32
                                );
                            }
                            success = true;
                        }
                        Err(e) => {
                            attempt += 1;
                            if attempt >= max_retries {
                                error!(
                                    "全量同步失败，已达到最大重试次数({}): {:?}",
                                    max_retries, e
                                );
                                {
                                    let mut board = sync_manager.local_board.write().await;
                                    board.set_sync_status(SyncStatus::Error(e.to_string()));
                                }
                            } else {
                                warn!("全量同步失败，第 {} 次重试，错误: {:?}", attempt, e);
                                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await;
                                // 指数退避
                            }
                        }
                    }
                }

                // 同步完成后，标记同步结束
                {
                    let mut sync_flag = sync_manager.sync_in_progress.write().await;
                    *sync_flag = false;
                }

                // 应用在同步期间缓存的事件
                sync_manager.apply_pending_events().await;
            }
        });

        Ok(())
    }

    /// 开始增量同步循环 - 只同步发生变化的区域
    pub async fn start_incremental_sync_loop(
        &self,
        client: Box<dyn winter_paintboard_sdk::PaintboardClientTrait + Send>,
        sync_interval: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let sync_manager = self.clone();
        let client = client; // 确保client是可变的

        // 不依赖本地版本号进行判断，而是总是获取服务器数据并进行比较
        // 或者可以记录服务器数据的某种标识（如校验和）来判断是否有变化

        tokio::spawn(async move {
            let mut interval_timer = interval(sync_interval);

            loop {
                // 检查是否需要停止
                {
                    let stop = sync_manager.should_stop.read().await;
                    if *stop {
                        break;
                    }
                    drop(stop); // 释放锁
                }

                // 等待下一个同步时间点
                interval_timer.tick().await;

                // 开始同步前，标记同步进行中
                {
                    let mut sync_flag = sync_manager.sync_in_progress.write().await;
                    *sync_flag = true;
                    info!("开始增量同步绘版数据...");
                }

                // 尝试获取服务器数据
                match client.get_board().await {
                    Ok(board_data) => {
                        let mut board = sync_manager.local_board.write().await;

                        // 执行差异同步（update_from_board 方法会进行实际的差异比较和更新）
                        board.update_from_board(&board_data);

                        // 验证同步后的数据一致性
                        if !board.verify_integrity() {
                            warn!("数据完整性验证失败，可能需要全量同步");
                        }

                        board.set_sync_status(SyncStatus::Idle);
                        info!("增量同步完成，处理了服务器数据更新");
                    }
                    Err(e) => {
                        error!("增量同步失败: {:?}", e);
                        {
                            let mut board = sync_manager.local_board.write().await;
                            board.set_sync_status(SyncStatus::Error(e.to_string()));
                        }
                    }
                }

                // 同步完成后，标记同步结束
                {
                    let mut sync_flag = sync_manager.sync_in_progress.write().await;
                    *sync_flag = false;
                }

                // 应用在同步期间缓存的事件
                sync_manager.apply_pending_events().await;
            }
        });

        Ok(())
    }

    /// 开始事件监听循环（增量更新）
    pub async fn start_event_listener(&self) -> Result<(), Box<dyn std::error::Error>> {
        let local_board = self.local_board.clone();
        // 在事件监听器内部创建新的 Receiver
        let mut event_receiver = self.event_bus.subscribe();
        let should_stop = self.should_stop.clone();
        let sync_in_progress = self.sync_in_progress.clone();
        let pending_events = self.pending_events.clone();

        tokio::spawn(async move {
            loop {
                // 检查是否需要停止
                {
                    let stop = should_stop.read().await;
                    if *stop {
                        info!("停止事件监听器");
                        break;
                    }
                    drop(stop); // 释放锁
                }

                match event_receiver.recv().await {
                    Ok(event) => {
                        // 检查是否正在进行同步
                        {
                            let sync_flag = sync_in_progress.read().await;
                            if *sync_flag {
                                // 在同步期间，将事件添加到待处理列表中
                                let mut pending = pending_events.lock().await;
                                pending.push(event.clone());
                                // debug!("同步进行中，缓存事件: {:?}", event);
                                continue; // 跳过事件处理
                            }
                            drop(sync_flag); // 释放锁
                        }

                        // 根据事件类型更新本地数据
                        Self::process_event(&local_board, event).await;
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        error!("事件接收器已关闭");
                        break;
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("事件接收滞后，跳过了 {} 个事件", skipped);
                    }
                }
            }
        });

        Ok(())
    }

    /// 处理单个事件
    async fn process_event(local_board: &Arc<RwLock<LocalBoard>>, event: Event) {
        match event {
            Event::OwnPaintEvent { pos, color } => {
                // debug!("😊 处理自己的绘制事件: ({}, {}) = {:?}", pos.x, pos.y, color);
                {
                    let mut board = local_board.write().await;
                    board.update_pixel(pos.x, pos.y, color, PixelSource::Own);
                }
            }
            Event::OtherPaintEvent { pos, color } => {
                // debug!("👀 处理他人的绘制事件: ({}, {}) = {:?}", pos.x, pos.y, color);
                {
                    let mut board = local_board.write().await;
                    board.update_pixel(pos.x, pos.y, color, PixelSource::Other);
                }
            }
            Event::HeartbeatEvent => {
                debug!("收到心跳事件");
            }
            Event::ConnectionOpened => {
                info!("WebSocket连接已建立");
            }
            Event::ConnectionClosed => {
                warn!("WebSocket连接已关闭");
            }
            Event::ErrorOccurred(error_msg) => {
                error!("WebSocket错误: {}", error_msg);
            }
            Event::ConnectionClosedWithCode(code) => {
                warn!("WebSocket连接已关闭，状态码: {}", code);
            }
        }
    }

    /// 应用缓存的事件
    async fn apply_pending_events(&self) {
        let pending_events = {
            let mut pending = self.pending_events.lock().await;
            pending.drain(..).collect::<Vec<_>>()
        };

        if !pending_events.is_empty() {
            info!("应用 {} 个缓存的事件", pending_events.len());
            for event in pending_events {
                Self::process_event(&self.local_board, event).await;
            }
        }
    }

    /// 停止同步管理器
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut should_stop = self.should_stop.write().await;
            *should_stop = true;
        }
        info!("同步管理器已停止");
        Ok(())
    }
}
