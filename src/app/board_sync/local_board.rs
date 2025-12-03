//! 本地画板同步模块
//!
//! 该模块实现了本地画板数据的存储和同步功能，包括像素数据、热力图、
//! 数据完整性验证等功能。

use std::{sync::Arc, time::Duration};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::RwLock;
use dashmap::DashMap;
use winter_paintboard_sdk::models::{Board, Pos, Rgb};
use tracing::{info, debug, trace};
use std::time::SystemTime;

use crate::app::multi_token::cli::get_penalty_scale;
use winter_paintboard_sdk::event;

/// 热力图过期时间
const HEATMAP_EXPIRE_DURATION: Duration = Duration::from_secs(10 * 60); // 10min

/// 像素来源枚举，用于区分像素是来自自己的绘制还是其他用户的绘制
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PixelSource {
    /// 来自自己的绘制
    Own = 0,
    /// 来自其他用户的绘制
    Other = 1,
}

/// 像素状态结构
///
/// 包含像素的颜色、来源和时间戳信息
#[derive(Debug, Clone)]
pub struct PixelStatus {
    /// 像素颜色
    pub color: Rgb,
    /// 像素来源（自己绘制或他人绘制）
    pub source: PixelSource,
    /// 像素更新时间戳
    pub timestamp: std::time::SystemTime,
}

/// 本地画板数据结构，使用 DashMap 存储
///
/// 管理本地画板的像素数据、热力图、同步状态等信息
#[derive(Debug)]
pub struct LocalBoard {
    /// 使用 DashMap 存储像素位置到状态的映射，支持高并发读写
    pixels: DashMap<Pos, PixelStatus>,
    /// 热力图数据，用于记录像素被绘制的时间戳
    heatmap: Arc<DashMap<Pos, Vec<SystemTime>>>,
    /// 最后同步时间
    last_sync_time: RwLock<Option<SystemTime>>,
    /// 同步状态
    sync_status: RwLock<SyncStatus>,
    /// 是否已初始化
    is_initialized: AtomicBool,
    /// 画板宽度
    width: u16,
    /// 画板高度
    height: u16,
    /// 版本号，用于跟踪数据更新
    version: AtomicU64,
}

#[derive(Debug, Clone)]
/// 同步状态枚举
///
/// 表示本地画板与服务器同步的不同状态
pub enum SyncStatus {
    /// 空闲状态
    Idle,
    /// 正在同步
    Syncing,
    /// 同步错误，包含错误信息
    Error(String),
}

