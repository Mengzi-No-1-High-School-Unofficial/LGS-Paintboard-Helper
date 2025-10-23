use std::{sync::Arc, time::Duration};

use rustc_hash::FxHashMap;
use tokio::{sync::RwLock, time::sleep};
use tracing::debug;
use winter_paintboard_sdk::models::{Board, Pos, Rgb};

const HEATMAP_EXPIRE_DURATION_MILLS: u64 = 60 * 60 * 1000;  // 1hrs

// 像素状态，区分来源和时间戳
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelSource {
    Own = 0,   // 来自自己的绘制
    Other = 1, // 来自其他用户的绘制
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
    pixels: FxHashMap<Pos, PixelStatus>,
    heatmap: Arc<RwLock<FxHashMap<Pos, u32>>>,
    last_sync_time: Option<std::time::SystemTime>,
    sync_status: SyncStatus,
    is_initialized: bool,
    width: u16,
    height: u16,
    // 添加版本号以跟踪数据更新
    version: u64,
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
            heatmap: Arc::new(RwLock::new(FxHashMap::default())),
            last_sync_time: None,
            sync_status: SyncStatus::Idle,
            is_initialized: false,
            width,
            height,
            version: 0,
        }
    }

    /// 更新像素颜色，保留来源信息和时间戳
    pub fn update_pixel(&mut self, x: u16, y: u16, color: Rgb, source: PixelSource) {
        if x < self.width && y < self.height {
            let current_time = std::time::SystemTime::now();
            let pos = Pos::new(x, y).expect("Invalid coordinates for Pos creation"); // Pos struct ensures valid coordinates

            self.pixels.insert(
                pos.clone(),
                PixelStatus {
                    color,
                    source,
                    timestamp: current_time,
                },
            );

            let heatmap = self.heatmap.clone();
            tokio::spawn(async move {
                {
                    let mut heatmap = heatmap.write().await;
                    heatmap.entry(pos.clone()).and_modify(|count| {
                        *count += 1;
                    });
                }
                
                sleep(Duration::from_millis(HEATMAP_EXPIRE_DURATION_MILLS)).await;

                {
                    let mut heatmap = heatmap.write().await;
                    heatmap.entry(pos).and_modify(|count| {
                        *count -= 1;
                    });
                }
            });

            self.version += 1;
        }
    }

    /// 获取像素颜色
    pub fn get_pixel(&self, x: u16, y: u16) -> Option<Rgb> {
        if x < self.width && y < self.height {
            let pos = Pos::new(x, y).expect("Invalid coordinates for Pos creation");
            self.pixels.get(&pos).map(|pixel| pixel.color)
        } else {
            None
        }
    }

    /// 获取所有像素数据的引用
    pub fn get_pixels(&self) -> &FxHashMap<Pos, PixelStatus> {
        &self.pixels
    }

    /// 获取heatmap的Arc引用，用于外部读取
    pub fn get_heatmap(&self) -> &Arc<RwLock<FxHashMap<Pos, u32>>> {
        &self.heatmap
    }

    /// 从Board对象更新本地数据 - 这是权威数据
    pub fn update_from_board(&mut self, board: &Board) {
        // 全量更新时，服务器数据是绝对权威
        // 但不直接清空，而是对比并更新差异

        // 收集所有需要更新或删除的像素
        let mut updates = Vec::new();
        let mut additions = Vec::new();
        let mut removals = Vec::new();

        // 检查本地存在的像素是否与服务器数据一致，或是否在服务器范围内
        for &pos in self.pixels.keys() {
            let x = pos.x;
            let y = pos.y;
            // 如果像素坐标在服务器画板范围内，则需要验证
            if x < board.width && y < board.height {
                if let Ok(server_pixel) = board.get_pixel(x, y) {
                    let server_color = Rgb::new(server_pixel.r, server_pixel.g, server_pixel.b);
                    // 如果本地存储的颜色与服务器不一致，则记录更新
                    if let Some(local_pixel_status) = self.pixels.get(&pos) {
                        if local_pixel_status.color != server_color {
                            updates.push((
                                pos,
                                PixelStatus {
                                    color: server_color,
                                    source: PixelSource::Other, // 来自服务器的数据视为他人
                                    timestamp: std::time::SystemTime::now(),
                                },
                            ));
                        }
                    }
                } else {
                    // 如果无法从服务器获取像素数据，则记录为待删除
                    removals.push(pos);
                }
            } else {
                // 如果像素坐标超出服务器画板范围，则记录为待删除（理论上不应发生）
                removals.push(pos);
            }
        }

        // 添加服务器有但本地没有的像素
        for y in 0..board.height.min(self.height) {
            for x in 0..board.width.min(self.width) {
                let pos = Pos::new(x, y).expect("Invalid coordinates during board iteration");

                if let Ok(server_pixel) = board.get_pixel(x, y) {
                    let server_color = Rgb::new(server_pixel.r, server_pixel.g, server_pixel.b);
                    // 只记录不存在的像素为待添加
                    if !self.pixels.contains_key(&pos) {
                        additions.push((
                            pos,
                            PixelStatus {
                                color: server_color,
                                source: PixelSource::Other, // 来自服务器的数据视为他人
                                timestamp: std::time::SystemTime::now(),
                            },
                        ));
                    }
                }
            }
        }

        // 记录更新前的版本，用于判断是否有实际更改
        let original_version = self.version;
        let has_changes = !updates.is_empty() || !additions.is_empty() || !removals.is_empty();

        // 执行实际的更新操作
        for (pos, new_status) in &updates {
            self.pixels.insert(pos.clone(), new_status.clone());
        }

        for (pos, new_status) in &additions {
            self.pixels.insert(pos.clone(), new_status.clone());
        }

        for pos in &removals {
            self.pixels.remove(&pos);
        }

        tracing::info!("全量同步完成\n更新：{} 个像素；新增：{} 个像素；删除：{} 个像素", &updates.len(), &additions.len(), &removals.len());

        // 只有当实际发生了更改时，才更新版本号和校验和
        if has_changes {
            self.version += 1;
        }

        self.is_initialized = true;
        self.last_sync_time = Some(std::time::SystemTime::now());
    }

    /// 检查数据完整性
    #[deprecated(note = "不必要的完整性检查，造成性能损耗，改为返回 `true`")]
    pub fn verify_integrity(&self) -> bool {
        true
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

    /// 获取当前版本号
    pub fn version(&self) -> u64 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winter_paintboard_sdk::models::{Board, Pixel};

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
        assert_eq!(board.get_pixel(11, 20), None); // No pixel at (11, 20)
    }

    #[test]
    fn test_update_from_board_applies_server_data() {
        // create a server board and modify a few pixels, then feed to LocalBoard
        let mut server_board = Board::new();
        // set pixel (0,0) to white and (1,0) to red
        server_board
            .set_pixel(0, 0, Pixel::new(255, 255, 255))
            .unwrap();
        server_board.set_pixel(1, 0, Pixel::new(255, 0, 0)).unwrap();

        let mut local = LocalBoard::new(1000, 600);
        // local has different color at (0,0)
        local.update_pixel(0, 0, Rgb::new(0, 0, 0), PixelSource::Own);

        local.update_from_board(&server_board);

        // After updating, local should match server at those positions
        assert_eq!(local.get_pixel(0, 0), Some(Rgb::new(255, 255, 255)));
        assert_eq!(local.get_pixel(1, 0), Some(Rgb::new(255, 0, 0)));
        assert!(local.is_initialized());
    }
}
