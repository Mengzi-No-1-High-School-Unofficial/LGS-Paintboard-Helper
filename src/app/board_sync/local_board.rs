//! 本地画板同步模块
//!
//! 该模块实现了本地画板数据的存储和同步功能，包括像素数据、热力图、
//! 数据完整性验证等功能。

use std::sync::atomic::{AtomicBool, Ordering};
use std::{sync::Arc, time::Duration};

use dashmap::DashMap;
use std::time::SystemTime;
use tokio::sync::RwLock;
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
    /// 感兴趣的像素点列表（优化遍历）
    interest_pixels: Arc<RwLock<Option<Vec<Pos>>>>,
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
            interest_pixels: Arc::new(RwLock::new(None)),
        }
    }

    /// 设置感兴趣的像素点列表
    ///
    /// 设置后，全量更新操作（如 update_from_board）将仅遍历这些像素点
    pub async fn set_interest_pixels(&self, pixels: Vec<Pos>) {
        let mut interest = self.interest_pixels.write().await;
        *interest = Some(pixels);
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

    /// 从Board对象更新本地数据 - 这是权威数据
    ///
    /// 使用服务器的Board数据全量更新本地画板数据，服务器数据是绝对权威
    ///
    /// # 参数
    ///
    /// * `board` - 服务器画板数据
    /// 从Board对象更新本地数据 - 这是权威数据
    ///
    /// 使用服务器的Board数据全量更新本地画板数据，服务器数据是绝对权威
    ///
    /// # 参数
    ///
    /// * `board` - 服务器画板数据
    ///
    /// # 返回值
    ///
    /// 返回发生变更的像素列表 (x, y, color)，用于广播通知
    pub async fn update_from_board(&self, board: &Board) -> Vec<(u32, u32, Rgb)> {
        // 全量更新时，服务器数据是绝对权威
        // 但不直接清空，而是对比并更新差异

        let interest_pixels = self.interest_pixels.read().await;

        if let Some(ref interest) = *interest_pixels {
            // 如果设定了感兴趣区域，仅遍历该区域
            let mut changed_pixels = Vec::new();

            for pos in interest {
                let x = pos.x;
                let y = pos.y;

                if x < board.width && y < board.height {
                    if let Ok(server_pixel) = board.get_pixel(x, y) {
                        let server_color = Rgb::new(server_pixel.r, server_pixel.g, server_pixel.b);

                        // 对比本地颜色
                        let is_diff = self
                            .pixels
                            .get(pos)
                            .map(|entry| entry.value().color != server_color)
                            .unwrap_or(true);

                        if is_diff {
                            self.pixels.insert(
                                *pos,
                                PixelStatus {
                                    color: server_color,
                                },
                            );
                            changed_pixels.push((x as u32, y as u32, server_color));
                        }
                    }
                }
            }

            info!(
                "全量同步完成（感兴趣区域模式），更新：{} 个像素",
                changed_pixels.len()
            );
            self.is_initialized.store(true, Ordering::Relaxed);
            return changed_pixels;
        }

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

        // 准备返回的变更列表
        let mut changed_pixels = Vec::with_capacity(updates_len + additions_len);

        // 执行实际的更新操作并标记为最近变更
        for (pos, new_status) in updates {
            changed_pixels.push((pos.x as u32, pos.y as u32, new_status.color));
            self.pixels.insert(pos, new_status);
        }

        for (pos, new_status) in additions {
            changed_pixels.push((pos.x as u32, pos.y as u32, new_status.color));
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

        changed_pixels
    }

    /// 检查本地画板是否已初始化
    ///
    /// # 返回值
    ///
    /// 如果已初始化返回true，否则返回false
    pub fn is_initialized(&self) -> bool {
        self.is_initialized.load(Ordering::Relaxed)
    }

    /// 计算带时间衰减的热度分数
    ///
    /// 返回值范围 [0, +∞),值越大表示该位置越"热"(被频繁绘制)
    /// 使用线性时间衰减:越近的绘制权重越高
    ///
    /// # 参数
    ///
    /// * `pos` - 像素位置
    ///
    /// # 返回值
    ///
    /// 热度分数,0 表示该位置在时间窗口内没有被绘制过
    pub fn calculate_heat_score(&self, pos: &Pos) -> f64 {
        const DECAY_WINDOW_SECS: f64 = 600.0; // 10分钟衰减窗口

        self.heatmap.get(pos).map_or(0.0, |entry| {
            let now = SystemTime::now();
            entry
                .value()
                .iter()
                .filter_map(|&timestamp| {
                    let elapsed = now.duration_since(timestamp).ok()?;
                    let elapsed_secs = elapsed.as_secs_f64();

                    if elapsed_secs < DECAY_WINDOW_SECS {
                        // 线性衰减: 越近的绘制权重越高 (1.0 -> 0.0)
                        Some(1.0 - (elapsed_secs / DECAY_WINDOW_SECS))
                    } else {
                        None
                    }
                })
                .sum()
        })
    }

    /// 将画板数据序列化为字节数组
    ///
    /// # 返回值
    ///
    /// RGB 格式的字节数组,长度为 width * height * 3
    pub async fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; (self.width as usize) * (self.height as usize) * 3];

        for entry in self.pixels.iter() {
            let pos = entry.key();
            let color = &entry.value().color;

            let idx = ((pos.y as usize) * (self.width as usize) + (pos.x as usize)) * 3;
            if idx + 2 < bytes.len() {
                bytes[idx] = color.r;
                bytes[idx + 1] = color.g;
                bytes[idx + 2] = color.b;
            }
        }

        bytes
    }

    /// 从字节数组更新画板数据
    ///
    /// # 参数
    ///
    /// * `bytes` - RGB 格式的字节数组
    pub async fn update_from_bytes(&self, bytes: &[u8]) {
        let expected_len = (self.width as usize) * (self.height as usize) * 3;
        if bytes.len() != expected_len {
            warn!(
                "Bytes length mismatch: expected {}, got {}",
                expected_len,
                bytes.len()
            );
            return;
        }

        let interest_pixels = self.interest_pixels.read().await;

        if let Some(ref interest) = *interest_pixels {
            for pos in interest {
                let idx = ((pos.y as usize) * (self.width as usize) + (pos.x as usize)) * 3;
                if idx + 2 < bytes.len() {
                    let r = bytes[idx];
                    let g = bytes[idx + 1];
                    let b = bytes[idx + 2];

                    self.pixels.insert(
                        *pos,
                        PixelStatus {
                            color: Rgb::new(r, g, b),
                        },
                    );
                }
            }
            self.is_initialized.store(true, Ordering::SeqCst);
            info!("画板从字节数据更新完成（感兴趣区域模式）");
            return;
        }

        for y in 0..self.height {
            for x in 0..self.width {
                let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 3;
                let r = bytes[idx];
                let g = bytes[idx + 1];
                let b = bytes[idx + 2];

                if let Ok(pos) = Pos::new(x, y) {
                    self.pixels.insert(
                        pos,
                        PixelStatus {
                            color: Rgb::new(r, g, b),
                        },
                    );
                }
            }
        }

        self.is_initialized.store(true, Ordering::SeqCst);
        info!("画板从字节数据更新完成");
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

    #[tokio::test]
    async fn test_local_board_creation() {
        let _board = LocalBoard::new(1000, 600);
        assert!(!_board.is_initialized());
    }

    #[tokio::test]
    async fn test_local_board_pixel_operations() {
        let board = LocalBoard::new(100, 100);
        let test_color = Rgb::new(255, 0, 0);

        board.update_pixel(10, 20, test_color);
        assert_eq!(board.get_pixel(10, 20), Some(test_color));
        assert_eq!(board.get_pixel(11, 20), None); // No pixel at (11, 20)
    }

    #[tokio::test]
    async fn test_update_from_board_applies_server_data() {
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

        local.update_from_board(&server_board).await;

        // After updating, local should match server at those positions
        assert_eq!(local.get_pixel(0, 0), Some(Rgb::new(255, 255, 255)));
        assert_eq!(local.get_pixel(1, 0), Some(Rgb::new(255, 0, 0)));
        assert!(local.is_initialized());
    }

    #[tokio::test]
    async fn test_update_from_board_with_interest_pixels() {
        let local = LocalBoard::new(1000, 600);
        let pos1 = Pos::new(10, 10).unwrap();
        // 仅对 (10, 10) 感兴趣
        local.set_interest_pixels(vec![pos1]).await;

        let mut server_board = Board::new();
        server_board
            .set_pixel(10, 10, Pixel::new(255, 255, 255))
            .unwrap();
        server_board
            .set_pixel(20, 20, Pixel::new(255, 0, 0))
            .unwrap();

        let changes = local.update_from_board(&server_board).await;

        // 只有 (10, 10) 应该被更新
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, 10);
        assert_eq!(local.get_pixel(10, 10), Some(Rgb::new(255, 255, 255)));

        // (20, 20) 即使在服务器上有数据，也应该被忽略
        assert_eq!(local.get_pixel(20, 20), None);
    }
}
