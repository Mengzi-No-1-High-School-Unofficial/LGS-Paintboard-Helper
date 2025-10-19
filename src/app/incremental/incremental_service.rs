use crate::app::board_sync::{BoardSyncManager, LocalBoard};
use crate::app::image_processing::ProcessedImageData;
use crate::app::incremental::pixel_comparison::calculate_color_difference;
use crate::app::incremental::restoration_manager::restore_sorted_pixels;
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use winter_paintboard_sdk::{PaintboardClientTrait, Pos, Rgb};

/// 增量修改管理器（从原 `incremental.rs` 迁移）
pub struct IncrementalManager {
    pub client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
    pub local_board: Arc<Mutex<LocalBoard>>,
    pub target_image_data: ProcessedImageData,
    pub start_x: i32,
    pub start_y: i32,
    pub monitor_interval: Duration,
    pub restore_delay: Duration,
    pub max_batch_size: usize,
}

impl IncrementalManager {
    /// 使用已共享的 Arc<Mutex> 客户端创建增量管理器
    pub fn new(
        client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image_data: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        monitor_interval: Duration,
        restore_delay: Duration,
        max_batch_size: usize,
    ) -> Self {
        Self {
            client,
            local_board,
            target_image_data,
            start_x,
            start_y,
            monitor_interval,
            restore_delay,
            max_batch_size,
        }
    }

    /// 执行初始绘制
    pub async fn initial_draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("开始执行增量模式初始绘制...");

        // 使用现有的批量绘制函数进行初始绘制
        crate::app::drawing::draw_image_to_paintboard_with_client(
            &self.client,
            &self.target_image_data,
            &crate::app::drawing::ProgressiveMode::None, // 使用普通模式
            self.max_batch_size,
            self.restore_delay.as_millis() as u64, // 使用恢复延迟作为绘制延迟
        )
        .await?;

        info!("初始绘制完成");
        Ok(())
    }

    /// 启动监控循环（内部使用）
    pub async fn start_monitoring(
        client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image_data: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        monitor_interval: Duration,
        restore_delay: Duration,
        max_batch_size: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!(
            "开始监控绘版变化，监控间隔: {:?}, 批处理大小: {}",
            monitor_interval, max_batch_size
        );

        tokio::spawn(async move {
            let mut interval_timer = interval(monitor_interval);

            loop {
                interval_timer.tick().await;

                if let Err(e) = Self::compare_and_restore(
                    &client,
                    &local_board,
                    &target_image_data,
                    start_x,
                    start_y,
                    restore_delay,
                    max_batch_size,
                )
                .await
                {
                    error!("监控和恢复过程中发生错误: {:?}", e);
                }
            }
        });

        Ok(())
    }

    /// 比较本地绘版与目标图片，并恢复被修改的像素
    async fn compare_and_restore(
        client: &Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
        local_board: &Arc<Mutex<LocalBoard>>,
        target_image_data: &ProcessedImageData,
        start_x: i32,
        start_y: i32,
        restore_delay: Duration,
        max_batch_size: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("开始比对绘版数据与目标图片...");

        // 获取本地绘版数据的副本
        let local_pixels = {
            let board = local_board.lock().await;
            if !board.is_initialized() {
                warn!("本地绘版数据未初始化，跳过本次比对");
                return Ok(());
            }
            board.get_pixels().clone()
        };

        let mut pixel_differences = Vec::new();

        // 遍历目标图片的所有像素坐标
        for (pos, target_color) in &target_image_data.full_scale_operations {
            let x = pos.x as i32;
            let y = pos.y as i32;

            // 计算相对于目标绘制位置的坐标
            let relative_x = x - start_x;
            let relative_y = y - start_y;

            // 检查目标图片中的坐标是否有效
            if relative_x >= 0
                && relative_y >= 0
                && relative_x < target_image_data.img_width as i32
                && relative_y < target_image_data.img_height as i32
            {
                // 检查本地绘版上的对应像素是否与目标颜色一致
                if let Some(current_pixel_status) = local_pixels.get(pos) {
                    // 计算颜色差异，用于优先级排序
                    let color_diff =
                        calculate_color_difference(&current_pixel_status.color, target_color);

                    if current_pixel_status.color != *target_color {
                        debug!(
                            "检测到像素变化: ({}, {}) 期望颜色 ({}, {}, {}) 实际颜色 ({}, {}, {})",
                            pos.x,
                            pos.y,
                            target_color.r,
                            target_color.g,
                            target_color.b,
                            current_pixel_status.color.r,
                            current_pixel_status.color.g,
                            current_pixel_status.color.b
                        );

                        pixel_differences.push((*pos, *target_color, color_diff));
                    }
                } else {
                    // 本地绘版上没有这个像素，需要恢复
                    debug!(
                        "本地绘版缺少像素: ({}, {}), 需要恢复为 ({}, {}, {})",
                        pos.x, pos.y, target_color.r, target_color.g, target_color.b
                    );

                    // 使用最大可能的颜色差异值
                    pixel_differences.push((*pos, *target_color, 255.0));
                }
            }
        }

        // 按颜色差异降序排序，优先修复差异大的像素
        pixel_differences.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

        // 提取排序后的像素进行修复
        let pixels_to_restore: Vec<(Pos, Rgb)> = pixel_differences
            .into_iter()
            .map(|(pos, color, _)| (pos, color))
            .collect();

        // 恢复被修改的像素
        if !pixels_to_restore.is_empty() {
            info!(
                "检测到 {} 个像素被修改，开始按优先级恢复...",
                pixels_to_restore.len()
            );

            // 使用已拆分出的 restoration_manager 执行批量恢复
            restore_sorted_pixels(client, pixels_to_restore, restore_delay, max_batch_size).await?;
        } else {
            info!("未检测到目标区域内像素被修改");
        }

        Ok(())
    }
}

/// 便捷函数：根据参数启动增量修改服务
pub async fn start_incremental_if_enabled(
    cli_token: Option<String>,
    cli_uid: u32,
    _cli_ws_url: Option<String>, // 参数未使用，因为客户端已在外部创建并配置
    sync_manager: &BoardSyncManager,
    enable_incremental: bool,
    target_image_data: &ProcessedImageData,
    start_x: i32,
    start_y: i32,
    monitor_interval: u64,
    restore_delay: u64,
    max_batch_size: usize,
    client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>, // 现在接收已共享的客户端
) -> Result<(), Box<dyn std::error::Error>> {
    if enable_incremental {
        info!("启用增量修改模式");

        // 设置认证信息（通过加锁设置）
        {
            let mut cl = client.lock().await;
            cl.as_mut()
                .set_auth(cli_uid, cli_token.unwrap_or_default().to_string());
        }

        // 创建增量管理器（直接传入共享客户端）
        let mut incremental_manager = IncrementalManager::new(
            client.clone(),
            sync_manager.local_board(),
            target_image_data.clone(),
            start_x,
            start_y,
            Duration::from_millis(monitor_interval),
            Duration::from_millis(restore_delay),
            max_batch_size,
        );

        // 执行初始绘制
        incremental_manager.initial_draw().await?;

        // 启动监控服务
        IncrementalManager::start_monitoring(
            incremental_manager.client.clone(),
            sync_manager.local_board(),
            target_image_data.clone(),
            start_x,
            start_y,
            Duration::from_millis(monitor_interval),
            Duration::from_millis(restore_delay),
            max_batch_size,
        )
        .await?;

        info!("增量修改模式已启动并运行");
    }

    Ok(())
}
