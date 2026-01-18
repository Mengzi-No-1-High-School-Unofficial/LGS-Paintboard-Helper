//! 多Token服务模块
//!
//! 该模块实现了多Token并发绘制的核心服务，包括任务调度、像素队列管理、
//! Token管理、绘制执行等功能，使用网格图算法优化绘制优先级。

use color_eyre::Report;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::app::board_sync::LocalBoard;
use crate::app::image_processing::ProcessedImageData;
use crate::app::ipc::MetricsCollector;
use crate::app::multi_token::cli::get_penalty_sensitivity;
use crate::app::multi_token::config::{PriorityPixel, TokenConfig, TokenEntry};
use crate::app::multi_token::paint_batcher::PaintBatcher;
use crate::app::multi_token::pixel_queue::PixelQueue;
use crate::app::multi_token::token_manager::{TokenInfo, TokenManager};
use crate::app::multi_token::token_worker::TokenWorker;
use crate::app::utils::get_token_with_access_key;
use winter_paintboard_sdk::basic_client::AsyncClient;

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
    /// 监控数据采集器（可选）
    metrics_collector: Option<Arc<MetricsCollector>>,
    /// 当前差异像素数（用于监控）
    current_diff_count: Arc<std::sync::atomic::AtomicUsize>,
    /// 正在绘制中的像素（已发送但未收到响应），记录发送时间
    pending_pixels: Arc<dashmap::DashMap<winter_paintboard_sdk::models::Pos, std::time::Instant>>,
}

impl MultiTokenService {
    /// 使用已有的 LocalBoard 创建服务 (用于 Worker 模式)
    ///
    /// # 参数
    ///
    /// * `token_config` - Token配置
    /// * `target_image` - 目标图像数据
    /// * `start_x` - 起始X坐标
    /// * `start_y` - 起始Y坐标
    /// * `local_board` - 已有的 LocalBoard 实例
    /// * `batch_size` - 批处理大小
    /// * `shared_client` - 共享的客户端
    pub async fn with_board(
        token_config: TokenConfig,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        local_board: Arc<LocalBoard>,
        comparison_interval: Duration,
        batch_size: usize,
        shared_client: Arc<AsyncClient>,
    ) -> Result<Self, Report> {
        let stop_signal = Arc::new(AtomicBool::new(false));

        // 解析所有 Token（将 access_key 转换为 token）
        let tokens = Self::fetch_tokens(&token_config).await?;

        // 创建 TokenManager 用于解析 Token
        let token_manager = Arc::new(TokenManager::new(tokens, token_config.cd_time_ms));

        Ok(Self {
            workers: Vec::new(),
            batcher_handle: None,
            comparison_handle: None,
            pixel_queue: Arc::new(PixelQueue::new()),
            local_board,
            target_image,
            start_x,
            start_y,
            stop_signal,
            comparison_interval,
            token_manager,
            shared_client,
            batch_size,
            metrics_collector: None,
            current_diff_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            pending_pixels: Arc::new(dashmap::DashMap::new()),
        })
    }

    /// 启用监控功能
    ///
    /// # 参数
    ///
    /// * `worker_id` - Worker 唯一标识
    pub fn with_metrics(mut self, worker_id: String) -> Self {
        self.metrics_collector = Some(Arc::new(MetricsCollector::new(worker_id)));
        self
    }

    /// 获取监控数据采集器
    pub fn metrics_collector(&self) -> Option<Arc<MetricsCollector>> {
        self.metrics_collector.clone()
    }

