//! 像素队列模块
//!
//! 该模块实现了线程安全的优先级像素队列，用于管理待绘制的像素，
//! 支持按优先级排序和并发访问。

use std::collections::BinaryHeap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::app::multi_token::config::PriorityPixel;

/// 线程安全的优先级像素队列
///
/// 使用二叉堆实现的优先级队列，支持并发访问和像素优先级管理
#[derive(Clone)]
pub struct PixelQueue {
    /// 二叉堆存储的像素队列
    queue: Arc<Mutex<BinaryHeap<PriorityPixel>>>,
}

impl PixelQueue {
    /// 创建新的像素队列
    ///
    /// # 返回值
    ///
    /// 返回初始化的像素队列实例
    pub fn new() -> Self {
        Self {
            queue: Arc::new(Mutex::new(BinaryHeap::new())),
        }
    }

    /// 清空队列并添加新像素（用于新的比对）
    ///
    /// 清空现有队列并将新的像素列表添加到队列中
    ///
    /// # 参数
    ///
    /// * `pixels` - 要添加的像素列表
    pub async fn reset_and_push(&self, pixels: Vec<PriorityPixel>) {
        let mut queue = self.queue.lock().await;
        queue.clear();
        for pixel in pixels {
            queue.push(pixel);
        }
    }

    /// 尝试弹出一个像素（非阻塞）
    ///
    /// 从队列中取出优先级最高的像素（如果存在）
    ///
    /// # 返回值
    ///
    /// 返回优先级最高的像素（如果存在）
    pub async fn try_pop(&self) -> Option<PriorityPixel> {
        let mut queue = self.queue.lock().await;
        queue.pop()
    }

    /// 获取队列长度
    ///
    /// # 返回值
    ///
    /// 返回队列中像素的数量
    pub async fn len(&self) -> usize {
        let queue = self.queue.lock().await;
        queue.len()
    }

    /// 检查队列是否为空
    ///
    /// # 返回值
    ///
    /// 如果队列为空返回true，否则返回false
    pub async fn is_empty(&self) -> bool {
        let queue = self.queue.lock().await;
        queue.is_empty()
    }

    /// 获取队列中的所有像素（用于调试）
    ///
    /// # 返回值
    ///
    /// 返回队列中所有像素的排序列表
    pub async fn get_all_pixels(&self) -> Vec<PriorityPixel> {
        let queue = self.queue.lock().await;
        queue.clone().into_sorted_vec().into_iter().rev().collect()
    }

    /// 增量合并更新（而非清空）
    ///
    /// 将新像素合并到现有队列中，新像素会覆盖相同位置的旧像素
    ///
    /// # 参数
    ///
    /// * `new_pixels` - 要合并的新像素列表
    pub async fn merge_updates(&self, new_pixels: Vec<PriorityPixel>) {
        let mut queue = self.queue.lock().await;

        // 创建现有像素的 HashMap（避免在重建期间的竞态条件）
        let mut pixel_map: std::collections::HashMap<
            winter_paintboard_sdk::models::Pos,
            PriorityPixel,
        > = queue.iter().cloned().map(|p| (p.pos, p)).collect();

        // 合并新像素（新优先级覆盖旧优先级）
        for pixel in new_pixels {
            pixel_map.insert(pixel.pos, pixel);
        }

        // 原子性地重建优先队列
        *queue = pixel_map.into_values().collect();
    }
}
