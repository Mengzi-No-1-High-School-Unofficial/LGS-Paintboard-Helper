//! 多Token服务模块
//!
//! 该模块实现了多Token并发绘制的核心服务，包括任务调度、像素队列管理、
//! Token管理、绘制执行等功能，使用网格图算法优化绘制优先级。

use color_eyre::eyre::Ok;
use color_eyre::Report;
use log::{debug, error, info, trace, warn};
use rustc_hash::FxHashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep, Instant};
use winter_paintboard_sdk::Pos;
use parking_lot::RwLock;

use crate::app::board_sync::LocalBoard;
use crate::app::image_processing::ProcessedImageData;
use crate::app::multi_token::cli::get_penalty_scale;
use crate::app::multi_token::config::{PriorityPixel, TokenConfig, TokenEntry};
use crate::app::multi_token::paint_batcher::PaintBatcher;
use crate::app::multi_token::pixel_queue::PixelQueue;
use crate::app::multi_token::token_manager::{TokenInfo, TokenManager};
use crate::app::multi_token::token_worker::TokenWorker;
use crate::app::utils::get_token_with_access_key;
use winter_paintboard_sdk::{basic_client::AsyncClient, config::Config, PaintboardClientTrait};

/// 多 Token 绘制服务
///
/// 核心服务类，管理多个Token的并发绘制任务，包括像素队列、工作线程、
/// 绘制执行器和比对循环等组件
pub struct MultiTokenService {
    /// Token工作线程句柄列表
    workers: Vec<tokio::task::JoinHandle<()>>,
    /// 批量绘制处理器线程句柄
    batcher_handle: Option<tokio::task::JoinHandle<()>>,
    /// 比对循环线程句柄
    comparison_handle: Option<tokio::task::JoinHandle<()>>,
    /// 指标打印线程句柄
    metrics_handle: Option<tokio::task::JoinHandle<()>>,
    /// 像素队列，用于存储待绘制的像素
    pixel_queue: Arc<PixelQueue>,
    /// 本地画板的共享引用
    local_board: Arc<LocalBoard>,
    /// 目标图像数据
    target_image: ProcessedImageData,
    /// 绘制起始X坐标
    start_x: i32,
    /// 绘制起始Y坐标
    start_y: i32,
    /// 停止信号，用于控制服务停止
    stop_signal: Arc<AtomicBool>,
    /// 比对间隔时间
    comparison_interval: Duration,
    /// Token管理器
    token_manager: Arc<TokenManager>,
    /// 共享客户端
    shared_client: Arc<AsyncClient>,
    /// 批处理大小
    batch_size: usize,
}

impl MultiTokenService {
    /// 创建新的多 Token 服务
    ///
    /// 使用默认的网格图算法阈值创建服务实例
    ///
    /// # 参数
    ///
    /// * `token_config` - Token配置
    /// * `ws_url` - WebSocket URL（可选）
    /// * `local_board` - 本地画板引用
    /// * `target_image` - 目标图像数据
    /// * `start_x` - 起始X坐标
    /// * `start_y` - 起始Y坐标
    /// * `comparison_interval` - 比对间隔时间
    ///
    /// # 返回值
    ///
    /// * `Ok(MultiTokenService)` - 成功创建的服务实例
    /// * `Err` - 创建过程中发生错误
    pub async fn new(
        token_config: TokenConfig,
        ws_url: Option<String>,
        local_board: Arc<LocalBoard>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        comparison_interval: Duration,
        batch_size: usize,
    ) -> Result<Self, Report> {
        Self::with_canny_thresholds(
            token_config,
            ws_url,
            local_board,
            target_image,
            start_x,
            start_y,
            comparison_interval,
            20.0, // 默认低阈值
            40.0, // 默认高阈值
            batch_size,
        )
        .await
    }