    /// 获取当前差异像素数
    pub fn current_diff_count(&self) -> usize {
        self.current_diff_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// 获取像素队列
    pub fn pixel_queue(&self) -> Arc<PixelQueue> {
        self.pixel_queue.clone()
    }

    /// 获取停止信号
    pub fn stop_signal(&self) -> Arc<AtomicBool> {
        self.stop_signal.clone()
    }

    /// 获取 Token 管理器
    pub fn token_manager(&self) -> Arc<TokenManager> {
        self.token_manager.clone()
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
            self.batch_size,                // 批处理大小限制
            Duration::from_millis(100),     // 时间限制
            self.metrics_collector.clone(), // 监控数据采集器
            self.pending_pixels.clone(),    // pending pixels 追踪
        );

        self.batcher_handle = Some(tokio::spawn(async move {
            batcher.run().await;
        }));

        // 为每个 Token 创建 Worker
        for i in 0..(self.token_manager.len() * 4) {
            let token_manager = self.token_manager.clone();
            let pixel_queue = self.pixel_queue.clone();
            let batch_sender = batch_sender.clone();
            let stop_signal = self.stop_signal.clone();

            let worker = TokenWorker::new(i, token_manager, pixel_queue, batch_sender);
            let handle = tokio::spawn(async move {
                if let Err(e) = worker.run(stop_signal).await {
                    error!("Worker {} 出错: {:?}", i, e);
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
        let pending_pixels = self.pending_pixels.clone();

        let comparison_handle = tokio::spawn(async move {
            Self::run_comparison_loop(
                pixel_queue,
                local_board,
                target_image,
                start_x,
                start_y,
                interval_duration,
                stop_signal,
                pending_pixels,
            )
            .await;
        });

        self.comparison_handle = Some(comparison_handle);

        // 启动 LocalBoard 的事件监听器和热力图清理任务
        self.local_board.start_event_listener();
        self.local_board.start_heatmap_cleanup_task();

        // 启动 pending pixels 清理任务
        let pending_pixels = self.pending_pixels.clone();
        tokio::spawn(async move {
            let mut receiver = winter_paintboard_sdk::event::subscribe();
            loop {
                match receiver.recv().await {
                    Ok(event) => match event {
                        winter_paintboard_sdk::event::PaintEvent::Success { pos, .. } => {
                            pending_pixels.remove(&pos);
                        }
                        winter_paintboard_sdk::event::PaintEvent::Failure { pos, .. } => {
                            pending_pixels.remove(&pos);
                        }
                        _ => {}
                    },
                    Err(_) => break,
                }
            }
        });

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
    /// * `pending_pixels` - 正在绘制中的像素追踪
    #[allow(clippy::too_many_arguments)]
    pub async fn run_comparison_loop(
        pixel_queue: Arc<PixelQueue>,
        local_board: Arc<LocalBoard>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        interval_duration: Duration,
        stop_signal: Arc<AtomicBool>,
        pending_pixels: Arc<
            dashmap::DashMap<winter_paintboard_sdk::models::Pos, std::time::Instant>,
        >,
    ) {
        let mut interval_timer = interval(interval_duration);

        loop {
            if stop_signal.load(Ordering::Acquire) {
                break;
            }

            interval_timer.tick().await;

            debug!("开始全量比对绘版与目标图片...");
            if !local_board.is_initialized() {
                warn!("本地绘版未初始化，跳过比对");
                continue;
            }

            // 清理超时的 pending 像素（超过 1 秒认为响应丢失或失败）
            let timeout = Duration::from_millis(1000);
            let now = std::time::Instant::now();
            pending_pixels.retain(|_, sent_time| now.duration_since(*sent_time) < timeout);

            // 获取惩罚敏感度系数
            let sensitivity = get_penalty_sensitivity() as f64;

            // 遍历目标图像的所有像素进行比对
            let mut differences = Vec::new();

            for (pos, target_color) in &target_image.full_scale_operations {
                // 跳过正在绘制中的像素（已发送但未收到响应）
                if pending_pixels.contains_key(pos) {
                    continue;
                }

                let x = pos.x as i32;
                let y = pos.y as i32;

                let relative_x = x - start_x;
                let relative_y = y - start_y;

                if relative_x >= 0
                    && relative_y >= 0
                    && relative_x < target_image.img_width as i32
                    && relative_y < target_image.img_height as i32
                {
                    // 扩大基础优先级范围,并添加边缘增强
                    let base_priority =
                        if let Some(current_pixel) = local_board.get_pixel(pos.x, pos.y) {
                            if current_pixel != *target_color {
                                // 1. 基础填充优先级 (0-799)
                                // 使用棋盘模式 + 随机扰动,确保均匀分散
                                let chess_priority = ((pos.x + pos.y) % 8) as f64 * 100.0;
                                let random_offset = ((pos.x * 7 + pos.y * 13) % 100) as f64;
                                let fill_priority = chess_priority + random_offset;

                                // 2. 边缘增强加成 (0-300)
                                // 从 Canny 边缘检测数据中获取边缘强度
                                let edge_strength = target_image
                                    .pixel_canny_priorities
                                    .get(pos)
                                    .copied()
                                    .unwrap_or(0.0);

                                // 归一化到 0-1 范围,然后乘以加成系数
                                let edge_bonus = (edge_strength / 255.0).min(1.0) * 300.0;

                                // 3. 组合:填充 + 边缘增强
                                // 总范围: 0-1099
                                // - 平坦区域: 0-799 (无边缘加成)
                                // - 边缘区域: 800-1099 (有边缘加成)
                                fill_priority + edge_bonus
                            } else {
                                continue; // 颜色一致,跳过
                            }
                        } else {
                            // 缺少像素,最高优先级
                            // 保持远高于正常范围,确保优先填补
                            10000.0
                        };

                    // 计算带时间衰减的热度分数
                    let heat_score = local_board.calculate_heat_score(pos);

                    // 乘法惩罚: penalty_factor ∈ (0, 1]
                    // heat_score = 0 时, penalty_factor = 1 (无惩罚)
                    // heat_score 越大, penalty_factor 越接近 0
                    let penalty_factor = 1.0 / (1.0 + heat_score * sensitivity);

                    let final_priority = base_priority * penalty_factor;

                    differences.push(PriorityPixel {
                        pos: *pos,
                        color: *target_color,
                        priority: final_priority,
                    });
                }
            }

            // 更新队列
            if !differences.is_empty() {
                info!("全量比对检测到 {} 个像素差异，更新队列", differences.len());

                // 打印绘制错误统计
                winter_paintboard_sdk::error_stats::print_stats();

                pixel_queue.merge_updates(differences);
            } else {
                info!("全量比对完成：目标图像已完成");
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

        for entry in &token_config.tokens {
            match Self::get_token_from_entry(entry).await {
                Ok(token) => {
                    tokens.push(TokenInfo {
                        uid: entry.uid,
                        token,
                        last_paint_time: None,
                        is_available: true,
                    });
                }
                Err(e) => {
                    tracing::error!("获取 Token 失败 (UID {}): {}", entry.uid, e);
                    eprintln!("获取 Token 失败 (UID {}): {:?}", entry.uid, e);
                }
            }
        }

        Ok(tokens)
    }
}
