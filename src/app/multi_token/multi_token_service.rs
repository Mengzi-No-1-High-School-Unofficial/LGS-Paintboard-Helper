use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval;
use log::{debug, error, info, warn};

use winter_paintboard_sdk::PoolClient;
use winter_paintboard_sdk::{PaintboardClientTrait, BasicClient, config::Config};
use crate::app::board_sync::LocalBoard;
use crate::app::image_processing::ProcessedImageData;
use crate::app::incremental::pixel_comparison::calculate_color_difference;
use crate::app::multi_token::config::{TokenConfig, PriorityPixel};
use crate::app::multi_token::pixel_queue::PixelQueue;
use crate::app::multi_token::token_manager::{TokenManager, TokenInfo};
use crate::app::multi_token::token_worker::TokenWorker;
use crate::app::multi_token::paint_executor::{PaintExecutor, PaintRequestQueue};

/// 多 Token 绘制服务
pub struct MultiTokenService {
    workers: Vec<tokio::task::JoinHandle<()>>,
    executor_handle: Option<tokio::task::JoinHandle<()>>,
    comparison_handle: Option<tokio::task::JoinHandle<()>>,
    metrics_handle: Option<tokio::task::JoinHandle<()>>,
    pixel_queue: Arc<PixelQueue>,
    local_board: Arc<Mutex<LocalBoard>>,
    target_image: ProcessedImageData,
    start_x: i32,
    start_y: i32,
    stop_signal: Arc<AtomicBool>,
    comparison_interval: Duration,
    token_manager: Arc<TokenManager>,
    shared_client: Arc<PoolClient>,
}

impl MultiTokenService {
    /// 创建新的多 Token 服务
    pub async fn new(
        token_config: TokenConfig,
        ws_url: Option<String>,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        comparison_interval: Duration,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 解析所有 Token（将 access_key 转换为 token）
        let mut tokens = Vec::new();
        for entry in token_config.tokens {
            let token = if let Some(token) = entry.token {
                token
            } else if let Some(access_key) = entry.access_key {
                // 需要在这里获取 token
                // 注意：这里需要一个 HTTP 客户端来获取 token
                // 我们暂时使用占位符，实际实现需要调用 get_token
                return Err("暂时不支持从 access_key 获取 token，请使用预获取的 token".into());
            } else {
                return Err(format!("Token entry for UID {} 缺少 token 或 access_key", entry.uid).into());
            };
            
            tokens.push(TokenInfo::new(entry.uid, token));
        }

        // 创建共享客户端
        let mut config = Config::default();
        if let Some(url) = ws_url {
            config.ws_url = url;
        }
        let shared_client = PoolClient::new(config).await?;

        // 创建 TokenManager
        let token_manager = Arc::new(TokenManager::new(tokens, token_config.cd_time_ms));

        Ok(Self {
            workers: Vec::new(),
            executor_handle: None,
            comparison_handle: None,
            metrics_handle: None,
            pixel_queue: Arc::new(PixelQueue::new()),
            local_board,
            target_image,
            start_x,
            start_y,
            stop_signal: Arc::new(AtomicBool::new(false)),
            comparison_interval,
            token_manager,
            shared_client: Arc::new(shared_client),
        })
    }

    /// 启动服务
    pub async fn start(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("启动多 Token 绘制服务，Token 数量: {}", self.token_manager.len());

        // 创建 PaintRequestQueue
        let paint_request_queue = Arc::new(PaintRequestQueue::new());

        // 启动 PaintExecutor
        let executor = PaintExecutor::new(
            self.shared_client.clone(),
            paint_request_queue.clone(),
            self.local_board.clone(),
            self.pixel_queue.clone(),
        );

        let stop_signal = self.stop_signal.clone();
        self.executor_handle = Some(tokio::spawn(async move {
            executor.run(stop_signal).await;
        }));

        // 为每个 Token 创建 Worker
        let ws_url = self.shared_client.get_config().ws_url.clone();

        for i in 0..self.token_manager.len() {
            let token_manager = self.token_manager.clone();
            let pixel_queue = self.pixel_queue.clone();
            let request_queue = paint_request_queue.clone();
            let stop_signal = self.stop_signal.clone();

            let worker = TokenWorker::new(i, token_manager, pixel_queue, request_queue);
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
            ).await;
        });

        self.comparison_handle = Some(comparison_handle);

        info!("多 Token 服务已启动，Worker 数量: {}", self.workers.len());
        Ok(())
    }

    /// 停止服务
    pub async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error>> {
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
    async fn run_comparison_loop(
        pixel_queue: Arc<PixelQueue>,
        local_board: Arc<Mutex<LocalBoard>>,
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

            // 获取本地绘版数据
            let local_pixels = {
                let board = local_board.lock().await;
                if !board.is_initialized() {
                    warn!("本地绘版未初始化，跳过比对");
                    continue;
                }
                board.get_pixels().clone()
            };

            // 计算差异像素
            let mut differences = Vec::new();
            for (pos, target_color) in &target_image.full_scale_operations {
                let x = pos.x as i32;
                let y = pos.y as i32;

                let relative_x = x - start_x;
                let relative_y = y - start_y;

                if relative_x >= 0 && relative_y >= 0
                    && relative_x < target_image.img_width as i32
                    && relative_y < target_image.img_height as i32
                {
                    let color_diff = if let Some(current_pixel) = local_pixels.get(pos) {
                        if current_pixel.color != *target_color {
                            calculate_color_difference(&current_pixel.color, target_color)
                        } else {
                            continue; // 颜色一致，跳过
                        }
                    } else {
                        255.0 // 缺少像素，最高优先级
                    };

                    differences.push(PriorityPixel {
                        pos: *pos,
                        color: *target_color,
                        priority: color_diff,
                    });
                }
            }

            // 更新队列（增量合并）
            if !differences.is_empty() {
                info!("检测到 {} 个像素差异，更新队列", differences.len());
                pixel_queue.merge_updates(differences).await;
            } else {
                info!("未检测到像素差异");
            }
        }
    }
}