    /// 创建新的多 Token 服务，支持配置网格图算法阈值
    ///
    /// # 参数
    ///
    /// * `token_config` - Token配置
    /// * `ws_url` - WebSocket URL（可选）
    /// * `local_board` - 本地画板引用
    /// * `target_image` - 目标图像数据
    /// * `start_x` - 起始X坐标
    /// * `start_y` - 起始Y坐标
    /// * `comparison_interval` - 比对间隔时间
    /// * `canny_low_thresh` - 网格图算法低阈值
    /// * `canny_high_thresh` - 网格图算法高阈值
    ///
    /// # 返回值
    ///
    /// * `Ok(MultiTokenService)` - 成功创建的服务实例
    /// * `Err` - 创建过程中发生错误
    pub async fn with_canny_thresholds(
        token_config: TokenConfig,
        ws_url: Option<String>,
        local_board: Arc<LocalBoard>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        comparison_interval: Duration,
        canny_low_thresh: f32,
        canny_high_thresh: f32,
        batch_size: usize,
    ) -> Result<Self, Report> {
        // 解析所有 Token（将 access_key 转换为 token）
        let tokens = Self::fetch_tokens(&token_config).await?;

        // 创建共享客户端
        let mut config = Config::default();
        if let Some(url) = ws_url {
            config.ws_url = url;
        }
        let shared_client = winter_paintboard_sdk::get_global_client(config).await?;

        // 创建 TokenManager
        let token_manager = Arc::new(TokenManager::new(tokens, token_config.cd_time_ms));

        // 创建 Metrics 打印任务
        let token_manager_clone = token_manager.clone();
        let stop_signal_clone = Arc::new(AtomicBool::new(false));
        let metrics_handle = {
            let stop_signal = stop_signal_clone.clone();

            tokio::spawn(async move {
                Self::print_metrics_loop(token_manager_clone, stop_signal).await;
            })
        };

        Ok(Self {
            workers: Vec::new(),
            batcher_handle: None,
            comparison_handle: None,
            metrics_handle: Some(metrics_handle),
            pixel_queue: Arc::new(PixelQueue::new()),
            local_board,
            target_image,
            start_x,
            start_y,
            stop_signal: Arc::new(AtomicBool::new(false)),
            comparison_interval,
            token_manager,
            shared_client,
            batch_size,
        })
    }

    /// 启动服务
    ///
    /// 启动所有组件，包括绘制执行器、Token工作线程和比对循环
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功启动服务
    /// * `Err` - 启动过程中发生错误
    pub async fn start(&mut self) -> Result<(), Report> {
        info!(
            "启动多 Token 绘制服务，Token 数量: {}",
            self.token_manager.len()
        );

        // 创建用于 PaintOperation 的 MPSC 通道
        let (batch_sender, batch_receiver) = tokio::sync::mpsc::unbounded_channel();

        // 启动 PaintBatcher
        let mut batcher = PaintBatcher::new(
            batch_receiver,
            self.shared_client.clone(),
            self.batch_size, // 批处理大小限制
            Duration::from_millis(200), // 时间限制
        );

        self.batcher_handle = Some(tokio::spawn(async move {
            batcher.run().await;
        }));

        // 为每个 Token 创建 Worker
        for i in 0..self.token_manager.len() {
            let token_manager = self.token_manager.clone();
            let pixel_queue = self.pixel_queue.clone();
            let batch_sender = batch_sender.clone();
            let stop_signal = self.stop_signal.clone();

            let worker = TokenWorker::new(i, token_manager, pixel_queue, batch_sender);
            let handle = tokio::spawn(async move {
                if let Err(e) = worker.run(stop_signal).await {
                    log::error!("Worker {} 出错: {:?}", i, e);
                }
            });

            self.workers.push(handle);
        }

        // 启动比对循环
        let pixel_queue = self.pixel_queue.clone();
        let local_board = self.local_board.clone();
        let target_image = self.target_image.clone();
        let start_x = self.start_x;
        let start_y = self.start_y;
        let interval_duration = self.comparison_interval;
        let stop_signal = self.stop_signal.clone();

        let comparison_handle = tokio::spawn(async move {
            Self::run_comparison_loop(
                pixel_queue,
                local_board,
                target_image,
                start_x,
                start_y,
                interval_duration,
                stop_signal,
            )
            .await;
        });

        self.comparison_handle = Some(comparison_handle);

        // 启动 LocalBoard 的事件监听器和热力图清理任务
        self.local_board.start_event_listener();
        self.local_board.start_heatmap_cleanup_task();

        info!("多 Token 服务已启动，Worker 数量: {}", self.workers.len());
        Ok(())
    }

