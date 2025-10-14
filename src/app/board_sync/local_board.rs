use log::debug;
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use winter_paintboard_sdk::models::{Board, Rgb};

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