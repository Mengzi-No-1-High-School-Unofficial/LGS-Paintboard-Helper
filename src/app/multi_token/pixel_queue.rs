use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::app::multi_token::config::PriorityPixel;

/// 线程安全的优先级像素队列
#[derive(Clone)]
pub struct PixelQueue {
    queue: Arc<Mutex<BinaryHeap<PriorityPixel>>>,
}

impl PixelQueue {
    /// 创建新的像素队列
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
        }
    }

    /// 清空队列并添加新像素（用于新的比对）
    pub async fn reset_and_push(&self, pixels: Vec<PriorityPixel>) {
        let mut queue = self.queue.lock().await;
        queue.clear();
        for pixel in pixels {
            queue.push(pixel);
        }
    }

    /// 尝试弹出一个像素（非阻塞）
    pub async fn try_pop(&self) -> Option<PriorityPixel> {
        let mut queue = self.queue.lock().await;
        queue.pop()
    }

    /// 获取队列长度
    pub async fn len(&self) -> usize {
        let queue = self.queue.lock().await;
        queue.len()
    }

    /// 检查队列是否为空
    pub async fn is_empty(&self) -> bool {
        let queue = self.queue.lock().await;
        queue.is_empty()
    }

    /// 获取队列中的所有像素（用于调试）
    pub async fn get_all_pixels(&self) -> Vec<PriorityPixel> {
        let queue = self.queue.lock().await;
        queue.clone().into_sorted_vec().into_iter().rev().collect()
    }

    /// 增量合并更新（而非清空）
    pub async fn merge_updates(&self, new_pixels: Vec<PriorityPixel>) {
        let mut queue = self.queue.lock().await;

        // 创建现有像素的 HashMap（避免在重建期间的竞态条件）
        let mut pixel_map: std::collections::HashMap<winter_paintboard_sdk::models::Pos, PriorityPixel> =
            queue.iter().cloned().map(|p| (p.pos, p)).collect();

        // 合并新像素（新优先级覆盖旧优先级）
        for pixel in new_pixels {
            pixel_map.insert(pixel.pos, pixel);
        }

        // 原子性地重建优先队列
        *queue = pixel_map.into_values().collect();
    }
}