    /// 停止服务
    ///
    /// 停止所有运行的组件并清理资源
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功停止服务
    /// * `Err` - 停止过程中发生错误
    pub async fn stop(&mut self) -> Result<(), Report> {
        info!("正在停止多 Token 服务...");

        // 设置停止信号
        self.stop_signal.store(true, Ordering::Release);

        // 等待所有 Worker 完成
        for handle in self.workers.drain(..) {
            if let Err(e) = handle.await {
                error!("Worker 任务等待错误: {:?}", e);
            }
        }

        // 等待比对循环完成
        if let Some(handle) = self.comparison_handle.take() {
            if let Err(e) = handle.await {
                error!("比对循环任务等待错误: {:?}", e);
            }
        }

        info!("多 Token 服务已停止");
        Ok(())
    }

    /// 运行比对循环
    ///
    /// 定期比较本地画板与目标图像，将差异像素添加到绘制队列
    ///
    /// # 参数
    ///
    /// * `pixel_queue` - 像素队列
    /// * `local_board` - 本地画板引用
    /// * `target_image` - 目标图像数据
    /// * `start_x` - 起始X坐标
    /// * `start_y` - 起始Y坐标
    /// * `interval_duration` - 比对间隔时间
    /// * `stop_signal` - 停止信号
    /// * `local_paint_history` - 本地绘制历史
    /// * `local_paint_total` - 本地绘制总数
    async fn run_comparison_loop(
        pixel_queue: Arc<PixelQueue>,
        local_board: Arc<LocalBoard>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        interval_duration: Duration,
        stop_signal: Arc<AtomicBool>,
    ) {
        let mut interval_timer = interval(interval_duration);

        loop {
            if stop_signal.load(Ordering::Acquire) {
                break;
            }

            interval_timer.tick().await;

            debug!("开始比对绘版与目标图片...");
            if !local_board.is_initialized() {
                warn!("本地绘版未初始化，跳过比对");
                continue;
            }

            // 1. 计算总绘制频率 (∑R)
            let total_recent_paints = local_board
                .get_heatmap()
                .iter()
                .map(|entry| {
                    let now = std::time::SystemTime::now();
                    entry
                        .value()
                        .iter()
                        .filter(|&&timestamp| {
                            now.duration_since(timestamp).unwrap_or_default()
                                < std::time::Duration::from_secs(10 * 60)
                        })
                        .count()
                })
                .sum::<usize>() as f64;

            const MIN_TOTAL_PAINTS: f64 = 100.0; // 最小总绘画数，避免早期惩罚过高
            let penalty_divisor = total_recent_paints.max(MIN_TOTAL_PAINTS);
            let penalty_scale = get_penalty_scale() as f64;

            // 2. 计算每个差异像素的优先级
            let mut differences = Vec::new();
            let local_pixels = local_board.get_pixels();
            for (pos, target_color) in &target_image.full_scale_operations {
                let x = pos.x as i32;
                let y = pos.y as i32;

                let relative_x = x - start_x;
                let relative_y = y - start_y;

                if relative_x >= 0
                    && relative_y >= 0
                    && relative_x < target_image.img_width as i32
                    && relative_y < target_image.img_height as i32
                {
                    let base_priority = if let Some(current_pixel) = local_pixels.get(pos) {
                        if current_pixel.color != *target_color {
                            // 基础优先级，后续可替换为 Canny 边缘强度等
                            ((pos.x + pos.y) % 8) as f64
                        } else {
                            continue; // 颜色一致，跳过
                        }
                    } else {
                        255.0 // 缺少像素，最高优先级
                    };

                    // 3. 计算归一化惩罚值并应用
                    let recent_paints = local_board.calculate_penalty(pos); // 这现在只返回近期绘制数
                    let penalty = (recent_paints / penalty_divisor) * penalty_scale;
                    let final_priority = base_priority - penalty;

                    trace!(
                        "像素 ({}, {}): 基础优先级: {:.2}, 近期绘制: {}, 惩罚: {:.2}, 最终优先级: {:.2}",
                        pos.x, pos.y, base_priority, recent_paints, penalty, final_priority
                    );

                    differences.push(PriorityPixel {
                        pos: *pos,
                        color: *target_color,
                        priority: final_priority,
                    });
                }
            }

            // 更新队列（增量合并）
            if !differences.is_empty() {
                info!("检测到 {} 个像素差异，更新队列", differences.len());
                pixel_queue.merge_updates(differences);
            } else {
                info!("未检测到像素差异");
            }
        }
    }