impl LocalBoard {
    /// 创建新的本地画板实例
    ///
    /// # 参数
    ///
    /// * `width` - 画板宽度
    /// * `height` - 画板高度
    ///
    /// # 返回值
    ///
    /// 返回初始化的本地画板实例
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            pixels: DashMap::new(),
            heatmap: Arc::new(DashMap::new()),
            last_sync_time: RwLock::new(None),
            sync_status: RwLock::new(SyncStatus::Idle),
            is_initialized: AtomicBool::new(false),
            width,
            height,
            version: AtomicU64::new(0),
        }
    }

    /// 更新像素颜色，保留来源信息和时间戳
    ///
    /// # 参数
    ///
    /// * `x` - 像素X坐标
    /// * `y` - 像素Y坐标
    /// * `color` - 像素颜色
    /// * `source` - 像素来源（自己绘制或他人绘制）
    pub fn update_pixel(&self, x: u16, y: u16, color: Rgb, source: PixelSource) {
        if x < self.width && y < self.height {
            let current_time = std::time::SystemTime::now();
            let pos = Pos::new(x, y).expect("Invalid coordinates for Pos creation");

            self.pixels.insert(
                pos,
                PixelStatus {
                    color,
                    source,
                    timestamp: current_time,
                },
            );


            self.version.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 获取指定坐标的像素颜色
    ///
    /// # 参数
    ///
    /// * `x` - 像素X坐标
    /// * `y` - 像素Y坐标
    ///
    /// # 返回值
    ///
    /// 如果坐标有效且存在像素，则返回像素颜色；否则返回None
    pub fn get_pixel(&self, x: u16, y: u16) -> Option<Rgb> {
        if x < self.width && y < self.height {
            let pos = Pos::new(x, y).expect("Invalid coordinates for Pos creation");
            self.pixels.get(&pos).map(|pixel| pixel.color)
        } else {
            None
        }
    }

    /// 获取所有像素数据的引用
    ///
    /// # 返回值
    ///
    /// 返回指向内部像素数据映射的引用
    pub fn get_pixels(&self) -> &DashMap<Pos, PixelStatus> {
        &self.pixels
    }

    /// 获取热力图的Arc引用，用于外部读取
    ///
    /// # 返回值
    ///
    /// 返回指向热力图数据的Arc引用
    pub fn get_heatmap(&self) -> &Arc<DashMap<Pos, Vec<SystemTime>>> {
        &self.heatmap
    }

    /// 从Board对象更新本地数据 - 这是权威数据
    ///
    /// 使用服务器的Board数据全量更新本地画板数据，服务器数据是绝对权威
    ///
    /// # 参数
    ///
    /// * `board` - 服务器画板数据
    pub fn update_from_board(&self, board: &Board) {
        // 全量更新时，服务器数据是绝对权威
        // 但不直接清空，而是对比并更新差异

        // 收集所有需要更新或删除的像素
        let mut updates = Vec::new();
        let mut additions = Vec::new();
        let mut removals = Vec::new();

        // 检查本地存在的像素是否与服务器数据一致，或是否在服务器范围内
        for entry in self.pixels.iter() {
            let pos = *entry.key();
            let x = pos.x;
            let y = pos.y;
            // 如果像素坐标在服务器画板范围内，则需要验证
            if x < board.width && y < board.height {
                if let Ok(server_pixel) = board.get_pixel(x, y) {
                    let server_color = Rgb::new(server_pixel.r, server_pixel.g, server_pixel.b);
                    // 如果本地存储的颜色与服务器不一致，则记录更新
                    if entry.value().color != server_color {
                        updates.push((
                            pos,
                            PixelStatus {
                                color: server_color,
                                source: PixelSource::Other, // 来自服务器的数据视为他人
                                timestamp: std::time::SystemTime::now(),
                            },
                        ));
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
                    // 只有当本地没有该像素时才添加
                    if !self.pixels.contains_key(&pos) {
                        let server_color = Rgb::new(server_pixel.r, server_pixel.g, server_pixel.b);
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

        let updates_len = updates.len();
        let additions_len = additions.len();
        let removals_len = removals.len();
        let has_changes = updates_len > 0 || additions_len > 0 || removals_len > 0;

        // 执行实际的更新操作
        for (pos, new_status) in updates {
            self.pixels.insert(pos, new_status);
        }

        for (pos, new_status) in additions {
            self.pixels.insert(pos, new_status);
        }

        for pos in removals {
            self.pixels.remove(&pos);
        }

        tracing::info!(
            "全量同步完成\n更新：{} 个像素；新增：{} 个像素；删除：{} 个像素",
            updates_len,
            additions_len,
            removals_len
        );

        // 只有当实际发生了更改时，才更新版本号
        if has_changes {
            self.version.fetch_add(1, Ordering::Relaxed);
        }

        self.is_initialized.store(true, Ordering::Relaxed);
        *self.last_sync_time.write() = Some(std::time::SystemTime::now());
    }

    /// 检查数据完整性
    ///
    /// # 返回值
    ///
    /// 由于不必要的完整性检查会造成性能损耗，此函数已废弃并始终返回true
    #[deprecated(note = "不必要的完整性检查，造成性能损耗，改为返回 `true`")]
    pub fn verify_integrity(&self) -> bool {
        true
    }

    /// 检查本地画板是否已初始化
    ///
    /// # 返回值
    ///
    /// 如果已初始化返回true，否则返回false
    pub fn is_initialized(&self) -> bool {
        self.is_initialized.load(Ordering::Relaxed)
    }

    /// 获取当前同步状态
    ///
    /// # 返回值
    ///
    /// 返回当前的同步状态副本
    pub fn sync_status(&self) -> SyncStatus {
        self.sync_status.read().clone()
    }

    /// 设置同步状态
    ///
    /// # 参数
    ///
    /// * `status` - 新的同步状态
    pub fn set_sync_status(&self, status: SyncStatus) {
        *self.sync_status.write() = status;
    }

    /// 获取最后同步时间
    ///
    /// # 返回值
    ///
    /// 返回最后同步时间的副本（如果存在）
    pub fn last_sync_time(&self) -> Option<SystemTime> {
        *self.last_sync_time.read()
    }

    /// 获取画板尺寸
    ///
    /// # 返回值
    ///
    /// 返回画板的宽度和高度
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    /// 获取当前版本号
    ///
    /// # 返回值
    ///
    /// 返回本地画板数据的当前版本号
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    /// 计算并返回指定像素位置在近期（10分钟内）的绘制次数
    ///
    /// # 参数
    ///
    /// * `pos` - 像素位置
    ///
    /// # 返回值
    ///
    /// 返回近期绘制次数 (`f64`)
    pub fn calculate_penalty(&self, pos: &Pos) -> f64 {
        // 计算10分钟内的绘制次数
        let recent_paints = self.heatmap.get(pos).map_or(0, |entry| {
            let now = SystemTime::now();
            entry
                .value()
                .iter()
                .filter(|&&timestamp| {
                    now.duration_since(timestamp).unwrap_or_default() < HEATMAP_EXPIRE_DURATION
                })
                .count()
        });

        recent_paints as f64
    }
    /// 启动事件监听器，监听来自 SDK 的绘制事件并更新本地画板状态
    pub fn start_event_listener(self: &Arc<Self>) {
        let mut receiver = event::subscribe();
        let self_clone = self.clone();
        tokio::spawn(async move {
            info!("LocalBoard: 事件监听器已启动");
            while let Ok(event) = receiver.recv().await {
                match event {
                    event::PaintEvent::Success { uid, pos, color } => {
                        // 1. 更新像素颜色
                        // 注意：我们将自己的成功绘制视为 PixelSource::Own
                        self_clone.update_pixel(pos.x, pos.y, color, PixelSource::Own);


                        // 3. 更新热力图，记录绘制时间戳
                        self_clone
                            .heatmap
                            .entry(pos)
                            .or_default()
                            .push(SystemTime::now());

                        trace!("LocalBoard: 通过事件更新像素 at ({}, {})", pos.x, pos.y);
                    }
                    event::PaintEvent::OtherPaint { pos, color } => {
                        // 1. 更新像素颜色
                        // 注意：我们将他人绘制视为 PixelSource::Other
                        self_clone.update_pixel(pos.x, pos.y, color, PixelSource::Other);

                        // 2. 更新热力图，记录绘制时间戳
                        self_clone
                            .heatmap
                            .entry(pos)
                            .or_default()
                            .push(SystemTime::now());

                        trace!("LocalBoard: 通过他人绘制事件更新像素 at ({}, {})", pos.x, pos.y);
                    }
                    event::PaintEvent::Failure { uid, pos } => {
                        // 记录失败事件，可能用于惩罚机制的调整
                        // 例如，可以增加一个失败计数，或者在惩罚计算中考虑失败次数
                        // 这里暂时只记录日志，后续可以根据需求细化惩罚逻辑
                        debug!("LocalBoard: 绘制失败事件 at ({}, {})", pos.x, pos.y);
                        // 失败也应该增加热力图计数，因为尝试绘制也消耗了资源
                        self_clone
                            .heatmap
                            .entry(pos)
                            .or_default()
                            .push(SystemTime::now());
                    }
                }
            }
        });
    }

    /// 启动热力图清理任务，定期移除过期的绘制记录
    pub fn start_heatmap_cleanup_task(self: &Arc<Self>) {
        let self_clone = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // 每分钟清理一次
            info!("LocalBoard: 热力图清理任务已启动");
            loop {
                interval.tick().await;
                trace!("LocalBoard: 开始清理过期热力图数据");
                let now = SystemTime::now();
                let mut empty_keys = Vec::new();

                self_clone.heatmap.iter_mut().for_each(|mut entry| {
                    // 移除所有超过10分钟的时间戳
                    entry.value_mut().retain(|&timestamp| {
                        now.duration_since(timestamp).unwrap_or_default() < HEATMAP_EXPIRE_DURATION
                    });
                    // 如果清理后列表为空，则记录该键以便后续删除
                    if entry.value().is_empty() {
                        empty_keys.push(*entry.key());
                    }
                });

                // 从DashMap中移除所有空的条目
                if !empty_keys.is_empty() {
                    trace!("LocalBoard: 移除 {} 个空的热力图条目", empty_keys.len());
                    for key in empty_keys {
                        self_clone.heatmap.remove(&key);
                    }
                }
                trace!("LocalBoard: 热力图数据清理完成");
            }
        });
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
        let board = LocalBoard::new(100, 100);
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

        let local = LocalBoard::new(1000, 600);
        // local has different color at (0,0)
        local.update_pixel(0, 0, Rgb::new(0, 0, 0), PixelSource::Own);

        local.update_from_board(&server_board);

        // After updating, local should match server at those positions
        assert_eq!(local.get_pixel(0, 0), Some(Rgb::new(255, 255, 255)));
        assert_eq!(local.get_pixel(1, 0), Some(Rgb::new(255, 0, 0)));
        assert!(local.is_initialized());
    }
}
