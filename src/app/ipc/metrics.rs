//! 监控指标模块
//!
//! 定义监控数据结构和采集逻辑

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Worker 监控指标
///
/// 包含 Worker 的所有运行时监控数据
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkerMetrics {
    /// Worker 唯一标识
    pub worker_id: String,
    /// 时间戳（Unix 时间，秒）
    pub timestamp: u64,

    // 基础信息
    /// 进程 ID
    pub process_id: u32,
    /// 启动时间（Unix 时间，秒）
    pub start_time: u64,
    /// 运行时长（秒）
    pub uptime_secs: u64,

    // Token 状态
    /// Token 总数
    pub total_tokens: usize,
    /// 可用 Token 数
    pub available_tokens: usize,
    /// 冷却中 Token 数
    pub cooldown_tokens: usize,

    // 绘制性能
    /// 累计绘制像素数
    pub total_pixels_painted: u64,
    /// 最近 1 分钟绘制像素数
    pub pixels_painted_last_minute: u64,
    /// 绘制成功率（%）
    pub paint_success_rate: f64,
    /// 平均绘制延迟（毫秒）
    pub avg_paint_delay_ms: f64,

    // 队列状态
    /// 当前像素队列长度
    pub queue_size: usize,
    /// 队列峰值长度
    pub queue_peak_size: usize,

    // 画板同步
    /// 与目标图像的差异像素数
    pub board_sync_diff_count: usize,
}

/// 数据库存储用的 Worker 监控指标
///
/// 使用 SurrealDB 原生的 Datetime 类型
#[derive(Serialize, Deserialize, Debug, Clone)]
struct WorkerMetricsDb {
    worker_id: String,
    timestamp: surrealdb::sql::Datetime,
    process_id: u32,
    start_time: surrealdb::sql::Datetime,
    uptime_secs: u64,
    total_tokens: usize,
    available_tokens: usize,
    cooldown_tokens: usize,
    total_pixels_painted: u64,
    pixels_painted_last_minute: u64,
    paint_success_rate: f64,
    avg_paint_delay_ms: f64,
    queue_size: usize,
    queue_peak_size: usize,
    board_sync_diff_count: usize,
}

impl From<&WorkerMetrics> for WorkerMetricsDb {
    fn from(metrics: &WorkerMetrics) -> Self {
        use chrono::DateTime;

        let timestamp_dt = surrealdb::sql::Datetime::from(
            DateTime::from_timestamp(metrics.timestamp as i64, 0).unwrap_or_default(),
        );
        let start_time_dt = surrealdb::sql::Datetime::from(
            DateTime::from_timestamp(metrics.start_time as i64, 0).unwrap_or_default(),
        );

        Self {
            worker_id: metrics.worker_id.clone(),
            timestamp: timestamp_dt,
            process_id: metrics.process_id,
            start_time: start_time_dt,
            uptime_secs: metrics.uptime_secs,
            total_tokens: metrics.total_tokens,
            available_tokens: metrics.available_tokens,
            cooldown_tokens: metrics.cooldown_tokens,
            total_pixels_painted: metrics.total_pixels_painted,
            pixels_painted_last_minute: metrics.pixels_painted_last_minute,
            paint_success_rate: metrics.paint_success_rate,
            avg_paint_delay_ms: metrics.avg_paint_delay_ms,
            queue_size: metrics.queue_size,
            queue_peak_size: metrics.queue_peak_size,
            board_sync_diff_count: metrics.board_sync_diff_count,
        }
    }
}

/// 全局监控摘要
///
/// Master 汇总所有 Worker 的监控数据
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetricsSummary {
    /// 时间戳（Unix 时间，秒）
    pub timestamp: u64,
    /// Worker 总数
    pub total_workers: usize,
    /// 活跃 Worker 数
    pub active_workers: usize,
    /// 所有 Worker 的 Token 总数
    pub total_tokens: usize,
    /// 可用 Token 总数
    pub available_tokens: usize,
    /// 冷却中 Token 总数
    pub cooldown_tokens: usize,
    /// 全局绘制速率（像素/秒）
    pub global_paint_rate: f64,
    /// 累计绘制总数
    pub total_pixels_painted: u64,
    /// 队列总长度
    pub total_queue_size: usize,
    /// 平均差异数
    pub avg_diff_count: usize,
}

