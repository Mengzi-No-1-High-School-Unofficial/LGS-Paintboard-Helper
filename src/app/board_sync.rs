use log::{debug, error, info, warn};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio::time::{interval, Duration};
use winter_paintboard_sdk::event::Event;
use winter_paintboard_sdk::models::{Board, Pos, Rgb};

// 像素状态，区分来源和时间戳
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelSource {
    Own,      // 来自自己的绘制
    Other,    // 来自其他用户的绘制
}

#[derive(Debug, Clone)]
pub struct PixelStatus {
    pub color: Rgb,
    pub source: PixelSource,
    pub timestamp: std::time::SystemTime,
}

// 本地绘版数据结构，使用高性能HashMap存储
#[derive(Debug)]
pub struct LocalBoard {
    // 使用FxHashMap存储像素位置到颜色的映射
    pixels: FxHashMap<(u16, u16), PixelStatus>,
    last_sync_time: Option<std::time::SystemTime>,
    sync_status: SyncStatus,
    is_initialized: bool,
    width: u16,
    height: u16,
}

#[derive(Debug, Clone)]
pub enum SyncStatus {
    Idle,
    Syncing,
    Error(String),
}

impl LocalBoard {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            pixels: FxHashMap::default(),
            last_sync_time: None,
            sync_status: SyncStatus::Idle,
            is_initialized: false,
            width,
            height,
        }
    }

    /// 更新像素颜色，保留来源信息和时间戳
    pub fn update_pixel(&mut self, x: u16, y: u16, color: Rgb, source: PixelSource) {
        if x < self.width && y < self.height {
            let current_time = std::time::SystemTime::now();
            
            // 服务端权威：来自他人的事件（服务端事件）总是优先
            if source == PixelSource::Other {
                // 来自他人的事件，即服务端真实状态，总是更新
                self.pixels.insert(
                    (x, y),
                    PixelStatus {
                        color,
                        source,
                        timestamp: current_time,
                    }
                );
            } else {
                // 来自自己的事件，只有在当前位置不是他人绘制的情况下才更新
                // 这样可以避免自己的绘制覆盖服务端真实状态
                match self.pixels.get(&(x, y)) {
                    Some(existing_pixel) => {
                        // 如果当前位置是由他人绘制的（服务端真实状态），不更新
                        if existing_pixel.source != PixelSource::Other {
                            // 只有当当前位置不是他人绘制时，才更新为自己的绘制
                            self.pixels.insert(
                                (x, y),
                                PixelStatus {
                                    color,
                                    source,
                                    timestamp: current_time,
                                }
                            );
                        } else {
                            debug!("忽略自己的绘制事件，因为服务端显示他人已修改: ({}, {})", x, y);
                        }
                    },
                    None => {
                        // 没有现有记录，直接插入自己的绘制
                        self.pixels.insert(
                            (x, y),
                            PixelStatus {
                                color,
                                source,
                                timestamp: current_time,
                            }
                        );
                    }
                }
            }
        }
    }

    /// 获取像素颜色
    pub fn get_pixel(&self, x: u16, y: u16) -> Option<Rgb> {
        if x < self.width && y < self.height {
            self.pixels.get(&(x, y)).map(|pixel| pixel.color)
        } else {
            None
        }
    }

    /// 获取所有像素数据的引用
    pub fn get_pixels(&self) -> &FxHashMap<(u16, u16), PixelStatus> {
        &self.pixels
    }

    /// 批量更新像素数据
    pub fn update_pixels(&mut self, pixels: Vec<(u16, u16, Rgb)>, source: PixelSource) {
        let current_time = std::time::SystemTime::now();
        for (x, y, color) in pixels {
            if x < self.width && y < self.height {
                self.pixels.insert(
                    (x, y),
                    PixelStatus {
                        color,
                        source,
                        timestamp: current_time,
                    }
                );
            }
        }
        self.is_initialized = true;
        self.last_sync_time = Some(std::time::SystemTime::now());
    }

    /// 从Board对象更新本地数据 - 这是权威数据
    pub fn update_from_board(&mut self, board: &Board) {
        // 全量更新时，服务器数据是绝对权威
        self.pixels.clear();
        
        for y in 0..board.height.min(self.height) {
            for x in 0..board.width.min(self.width) {
                if let Ok(pixel) = board.get_pixel(x, y) {
                    let color = Rgb::new(pixel.r, pixel.g, pixel.b);
                    self.pixels.insert(
                        (x, y),
                        PixelStatus {
                            color,
                            source: PixelSource::Other, // 来自服务器的数据视为他人
                            timestamp: std::time::SystemTime::now(),
                        }
                    );
                }
            }
        }
        
        self.is_initialized = true;
        self.last_sync_time = Some(std::time::SystemTime::now());
    }

    /// 检查是否已初始化
    pub fn is_initialized(&self) -> bool {
        self.is_initialized
    }

    /// 获取同步状态
    pub fn sync_status(&self) -> &SyncStatus {
        &self.sync_status
    }

    /// 设置同步状态
    pub fn set_sync_status(&mut self, status: SyncStatus) {
        self.sync_status = status;
    }

    /// 获取最后同步时间
    pub fn last_sync_time(&self) -> Option<&std::time::SystemTime> {
        self.last_sync_time.as_ref()
    }

    /// 获取绘版尺寸
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
}

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
        mut client: winter_paintboard_sdk::PaintboardClient,
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
                                    board.update_pixel(pos.x, pos.y, color, PixelSource::Own);
                                }
                            },
                            Event::OtherPaintEvent { pos, color } => {
                                debug!("处理他人的绘制事件: ({}, {}) = {:?}", pos.x, pos.y, color);
                                {
                                    let mut board = local_board.lock().await;
                                    board.update_pixel(pos.x, pos.y, color, PixelSource::Other);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_board_creation() {
        let board = LocalBoard::new(1000, 600);
        assert_eq!(board.dimensions(), (1000, 600));
        assert!(!board.is_initialized());
    }

    #[test]
    fn test_local_board_pixel_operations() {
        let mut board = LocalBoard::new(100, 100);
        let test_color = Rgb::new(255, 0, 0);

        board.update_pixel(10, 20, test_color, PixelSource::Own);
        assert_eq!(board.get_pixel(10, 20), Some(test_color));
        assert_eq!(board.get_pixel(11, 20), None);
    }
}