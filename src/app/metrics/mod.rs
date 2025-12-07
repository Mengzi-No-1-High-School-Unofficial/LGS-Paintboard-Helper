//! 指标收集模块
//!
//! 该模块负责收集和管理绘制过程中的各种指标，包括全局指标和
//! 每个Token的独立指标，用于监控绘制性能和成功率。

use color_eyre::Report;
use dashmap::DashMap;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use winter_paintboard_sdk::Pos;

/// 指标管理器结构
///
/// 管理全局指标和每个Token的独立指标数据
pub struct Metrics {
    /// 全局指标数据
    pub global: GlobalMetricsData,
    /// 每个 Token 的指标数据映射
    pub tokens: DashMap<u32, Arc<TokenMetricsData>>,
}

/// 全局指标数据结构
///
/// 包含整个绘制过程的全局统计信息
pub struct GlobalMetricsData {
    /// 总绘制像素数
    pub total_painted_pixels: AtomicU64,
    /// 成功绘制像素数
    pub successful_painted_pixels: AtomicU64,
    /// 失败绘制像素数
    pub failed_painted_pixels: AtomicU64,
}

/// Token指标数据结构
///
/// 包含单个Token的绘制统计信息和近期绘制记录
pub struct TokenMetricsData {
    /// 关联的 UID
    pub uid: u32,
    /// 绘制像素数
    pub painted_pixels: AtomicU64,
    /// 成功绘制像素数
    pub successful_painted_pixels: AtomicU64,
    /// 失败绘制像素数
    pub failed_painted_pixels: AtomicU64,
    /// 近 10 分钟绘制过的像素记录
    pub recent_painted_pixels: Mutex<VecDeque<(std::time::Instant, Pos)>>,
}

static METRICS: OnceCell<Arc<Metrics>> = OnceCell::new();

impl Metrics {
    /// 创建新的指标实例
    ///
    /// # 返回值
    ///
    /// 返回初始化的指标实例
    pub fn new() -> Self {
        Metrics {
            global: GlobalMetricsData {
                total_painted_pixels: AtomicU64::new(0),
                successful_painted_pixels: AtomicU64::new(0),
                failed_painted_pixels: AtomicU64::new(0),
            },
            tokens: DashMap::new(),
        }
    }

    /// 获取全局指标存储的引用
    ///
    /// 使用单例模式获取全局指标实例，如果不存在则创建新的实例
    ///
    /// # 返回值
    ///
    /// * `Ok(Arc<Metrics>)` - 全局指标实例的Arc引用
    /// * `Err` - 获取过程中发生错误
    pub fn get_instance() -> Result<Arc<Metrics>, Report> {
        let metrics = METRICS.get_or_init(|| Arc::new(Metrics::new())).clone();

        Ok(metrics)
    }

    /// 获取某个 Token 的指标数据引用 [`TokenMetricsData`]
    ///
    /// 如果不存在，则创建一个新的空条目
    ///
    /// # 参数
    ///
    /// * `uid` - 用户ID
    ///
    /// # 返回值
    ///
    /// 返回指定Token指标数据的Arc引用
    pub fn get_token_metrics(&self, uid: u32) -> Arc<TokenMetricsData> {
        self.tokens
            .entry(uid)
            .or_insert_with(|| Arc::new(TokenMetricsData::new(uid)))
            .clone()
    }

    /// 记录全局绘制成功事件
    ///
    /// 更新全局和指定Token的成功绘制统计
    ///
    /// # 参数
    ///
    /// * `uid` - 用户ID
    /// * `pos` - 绘制位置
    pub fn record_global_paint_success(&self, uid: u32, pos: Pos) {
        self.global
            .total_painted_pixels
            .fetch_add(1, Ordering::Relaxed);
        self.global
            .successful_painted_pixels
            .fetch_add(1, Ordering::Relaxed);

        let token_metrics = self.get_token_metrics(uid);
        token_metrics.record_paint_success(pos);
    }

    /// 记录全局绘制失败事件
    ///
    /// 更新全局和指定Token的失败绘制统计
    ///
    /// # 参数
    ///
    /// * `uid` - 用户ID
    /// * `pos` - 绘制位置
    pub fn record_global_paint_failure(&self, uid: u32, pos: Pos) {
        self.global
            .total_painted_pixels
            .fetch_add(1, Ordering::Relaxed);
        self.global
            .failed_painted_pixels
            .fetch_add(1, Ordering::Relaxed);

        let token_metrics = self.get_token_metrics(uid);
        token_metrics.record_paint_failure(pos);
    }
}

impl TokenMetricsData {
    /// 创建新的Token指标数据实例
    ///
    /// # 参数
    ///
    /// * `uid` - 用户ID
    ///
    /// # 返回值
    ///
    /// 返回初始化的Token指标数据实例
    pub fn new(uid: u32) -> Self {
        TokenMetricsData {
            uid,
            painted_pixels: AtomicU64::new(0),
            successful_painted_pixels: AtomicU64::new(0),
            failed_painted_pixels: AtomicU64::new(0),
            recent_painted_pixels: Mutex::new(VecDeque::new()),
        }
    }

    /// 记录绘制成功事件
    ///
    /// 更新成功绘制统计并记录绘制位置和时间
    ///
    /// # 参数
    ///
    /// * `pos` - 绘制位置
    pub(self) fn record_paint_success(&self, pos: Pos) {
        self.painted_pixels.fetch_add(1, Ordering::Relaxed);
        self.successful_painted_pixels
            .fetch_add(1, Ordering::Relaxed);

        let mut history = self.recent_painted_pixels.lock();
        history.push_back((std::time::Instant::now(), pos));
        self.clear_old_entries(&mut history);
    }

    /// 记录绘制失败事件
    ///
    /// 更新失败绘制统计并记录绘制位置和时间
    ///
    /// # 参数
    ///
    /// * `pos` - 绘制位置
    pub(self) fn record_paint_failure(&self, pos: Pos) {
        self.painted_pixels.fetch_add(1, Ordering::Relaxed);
        self.failed_painted_pixels.fetch_add(1, Ordering::Relaxed);

        let mut history = self.recent_painted_pixels.lock();
        history.push_back((std::time::Instant::now(), pos));
        self.clear_old_entries(&mut history);
    }

    /// 清理旧的记录
    ///
    /// 移除超过10分钟的绘制记录，保持近期记录的准确性
    fn clear_old_entries(&self, history: &mut VecDeque<(std::time::Instant, Pos)>) {
        let now = std::time::Instant::now();
        let duration = std::time::Duration::from_secs(10 * 60);

        while let Some((timestamp, _)) = history.front() {
            if now.duration_since(*timestamp) > duration {
                history.pop_front();
            } else {
                break;
            }
        }
    }

    /// 获取近期绘制速率（每分钟像素数）
    ///
    /// 基于最近的绘制记录计算每分钟的绘制速率
    ///
    /// # 返回值
    ///
    /// 每分钟绘制的像素数
    pub fn get_recent_paint_rate(&self) -> f64 {
        let history = self.recent_painted_pixels.lock();
        let first = history.front();
        let last = history.back();

        if let (None, None) = (first, last) {
            return 0.0;
        } else {
            let first = first.unwrap();
            let last = last.unwrap();

            let duration = last.0.duration_since(first.0).as_secs_f64();
            if duration == 0.0 {
                return history.len() as f64;
            }

            (history.len() as f64) / (duration / 60.0)
        }
    }
}