/// Worker 端指标采集器
///
/// 负责在 Worker 端收集各类监控指标
pub struct MetricsCollector {
    /// Worker ID
    worker_id: String,
    /// 进程 ID
    process_id: u32,
    /// 启动时间
    start_time: SystemTime,

    // 绘制统计（原子计数器）
    /// 累计绘制像素数
    total_pixels_painted: Arc<AtomicU64>,
    /// 最近 1 分钟绘制像素数
    pixels_painted_last_minute: Arc<AtomicU64>,
    /// 绘制成功次数
    paint_success_count: Arc<AtomicU64>,
    /// 绘制总次数
    paint_total_count: Arc<AtomicU64>,
    /// 绘制延迟总和（毫秒）
    paint_delay_sum_ms: Arc<AtomicU64>,

    // 队列统计
    /// 队列峰值长度
    queue_peak_size: Arc<AtomicUsize>,
}

impl MetricsCollector {
    /// 创建新的指标采集器
    ///
    /// # 参数
    ///
    /// * `worker_id` - Worker 唯一标识
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            process_id: std::process::id(),
            start_time: SystemTime::now(),
            total_pixels_painted: Arc::new(AtomicU64::new(0)),
            pixels_painted_last_minute: Arc::new(AtomicU64::new(0)),
            paint_success_count: Arc::new(AtomicU64::new(0)),
            paint_total_count: Arc::new(AtomicU64::new(0)),
            paint_delay_sum_ms: Arc::new(AtomicU64::new(0)),
            queue_peak_size: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// 记录成功绘制一个像素
    ///
    /// # 参数
    ///
    /// * `delay_ms` - 绘制延迟（毫秒）
    pub fn record_paint_success(&self, delay_ms: u64) {
        self.total_pixels_painted.fetch_add(1, Ordering::Relaxed);
        self.pixels_painted_last_minute
            .fetch_add(1, Ordering::Relaxed);
        self.paint_success_count.fetch_add(1, Ordering::Relaxed);
        self.paint_total_count.fetch_add(1, Ordering::Relaxed);
        self.paint_delay_sum_ms
            .fetch_add(delay_ms, Ordering::Relaxed);
    }

    /// 记录绘制失败
    pub fn record_paint_failure(&self) {
        self.paint_total_count.fetch_add(1, Ordering::Relaxed);
    }

    /// 更新队列峰值
    ///
    /// # 参数
    ///
    /// * `current_size` - 当前队列大小
    pub fn update_queue_peak(&self, current_size: usize) {
        self.queue_peak_size
            .fetch_max(current_size, Ordering::Relaxed);
    }

    /// 重置每分钟计数器（应该每分钟调用一次）
    pub fn reset_minute_counters(&self) {
        self.pixels_painted_last_minute.store(0, Ordering::Relaxed);
    }

    /// 收集当前指标
    ///
    /// # 参数
    ///
    /// * `token_manager` - Token 管理器（用于获取 Token 状态）
    /// * `pixel_queue` - 像素队列（用于获取队列状态）
    /// * `diff_count` - 当前差异像素数
    ///
    /// # 返回值
    ///
    /// 返回当前的监控指标
    pub fn collect_metrics(
        &self,
        total_tokens: usize,
        available_tokens: usize,
        cooldown_tokens: usize,
        queue_size: usize,
        diff_count: usize,
    ) -> WorkerMetrics {
        let now = SystemTime::now();
        let timestamp = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let uptime_secs = now
            .duration_since(self.start_time)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let start_time = self
            .start_time
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let total_pixels_painted = self.total_pixels_painted.load(Ordering::Relaxed);
        let pixels_painted_last_minute = self.pixels_painted_last_minute.load(Ordering::Relaxed);

        let paint_success = self.paint_success_count.load(Ordering::Relaxed);
        let paint_total = self.paint_total_count.load(Ordering::Relaxed);
        let paint_success_rate = if paint_total > 0 {
            (paint_success as f64 / paint_total as f64) * 100.0
        } else {
            0.0
        };

        let delay_sum = self.paint_delay_sum_ms.load(Ordering::Relaxed);
        let avg_paint_delay_ms = if paint_success > 0 {
            delay_sum as f64 / paint_success as f64
        } else {
            0.0
        };

        let queue_peak_size = self.queue_peak_size.load(Ordering::Relaxed);

        WorkerMetrics {
            worker_id: self.worker_id.clone(),
            timestamp,
            process_id: self.process_id,
            start_time,
            uptime_secs,
            total_tokens,
            available_tokens,
            cooldown_tokens,
            total_pixels_painted,
            pixels_painted_last_minute,
            paint_success_rate,
            avg_paint_delay_ms,
            queue_size,
            queue_peak_size,
            board_sync_diff_count: diff_count,
        }
    }