    /// 从Token条目获取Token
    ///
    /// 根据Token条目中的信息获取实际的Token，可能是直接提供或通过访问密钥获取
    ///
    /// # 参数
    ///
    /// * `entry` - Token条目
    ///
    /// # 返回值
    ///
    /// * `Ok(String)` - 获取到的Token
    /// * `Err` - 获取过程中发生错误
    async fn get_token_from_entry(entry: &TokenEntry) -> Result<String, Report> {
        let token = match (entry.uid, entry.access_key.clone(), entry.token.clone()) {
            (uid, Some(access_key), None) => {
                debug!("为 UID {} 获取 Token...", uid);

                let token = get_token_with_access_key(uid, &access_key).await?;

                info!("获取到 UID {} 的 Token", uid);

                token
            }
            (uid, None, Some(token)) => {
                debug!("使用提供的 Token 为 UID {}", uid);

                token
            }
            _ => {
                return Err(Report::msg(format!(
                    "TokenEntry 必须提供 access_key 或 token，UID: {}",
                    entry.uid
                )));
            }
        };

        Ok(token)
    }

    /// 将 [`TokenConfig`] 中的所有 [`TokenEntry`] 解析为 [`TokenInfo`] 列表
    ///
    /// 从配置中获取所有Token信息，包括通过访问密钥获取的Token
    ///
    /// # Note
    ///
    /// 不应为该函数增加异步并行操作，否则会触发 429 Rate Limit 错误
    ///
    /// # 参数
    ///
    /// * `token_config` - Token配置
    ///
    /// # 返回值
    ///
    /// * `Ok(Vec<TokenInfo>)` - Token信息列表
    /// * `Err` - 获取过程中发生错误
    async fn fetch_tokens(token_config: &TokenConfig) -> Result<Vec<TokenInfo>, Report> {
        let mut tokens = Vec::new();

        for entry in token_config.tokens.clone() {
            let token = Self::get_token_from_entry(&entry).await?;
            tokens.push(TokenInfo {
                uid: entry.uid,
                token,
                last_paint_time: None,
                is_available: true,
            });

            sleep(Duration::from_millis(1000)).await; // 避免触发 429 Rate Limit
        }

        Ok(tokens)
    }

    /// 打印指标循环
    ///
    /// 定期打印绘制指标，包括总绘制数、成功/失败数等
    ///
    /// # 参数
    ///
    /// * `token_manager` - Token管理器
    /// * `stop_signal` - 停止信号
    async fn print_metrics_loop(token_manager: Arc<TokenManager>, stop_signal: Arc<AtomicBool>) {
        use crate::app::metrics::Metrics;

        let mut interval_timer = interval(Duration::from_secs(60));

        loop {
            if stop_signal.load(Ordering::Acquire) {
                break;
            }

            interval_timer.tick().await;

            let metrics = Metrics::get_instance();

            if let Err(e) = metrics {
                error!("获取全局指标存储失败: {:?}", e);
                continue;
            }

            let metrics = metrics.unwrap();

            info!("=== 全局绘制指标 ===");
            info!("总绘制像素数: {}", metrics.global.total_painted_pixels.load(Ordering::Relaxed));
            info!(
                "成功绘制像素数: {}",
                metrics.global.successful_painted_pixels.load(Ordering::Relaxed)
            );
            info!("失败绘制像素数: {}", metrics.global.failed_painted_pixels.load(Ordering::Relaxed));
            info!("===================");

            for token_info in token_manager.get_all_tokens() {
                let uid = token_info.uid;
                let token_metrics = metrics.tokens.get(&uid);

                if let Some(token_metrics) = token_metrics {
                    let token_metrics = token_metrics.value();
                    info!("--- Token UID: {} 指标 ---", uid);
                    info!("总绘制像素数: {}", token_metrics.painted_pixels.load(Ordering::Relaxed));
                    info!(
                        "成功绘制像素数: {}",
                        token_metrics.successful_painted_pixels.load(Ordering::Relaxed)
                    );
                    info!("失败绘制像素数: {}", token_metrics.failed_painted_pixels.load(Ordering::Relaxed));
                    info!(
                        "绘制速率 (像素/分钟): {:.2}",
                        token_metrics.get_recent_paint_rate()
                    );
                    info!("-------------------------");
                } else {
                    info!("Token UID: {} 无指标数据", uid);
                }
            }
        }
    }



}
