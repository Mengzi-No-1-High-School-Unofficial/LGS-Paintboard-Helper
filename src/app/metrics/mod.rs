//! 指标收集模块
//!
//! 该模块负责收集和管理绘制过程中的各种指标，包括全局指标和
//! 每个Token的独立指标，用于监控绘制性能和成功率。

use color_eyre::Report;
use log::warn;
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use winter_paintboard_sdk::Pos;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 指标管理器结构
///
/// 管理全局指标和每个Token的独立指标数据
pub struct Metrics {
    /// 全局指标数据
    pub global: GlobalMetricsData,
    /// 每个 Token 的指标数据映射
    pub tokens: FxHashMap<u32, Arc<RwLock<TokenMetricsData>>>,
}

/// 全局指标数据结构
///
/// 包含整个绘制过程的全局统计信息
pub struct GlobalMetricsData {
    /// 总绘制像素数
    pub total_painted_pixels: u64,
    /// 成功绘制像素数
    pub successful_painted_pixels: u64,
    /// 失败绘制像素数
    pub failed_painted_pixels: u64,
}

/// Token指标数据结构
///
/// 包含单个Token的绘制统计信息和近期绘制记录
pub struct TokenMetricsData {
    /// 关联的 UID
    pub uid: u32,
    /// 绘制像素数
    pub painted_pixels: u64,
    /// 成功绘制像素数
    pub successful_painted_pixels: u64,
    /// 失败绘制像素数
    pub failed_painted_pixels: u64,
    /// 近 10 分钟绘制过的像素记录
    pub recent_painted_pixels: Vec<(std::time::Instant, Pos)>,
}

static METRICS: OnceCell<Arc<RwLock<Metrics>>> = OnceCell::new();

impl Metrics {
    /// 创建新的指标实例
    ///
    /// # 返回值
    ///
    /// 返回初始化的指标实例
    pub fn new() -> Self {
        let metrics = Metrics {
            global: GlobalMetricsData {
                total_painted_pixels: 0,
                successful_painted_pixels: 0,
                failed_painted_pixels: 0,
            },
            tokens: FxHashMap::default(),
        };

        metrics
    }

    /// 获取全局指标存储的引用
    ///
    /// 使用单例模式获取全局指标实例，如果不存在则创建新的实例
    ///
    /// # 返回值
    ///
    /// * `Ok(Arc<RwLock<Metrics>>)` - 全局指标实例的Arc引用
    /// * `Err` - 获取过程中发生错误
    pub fn get_instance() -> Result<Arc<RwLock<Metrics>>, Report> {
        let metrics = METRICS.get_or_init(|| {
            Arc::new(RwLock::new(Metrics::new()))
        }).clone();

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
    pub fn get_token_metrics(
        &mut self,
        uid: u32,
    ) -> Arc<RwLock<TokenMetricsData>> {
        self.tokens.entry(uid).or_insert_with(|| {
            Arc::new(RwLock::new(TokenMetricsData::new(uid)))
        }).clone()
    }

    /// 记录全局绘制成功事件
    ///
    /// 更新全局和指定Token的成功绘制统计
    ///
    /// # 参数
    ///
    /// * `uid` - 用户ID
    /// * `pos` - 绘制位置
    pub async fn record_global_paint_success(&mut self, uid: u32, pos: Pos) {
        self.global.total_painted_pixels += 1;
        self.global.successful_painted_pixels += 1;

        let token_metrics = self.get_token_metrics(uid);
        let mut token_metrics = token_metrics.write().await;
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
    pub async fn record_global_paint_failure(&mut self, uid: u32, pos: Pos) {
        self.global.total_painted_pixels += 1;
        self.global.failed_painted_pixels += 1;

        let token_metrics = self.get_token_metrics(uid);
        let mut token_metrics = token_metrics.write().await;
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
            painted_pixels: 0,
            successful_painted_pixels: 0,
            failed_painted_pixels: 0,
            recent_painted_pixels: Vec::new(),
        }
    }

    /// 记录绘制成功事件
    ///
    /// 更新成功绘制统计并记录绘制位置和时间
    ///
    /// # 参数
    ///
    /// * `pos` - 绘制位置
    pub(self) fn record_paint_success(&mut self, pos: Pos) {
        self.painted_pixels += 1;
        self.successful_painted_pixels += 1;
        self.recent_painted_pixels.push((std::time::Instant::now(), pos));
        self.clear_old_entries();
    }

    /// 记录绘制失败事件
    ///
    /// 更新失败绘制统计并记录绘制位置和时间
    ///
    /// # 参数
    ///
    /// * `pos` - 绘制位置
    pub(self) fn record_paint_failure(&mut self, pos: Pos) {
        self.painted_pixels += 1;
        self.failed_painted_pixels += 1;
        self.recent_painted_pixels.push((std::time::Instant::now(), pos));
        self.clear_old_entries();
    }

    /// 清理旧的记录
    ///
    /// 移除超过10分钟的绘制记录，保持近期记录的准确性
    pub(self) fn clear_old_entries(&mut self) {
        let now = std::time::Instant::now();
        let duration = std::time::Duration::from_secs(10 * 60);

        // 保留最近 duration 时间内的记录
        self.recent_painted_pixels.retain(|(timestamp, _)| {
            now.duration_since(*timestamp) <= duration
        });
    }

    /// 获取近期绘制速率（每分钟像素数）
    ///
    /// 基于最近的绘制记录计算每分钟的绘制速率
    ///
    /// # 返回值
    ///
    /// 每分钟绘制的像素数
    pub fn get_recent_paint_rate(&self) -> f64 {
        let first = self.recent_painted_pixels.first();
        let last = self.recent_painted_pixels.last();

        if let (None, None) = (first, last) {
            warn!("空绘制队列，无法计算绘制速率");
            return 0.0;
        }
        else {
            let first = first.unwrap();
            let last = last.unwrap();

            let duration = last.0.duration_since(first.0).as_secs_f64();
            if duration == 0.0 {
                return self.recent_painted_pixels.len() as f64;
            }

            (self.recent_painted_pixels.len() as f64) / (duration / 60.0)
        }
    }
}