use log::{info, error, warn, debug};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration, sleep};
use winter_paintboard_sdk::{BasicClient, PaintboardClientTrait, Pos, Rgb};
use crate::app::image_processing::ProcessedImageData;
use crate::app::drawing::{create_client, ProgressiveMode};
use crate::app::board_sync::{LocalBoard, BoardSyncManager};

/// 增量修改管理器
pub struct IncrementalManager {
    client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
    local_board: Arc<Mutex<LocalBoard>>,
    target_image_data: ProcessedImageData,
    start_x: i32,
    start_y: i32,
    monitor_interval: Duration,
    restore_delay: Duration,
    max_batch_size: usize,
}

impl IncrementalManager {
    /// 创建新的增量管理器
    pub fn new(
        client: Box<dyn PaintboardClientTrait + Send>,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image_data: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        monitor_interval: Duration,
        restore_delay: Duration,
        max_batch_size: usize,  // 添加批处理大小参数
    ) -> Self {
        Self {
            client: Arc::new(Mutex::new(client)),
            local_board,
            target_image_data,
            start_x,
            start_y,
            monitor_interval,
            restore_delay,
            max_batch_size,  // 添加批处理大小
        }
    }

    /// 执行初始绘制
    pub async fn initial_draw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        info!("开始执行增量模式初始绘制...");
        
        // 使用现有的批量绘制函数进行初始绘制
        {
            let mut client = self.client.lock().await;
            
            // 使用批量模式进行初始绘制，参数使用传入的值
            crate::app::drawing::draw_image_to_paintboard_with_client(
                client.as_mut(),
                &self.target_image_data,
                &crate::app::drawing::ProgressiveMode::None, // 使用普通模式
                self.max_batch_size, // 使用传入的批量大小
                self.restore_delay.as_millis() as u64, // 使用恢复延迟作为绘制延迟
            ).await?;
        }
        
        info!("初始绘制完成");
        Ok(())
    }

    /// 开始监控并增量修改
    pub async fn start_monitoring(
        client: Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
        local_board: Arc<Mutex<LocalBoard>>,
        target_image_data: ProcessedImageData,
        start_x: i32,
        start_y: i32,
        monitor_interval: Duration,
        restore_delay: Duration,
        max_batch_size: usize, // 添加批处理大小参数
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("开始监控绘版变化，监控间隔: {:?}, 批处理大小: {}", monitor_interval, max_batch_size);
        
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
                    max_batch_size // 传递批处理大小参数
                ).await {
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
        max_batch_size: usize, // 添加批处理大小参数
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
            if relative_x >= 0 && relative_y >= 0 && 
               relative_x < target_image_data.img_width as i32 && 
               relative_y < target_image_data.img_height as i32 {
                
                // 检查本地绘版上的对应像素是否与目标颜色一致
                if let Some(current_pixel_status) = local_pixels.get(&(pos.x, pos.y)) {
                    // 计算颜色差异，用于优先级排序
                    let color_diff = Self::calculate_color_difference(&current_pixel_status.color, target_color);
                    
                    if current_pixel_status.color != *target_color {
                        debug!("检测到像素变化: ({}, {}) 期望颜色 ({}, {}, {}) 实际颜色 ({}, {}, {})",
                               pos.x, pos.y,
                               target_color.r, target_color.g, target_color.b,
                               current_pixel_status.color.r, current_pixel_status.color.g, current_pixel_status.color.b);
                        
                        pixel_differences.push((
                            *pos, 
                            *target_color, 
                            color_diff
                        ));
                    }
                } else {
                    // 本地绘版上没有这个像素，需要恢复
                    debug!("本地绘版缺少像素: ({}, {}), 需要恢复为 ({}, {}, {})",
                           pos.x, pos.y,
                           target_color.r, target_color.g, target_color.b);
                    
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
            info!("检测到 {} 个像素被修改，开始按优先级恢复...", pixels_to_restore.len());
            
            let pixels_count = pixels_to_restore.len();
            
            // 使用异步批量发送功能恢复像素
            let mut tasks = Vec::new();
            for chunk in pixels_to_restore.chunks(max_batch_size) {
                let client_clone = client.clone();
                let chunk_vec = chunk.to_vec();  // 创建一个拥有所有权的Vec
                let delay = restore_delay; // 保存delay用于异步任务
                let chunk_len = chunk.len(); // 保存chunk长度用于异步任务
                
                let task = tokio::spawn(async move {
                    // 短暂延迟以避免所有任务同时发送
                    sleep(delay).await;
                    
                    let mut client_lock = client_clone.lock().await;
                    match client_lock.as_mut().paint_batch(chunk_vec).await {
                        Ok(()) => debug!("成功恢复像素批次，包含 {} 个像素", chunk_len),
                        Err(e) => error!("批量恢复像素失败: {:?}", e),
                    }
                });
                
                tasks.push(task);
                
                // 为了防止单次产生过多并发任务，可以添加一个小延迟
                sleep(Duration::from_millis(10)).await;
            }
            
            // 等待所有异步任务完成
            for task in tasks {
                let _ = task.await;
            }
            
            info!("完成恢复 {} 个像素", pixels_count);
        } else {
            info!("未检测到目标区域内像素被修改");
        }
        
        Ok(())
    }

    // 计算两个RGB颜色之间的差异
    fn calculate_color_difference(color1: &Rgb, color2: &Rgb) -> f64 {
        let dr = (color1.r as i32 - color2.r as i32) as f64;
        let dg = (color1.g as i32 - color2.g as i32) as f64;
        let db = (color1.b as i32 - color2.b as i32) as f64;
        
        // 使用欧几里得距离计算颜色差异
        ((dr * dr + dg * dg + db * db) / 3.0).sqrt()
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
    max_batch_size: usize, // 添加批处理大小参数
    mut client: Box<dyn PaintboardClientTrait + Send>, // 从外部传入客户端，支持连接池
) -> Result<(), Box<dyn std::error::Error>> {
    if enable_incremental {
        info!("启用增量修改模式");
        
        // 设置认证信息
        client.set_auth(cli_uid, cli_token.unwrap_or_default().to_string());
        
        // 创建增量管理器
        let mut incremental_manager = IncrementalManager::new(
            client,
            sync_manager.local_board(),
            target_image_data.clone(),
            start_x,
            start_y,
            Duration::from_millis(monitor_interval),
            Duration::from_millis(restore_delay),
            max_batch_size, // 传递批处理大小参数
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
            max_batch_size, // 传递批处理大小参数
        ).await?;
        
        info!("增量修改模式已启动并运行");
    }
    
    Ok(())
}