    /// 获取绘制成功计数器（供外部模块使用）
    pub fn paint_success_counter(&self) -> Arc<AtomicU64> {
        self.total_pixels_painted.clone()
    }

    /// 获取队列峰值计数器（供外部模块使用）
    pub fn queue_peak_counter(&self) -> Arc<AtomicUsize> {
        self.queue_peak_size.clone()
    }
}

/// Master 端监控数据聚合器
///
/// 负责接收、存储和聚合所有 Worker 的监控数据
pub struct MetricsAggregator {
    /// 数据库句柄
    db: Arc<surrealdb::Surreal<surrealdb::engine::local::Db>>,
    /// Worker 最新指标缓存（worker_id -> metrics）
    latest_metrics: Arc<parking_lot::RwLock<std::collections::HashMap<String, WorkerMetrics>>>,
    /// Master 启动时间
    master_start_time: SystemTime,
}

impl MetricsAggregator {
    /// 创建新的指标聚合器
    ///
    /// # 参数
    ///
    /// * `db_path` - SurrealDB 数据库路径
    ///
    /// # 返回值
    ///
    /// * `Ok(MetricsAggregator)` - 成功创建聚合器
    /// * `Err` - 创建过程中发生错误
    pub async fn new(db_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        use surrealdb::engine::local::RocksDb;
        use surrealdb::Surreal;

        // 创建或打开 RocksDB 数据库
        let db: Surreal<surrealdb::engine::local::Db> = Surreal::new::<RocksDb>(db_path).await?;

        // 使用命名空间和数据库
        db.use_ns("paintboard").use_db("metrics").await?;

        // 初始化数据库表结构
        let _result = db
            .query(
                r#"
            DEFINE TABLE IF NOT EXISTS worker_metrics SCHEMAFULL;
            DEFINE FIELD IF NOT EXISTS worker_id ON worker_metrics TYPE string;
            DEFINE FIELD IF NOT EXISTS timestamp ON worker_metrics TYPE datetime;
            DEFINE FIELD IF NOT EXISTS process_id ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS start_time ON worker_metrics TYPE datetime;
            DEFINE FIELD IF NOT EXISTS uptime_secs ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS total_tokens ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS available_tokens ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS cooldown_tokens ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS total_pixels_painted ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS pixels_painted_last_minute ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS paint_success_rate ON worker_metrics TYPE float;
            DEFINE FIELD IF NOT EXISTS avg_paint_delay_ms ON worker_metrics TYPE float;
            DEFINE FIELD IF NOT EXISTS queue_size ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS queue_peak_size ON worker_metrics TYPE int;
            DEFINE FIELD IF NOT EXISTS board_sync_diff_count ON worker_metrics TYPE int;
            DEFINE INDEX IF NOT EXISTS idx_worker_id ON worker_metrics FIELDS worker_id;
            DEFINE INDEX IF NOT EXISTS idx_timestamp ON worker_metrics FIELDS timestamp;
            "#,
            )
            .await?;

        Ok(Self {
            db: Arc::new(db),
            latest_metrics: Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new())),
            master_start_time: SystemTime::now(),
        })
    }

    /// 记录 Worker 监控指标
    ///
    /// 将指标存入数据库并更新内存缓存
    ///
    /// # 参数
    ///
    /// * `metrics` - Worker 监控指标
    pub async fn record_metrics(
        &self,
        metrics: WorkerMetrics,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // 更新内存缓存
        {
            let mut cache = self.latest_metrics.write();
            cache.insert(metrics.worker_id.clone(), metrics.clone());
        }

        // 转换为数据库格式并存储
        let db_metrics = WorkerMetricsDb::from(&metrics);
        let _: Option<WorkerMetricsDb> =
            self.db.create("worker_metrics").content(db_metrics).await?;

        Ok(())
    }

    /// 获取所有 Worker 的最新指标
    pub fn get_all_latest_metrics(&self) -> Vec<WorkerMetrics> {
        let cache = self.latest_metrics.read();
        cache.values().cloned().collect()
    }

    /// 获取指定 Worker 的最新指标
    ///
    /// # 参数
    ///
    /// * `worker_id` - Worker ID
    pub fn get_worker_metrics(&self, worker_id: &str) -> Option<WorkerMetrics> {
        let cache = self.latest_metrics.read();
        cache.get(worker_id).cloned()
    }

    /// 获取全局监控摘要
    pub fn get_summary(&self) -> MetricsSummary {
        let cache = self.latest_metrics.read();
        let workers: Vec<&WorkerMetrics> = cache.values().collect();

        let now = SystemTime::now();
        let timestamp = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();

        let total_workers = workers.len();
        let active_workers = workers.len(); // 所有在缓存中的都是活跃的

        let total_tokens: usize = workers.iter().map(|w| w.total_tokens).sum();
        let available_tokens: usize = workers.iter().map(|w| w.available_tokens).sum();
        let cooldown_tokens: usize = workers.iter().map(|w| w.cooldown_tokens).sum();

        let total_pixels_painted: u64 = workers.iter().map(|w| w.total_pixels_painted).sum();
        let pixels_painted_last_minute: u64 =
            workers.iter().map(|w| w.pixels_painted_last_minute).sum();

        // 全局绘制速率 = 所有 Worker 最近 1 分钟绘制数 / 60 秒
        let global_paint_rate = pixels_painted_last_minute as f64 / 60.0;

        let total_queue_size: usize = workers.iter().map(|w| w.queue_size).sum();

        let avg_diff_count = if !workers.is_empty() {
            workers
                .iter()
                .map(|w| w.board_sync_diff_count)
                .sum::<usize>()
                / workers.len()
        } else {
            0
        };

        MetricsSummary {
            timestamp,
            total_workers,
            active_workers,
            total_tokens,
            available_tokens,
            cooldown_tokens,
            global_paint_rate,
            total_pixels_painted,
            total_queue_size,
            avg_diff_count,
        }
    }

    /// 获取指定 Worker 的历史数据
    ///
    /// # 参数
    ///
    /// * `worker_id` - Worker ID
    /// * `duration_secs` - 查询时长（秒）
    pub async fn get_worker_history(
        &self,
        worker_id: &str,
        duration_secs: u64,
    ) -> Result<Vec<WorkerMetrics>, Box<dyn std::error::Error>> {
        let now = SystemTime::now();
        let since_timestamp = now
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
            .saturating_sub(duration_secs);

        let query = format!(
            "SELECT * FROM worker_metrics WHERE worker_id = '{}' AND timestamp >= {} ORDER BY timestamp DESC",
            worker_id, since_timestamp
        );

        let mut result: surrealdb::Response = self.db.query(query).await?;
        let metrics: Vec<WorkerMetrics> = result.take(0)?;

        Ok(metrics)
    }

    /// 清理过期数据
    ///
    /// 删除超过指定天数的历史数据
    ///
    /// # 参数
    ///
    /// * `days` - 保留天数
    pub async fn cleanup_old_data(&self, days: u64) -> Result<(), Box<dyn std::error::Error>> {
        let cutoff_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
            .saturating_sub(days * 86400); // 86400 秒 = 1 天

        let query = format!(
            "DELETE FROM worker_metrics WHERE timestamp < {}",
            cutoff_timestamp
        );

        self.db.query(query).await?;

        Ok(())
    }

    /// 移除 Worker
    ///
    /// 当 Worker 断开连接时从缓存中移除
    ///
    /// # 参数
    ///
    /// * `worker_id` - Worker ID
    pub fn remove_worker(&self, worker_id: &str) {
        let mut cache = self.latest_metrics.write();
        cache.remove(worker_id);
    }

    /// 获取 Master 运行时长（秒）
    pub fn get_master_uptime(&self) -> u64 {
        SystemTime::now()
            .duration_since(self.master_start_time)
            .unwrap_or(Duration::ZERO)
            .as_secs()
    }
}
