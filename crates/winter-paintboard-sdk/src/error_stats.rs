//! 绘制错误统计模块
//!
//! 提供全局的绘制错误统计功能，用于追踪各种错误类型的出现次数。

use crate::models::PaintStatus;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicU64, Ordering};

/// 全局错误统计器
pub struct ErrorStats {
    /// 错误类型计数器
    error_counts: DashMap<String, AtomicU64>,
    /// 成功计数
    success_count: AtomicU64,
    /// 总计数
    total_count: AtomicU64,
}

impl ErrorStats {
    fn new() -> Self {
        Self {
            error_counts: DashMap::new(),
            success_count: AtomicU64::new(0),
            total_count: AtomicU64::new(0),
        }
    }

    /// 记录一个绘制结果
    pub fn record(&self, status: &PaintStatus) {
        self.total_count.fetch_add(1, Ordering::Relaxed);

        match status {
            PaintStatus::Success => {
                self.success_count.fetch_add(1, Ordering::Relaxed);
            }
            other => {
                let key = format!("{:?}", other);
                self.error_counts
                    .entry(key)
                    .or_insert_with(|| AtomicU64::new(0))
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// 获取错误统计摘要
    pub fn get_summary(&self) -> String {
        let total = self.total_count.load(Ordering::Relaxed);
        let success = self.success_count.load(Ordering::Relaxed);

        if total == 0 {
            return "无统计数据".to_string();
        }

        let mut lines = vec![
            format!("绘制结果统计 (总计: {})", total),
            format!(
                "  成功: {} ({:.2}%)",
                success,
                (success as f64 / total as f64) * 100.0
            ),
        ];

        // 收集错误统计
        let mut error_list: Vec<(String, u64)> = self
            .error_counts
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().load(Ordering::Relaxed)))
            .collect();

        // 按计数降序排序
        error_list.sort_by(|a, b| b.1.cmp(&a.1));

        for (error_type, count) in error_list {
            lines.push(format!(
                "  {}: {} ({:.2}%)",
                error_type,
                count,
                (count as f64 / total as f64) * 100.0
            ));
        }

        lines.join("\n")
    }

    /// 重置所有统计
    pub fn reset(&self) {
        self.total_count.store(0, Ordering::Relaxed);
        self.success_count.store(0, Ordering::Relaxed);
        self.error_counts.clear();
    }
}

/// 全局错误统计实例
static GLOBAL_STATS: Lazy<ErrorStats> = Lazy::new(ErrorStats::new);

/// 记录一个绘制结果统计
pub fn record_paint_result(status: &PaintStatus) {
    GLOBAL_STATS.record(status);
}

/// 获取错误统计摘要
pub fn get_stats_summary() -> String {
    GLOBAL_STATS.get_summary()
}

/// 打印错误统计
pub fn print_stats() {
    tracing::info!("\n{}", get_stats_summary());
}

/// 重置统计
pub fn reset_stats() {
    GLOBAL_STATS.reset();
}
