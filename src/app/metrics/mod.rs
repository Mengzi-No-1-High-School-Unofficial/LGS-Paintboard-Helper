use color_eyre::Report;
use log::warn;
use once_cell::sync::OnceCell;
use rustc_hash::FxHashMap;
use winter_paintboard_sdk::Pos;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Metrics {
    /// 全局数据
    pub global: GlobalMetricsData,
    /// 每个 Token 的数据
    pub tokens: FxHashMap<u32, Arc<RwLock<TokenMetricsData>>>,
}

pub struct GlobalMetricsData {
    /// 总绘制像素数
    pub total_painted_pixels: u64,
    /// 成功绘制像素数
    pub successful_painted_pixels: u64,
    /// 失败绘制像素数
    pub failed_painted_pixels: u64,
}
pub struct TokenMetricsData {
    /// 关联的 UID
    pub uid: u32,
    /// 绘制像素数
    pub painted_pixels: u64,
    /// 成功绘制像素数
    pub successful_painted_pixels: u64,
    /// 失败绘制像素数
    pub failed_painted_pixels: u64,
    /// 近 10 分钟绘制过的像素
    pub recent_painted_pixels: Vec<(std::time::Instant, Pos)>,
}

static METRICS: OnceCell<Arc<RwLock<Metrics>>> = OnceCell::new();

impl Metrics {
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
    pub fn get_instance() -> Result<Arc<RwLock<Metrics>>, Report> {
        let metrics = METRICS.get_or_init(|| {
            Arc::new(RwLock::new(Metrics::new()))
        }).clone();

        Ok(metrics)
    }

    /// 获取某个 Token 的指标数据引用 [`TokenMetricsData`]
    /// 
    /// 如果不存在，则创建一个新的空条目
    pub fn get_token_metrics(
        &mut self,
        uid: u32,
    ) -> Arc<RwLock<TokenMetricsData>> {
        self.tokens.entry(uid).or_insert_with(|| {
            Arc::new(RwLock::new(TokenMetricsData::new(uid)))
        }).clone()
    }

    pub async fn record_global_paint_success(&mut self, uid: u32, pos: Pos) {
        self.global.total_painted_pixels += 1;
        self.global.successful_painted_pixels += 1;

        let token_metrics = self.get_token_metrics(uid);
        let mut token_metrics = token_metrics.write().await;
        token_metrics.record_paint_success(pos);
    }
    
    pub async fn record_global_paint_failure(&mut self, uid: u32, pos: Pos) {
        self.global.total_painted_pixels += 1;
        self.global.failed_painted_pixels += 1;

        let token_metrics = self.get_token_metrics(uid);
        let mut token_metrics = token_metrics.write().await;
        token_metrics.record_paint_failure(pos);
    }
}

impl TokenMetricsData {
    pub fn new(uid: u32) -> Self {
        TokenMetricsData {
            uid,
            painted_pixels: 0,
            successful_painted_pixels: 0,
            failed_painted_pixels: 0,
            recent_painted_pixels: Vec::new(),
        }
    }

    pub(self) fn record_paint_success(&mut self, pos: Pos) {
        self.painted_pixels += 1;
        self.successful_painted_pixels += 1;
        self.recent_painted_pixels.push((std::time::Instant::now(), pos));
        self.clear_old_entries();
    }

    pub(self) fn record_paint_failure(&mut self, pos: Pos) {
        self.painted_pixels += 1;
        self.failed_painted_pixels += 1;
        self.recent_painted_pixels.push((std::time::Instant::now(), pos));
        self.clear_old_entries();
    }

    pub(self) fn clear_old_entries(&mut self) {
        let now = std::time::Instant::now();
        let duration = std::time::Duration::from_secs(10 * 60);

        // 保留最近 duration 时间内的记录
        self.recent_painted_pixels.retain(|(timestamp, _)| {
            now.duration_since(*timestamp) <= duration
        });
    }

    /// 获取近期绘制速率（每分钟像素数）
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