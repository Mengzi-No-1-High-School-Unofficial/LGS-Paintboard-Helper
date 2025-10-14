use log::{error, info, warn, debug};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{interval, Duration};
use winter_paintboard_sdk::event::Event;
use winter_paintboard_sdk::{PaintboardClientTrait};

use super::local_board::{LocalBoard, SyncStatus};

/// 绘版同步管理器
pub struct BoardSyncManager {
    local_board: Arc<Mutex<LocalBoard>>,
    event_receiver: broadcast::Receiver<Event>,
    should_stop: Arc<Mutex<bool>>,
    sync_in_progress: Arc<Mutex<bool>>, // 用于在同步期间暂停事件处理的标志
}

impl BoardSyncManager {
    /// 创建新的同步管理器
    pub fn new(event_bus: &winter_paintboard_sdk::event::EventBus) -> Self {
        Self {
            local_board: Arc::new(Mutex::new(LocalBoard::new(1000, 600))),
            event_receiver: event_bus.subscribe(),
            should_stop: Arc::new(Mutex::new(false)),
            sync_in_progress: Arc::new(Mutex::new(false)),
        }
    }

    /// 获取本地绘版数据的Arc引用
    pub fn local_board(&self) -> Arc<Mutex<LocalBoard>> {
        self.local_board.clone()
    }

    /// 开始全量同步循环
    pub async fn start_sync_loop(
        &self,
        mut client: Box<dyn winter_paintboard_sdk::PaintboardClientTrait + Send>,
        sync_interval: Duration,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let local_board = self.local_board.clone();
        let should_stop = self.should_stop.clone();
        let sync_in_progress = self.sync_in_progress.clone();
        
        tokio::spawn(async move {
            let mut interval_timer = interval(sync_interval);
            
            loop {
                // 检查是否需要停止
                {
                    let stop = should_stop.lock().await;
                    if *stop {
                        break;
                    }
                    drop(stop); // 释放锁
                }
                
                // 等待下一个同步时间点
                interval_timer.tick().await;
                
                // 开始同步前，标记同步进行中
                {
                    let mut sync_flag = sync_in_progress.lock().await;
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
                                let mut board = local_board.lock().await;
                                board.update_from_board(&board_data);
                                board.set_sync_status(SyncStatus::Idle);
                                info!("全量同步完成，获取到 {} 个像素数据", board_data.width as u32 * board_data.height as u32);
                            }
                            success = true;
                        }
                        Err(e) => {
                            attempt += 1;
                            if attempt >= max_retries {
                                error!("全量同步失败，已达到最大重试次数({}): {:?}", max_retries, e);
                                {
                                    let mut board = local_board.lock().await;
                                    board.set_sync_status(SyncStatus::Error(e.to_string()));
                                }
                            } else {
                                warn!("全量同步失败，第 {} 次重试，错误: {:?}", attempt, e);
                                tokio::time::sleep(Duration::from_secs(2_u64.pow(attempt))).await; // 指数退避
                            }
                        }
                    }
                }
                
                // 同步完成后，标记同步结束
                {
                    let mut sync_flag = sync_in_progress.lock().await;
                    *sync_flag = false;
                }
            }
        });
        
        Ok(())
    }

    /// 开始事件监听循环（增量更新）
    pub async fn start_event_listener(&self) -> Result<(), Box<dyn std::error::Error>> {
        let local_board = self.local_board.clone();
        let mut event_receiver = self.event_receiver.resubscribe(); // 使用resubscribe避免错过消息
        let should_stop = self.should_stop.clone();
        let sync_in_progress = self.sync_in_progress.clone();
        
        tokio::spawn(async move {
            loop {
                // 检查是否需要停止
                {
                    let stop = should_stop.lock().await;
                    if *stop {
                        info!("停止事件监听器");
                        break;
                    }
                    drop(stop); // 释放锁
                }
                
                match event_receiver.recv().await {
                    Ok(event) => {
                        // 检查是否正在进行同步，如果是则忽略事件（避免同步过程中数据冲突）
                        {
                            let sync_flag = sync_in_progress.lock().await;
                            if *sync_flag {
                                debug!("同步进行中，忽略事件: {:?}", event);
                                continue; // 跳过事件处理
                            }
                            drop(sync_flag); // 释放锁
                        }
                        
                        // 根据事件类型更新本地数据
                        match event {
                            Event::OwnPaintEvent { pos, color } => {
                                debug!("处理自己的绘制事件: ({}, {}) = {:?}", pos.x, pos.y, color);
                                {
                                    let mut board = local_board.lock().await;
                                    board.update_pixel(pos.x, pos.y, color, crate::app::board_sync::local_board::PixelSource::Own);
                                }
                            },
                            Event::OtherPaintEvent { pos, color } => {
                                debug!("处理他人的绘制事件: ({}, {}) = {:?}", pos.x, pos.y, color);
                                {
                                    let mut board = local_board.lock().await;
                                    board.update_pixel(pos.x, pos.y, color, crate::app::board_sync::local_board::PixelSource::Other);
                                }
                            },
                            Event::HeartbeatEvent => {
                                debug!("收到心跳事件");
                            },
                            Event::ConnectionOpened => {
                                info!("WebSocket连接已建立");
                            },
                            Event::ConnectionClosed => {
                                warn!("WebSocket连接已关闭");
                            },
                            Event::ErrorOccurred(error_msg) => {
                                error!("WebSocket错误: {}", error_msg);
                            }
                        }
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

    /// 停止同步管理器
    pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        {
            let mut should_stop = self.should_stop.lock().await;
            *should_stop = true;
        }
        info!("同步管理器已停止");
        Ok(())
    }
}