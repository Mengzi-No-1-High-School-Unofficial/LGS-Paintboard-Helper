//! 本地画板同步模块
//!
//! 该模块实现了本地画板数据的存储和同步功能，包括像素数据、热力图、
//! 数据完整性验证等功能。

use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use std::time::SystemTime;
use tracing::{debug, info, trace, warn};
use winter_paintboard_sdk::models::{Board, Pos, Rgb};

use winter_paintboard_sdk::event;

/// 热力图过期时间
const HEATMAP_EXPIRE_DURATION: Duration = Duration::from_secs(10 * 60); // 10min

/// 像素状态结构
#[derive(Debug, Clone)]
pub struct PixelStatus {
    /// 像素颜色
    pub color: Rgb,
}

/// 本地画板数据结构，使用 DashMap 存储
///
/// 管理本地画板的像素数据、热力图、同步状态等信息
#[derive(Debug)]
pub struct LocalBoard {
    /// 使用 DashMap 存储像素位置到状态的映射，支持高并发读写
    pixels: Arc<DashMap<Pos, PixelStatus>>,
    /// 热力图数据，用于记录像素被绘制的时间戳
    heatmap: Arc<DashMap<Pos, Vec<SystemTime>>>,
    /// 是否已初始化
    is_initialized: AtomicBool,
    /// 画板宽度
    width: u16,
    /// 画板高度
    height: u16,
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
            pixels: Arc::new(DashMap::new()),
            heatmap: Arc::new(DashMap::new()),
            is_initialized: AtomicBool::new(false),
            width,
            height,
        }
    }

    /// 更新像素颜色
    ///
    /// # 参数
    ///
    /// * `x` - 像素X坐标
    /// * `y` - 像素Y坐标
    /// * `color` - 像素颜色
    pub fn update_pixel(&self, x: u16, y: u16, color: Rgb) {
        if x < self.width && y < self.height {
            let pos = Pos::new(x, y).expect("Invalid coordinates for Pos creation");
            self.pixels.insert(pos, PixelStatus { color });
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
                let pos = Pos::new(x, y);

                if let Err(e) = pos {
                    warn!("Failed to parse pos ({}, {}), ignoring: {:?}", x, y, e);
                    continue;
                };

                let pos = pos.unwrap();

                if let Ok(server_pixel) = board.get_pixel(x, y) {
                    // 只有当本地没有该像素时才添加
                    if !self.pixels.contains_key(&pos) {
                        let server_color = Rgb::new(server_pixel.r, server_pixel.g, server_pixel.b);
                        additions.push((
                            pos,
                            PixelStatus {
                                color: server_color,
                            },
                        ));
                    }
                }
            }
        }

        let updates_len = updates.len();
        let additions_len = additions.len();
        let removals_len = removals.len();
        let _has_changes = updates_len > 0 || additions_len > 0 || removals_len > 0;

        // 执行实际的更新操作并标记为最近变更
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
        self.is_initialized.store(true, Ordering::Relaxed);
    }

    /// 检查本地画板是否已初始化
    ///
    /// # 返回值
    ///
    /// 如果已初始化返回true，否则返回false
    pub fn is_initialized(&self) -> bool {
        self.is_initialized.load(Ordering::Relaxed)
    }

    /// 获取画板尺寸
    ///
    /// # 返回值
    ///
    /// 返回画板的宽度和高度
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

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

    /// 启动事件监听器，监听来自 SDK 的绘制事件并 update 本地画板状态
    pub fn start_event_listener(self: &Arc<Self>) {
        let mut receiver = event::subscribe();
        let self_clone = self.clone();
        tokio::spawn(async move {
            info!("LocalBoard: 事件监听器已启动");

            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        match event {
                            event::PaintEvent::Success { uid: _, pos, color } => {
                                // 更新像素颜色
                                self_clone.update_pixel(pos.x, pos.y, color);

                                // 3. 更新热力图，记录绘制时间戳
                                self_clone
                                    .heatmap
                                    .entry(pos)
                                    .or_default()
                                    .push(SystemTime::now());

                                trace!("LocalBoard: 通过事件更新像素 at ({}, {})", pos.x, pos.y);
                            }
                            event::PaintEvent::PixelUpdate { pos, color } => {
                                // 更新像素颜色
                                self_clone.update_pixel(pos.x, pos.y, color);

                                // 3. 更新热力图，记录绘制时间戳
                                self_clone
                                    .heatmap
                                    .entry(pos)
                                    .or_default()
                                    .push(SystemTime::now());

                                trace!(
                                    "LocalBoard: 通过像素更新事件更新 at ({}, {})",
                                    pos.x,
                                    pos.y
                                );
                            }
                            event::PaintEvent::Failure { uid: _, pos } => {
                                debug!("LocalBoard: 绘制失败事件 at ({}, {})", pos.x, pos.y);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("LocalBoard: 事件接收器滞后，跳过 {} 条消息", skipped);
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        warn!("LocalBoard: 事件广播通道已关闭，监听器退出");
                        break;
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

        board.update_pixel(10, 20, test_color);
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
        local.update_pixel(0, 0, Rgb::new(0, 0, 0));

        local.update_from_board(&server_board);

        // After updating, local should match server at those positions
        assert_eq!(local.get_pixel(0, 0), Some(Rgb::new(255, 255, 255)));
        assert_eq!(local.get_pixel(1, 0), Some(Rgb::new(255, 0, 0)));
        assert!(local.is_initialized());
    }
}
