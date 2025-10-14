use log::{info, error, warn, debug};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration, sleep};
use winter_paintboard_sdk::{PaintboardClient, Pos, Rgb};
use crate::app::image_processing::ProcessedImageData;
use crate::app::drawing::{create_client, ProgressiveMode};
use crate::app::board_sync::{LocalBoard, BoardSyncManager};

/// 增量修改管理器
pub struct IncrementalManager {
    client: Arc<Mutex<PaintboardClient>>,
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
        client: PaintboardClient,
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
                &mut client,
                &self.target_image_data,
                &crate::app::drawing::ProgressiveMode::None, // 使用普通模式
                self.max_batch_size, // 使用传入的批量大小
                self.restore_delay.as_millis() as u64, // 使用恢复延迟作为绘制延迟
                true, // 启用批量模式
            ).await?;
        }
        
        info!("初始绘制完成");
        Ok(())
    }

    /// 开始监控并增量修改
    pub async fn start_monitoring(
        client: Arc<Mutex<PaintboardClient>>,
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
        client: &Arc<Mutex<PaintboardClient>>,
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
        
        let mut pixels_to_restore = Vec::new();
        
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
                if let Some(current_color) = local_pixels.get(&(pos.x, pos.y)) {
                    if current_color != target_color {
                        debug!("检测到像素变化: ({}, {}) 从 ({}, {}, {}) 变为 ({}, {}, {})",
                               pos.x, pos.y,
                               target_color.r, target_color.g, target_color.b,
                               current_color.r, current_color.g, current_color.b);
                        pixels_to_restore.push((*pos, *target_color));
                    }
                } else {
                    // 本地绘版上没有这个像素，需要恢复
                    debug!("本地绘版缺少像素: ({}, {}), 需要恢复为 ({}, {}, {})",
                           pos.x, pos.y,
                           target_color.r, target_color.g, target_color.b);
                    pixels_to_restore.push((*pos, *target_color));
                }
            }
        }
        
        // 恢复被修改的像素
        if !pixels_to_restore.is_empty() {
            info!("检测到 {} 个像素被修改，开始恢复...", pixels_to_restore.len());
            
            let mut client_lock = client.lock().await;
            let pixels_count = pixels_to_restore.len();
            
            // 使用批量发送功能恢复像素
            for chunk in pixels_to_restore.chunks(max_batch_size) {
                match client_lock.paint_batch(chunk.to_vec()).await {
                    Ok(()) => debug!("成功恢复像素批次，包含 {} 个像素", chunk.len()),
                    Err(e) => error!("批量恢复像素失败: {:?}", e),
                }
                
                // 在批次之间添加延迟以避免速率限制
                sleep(restore_delay).await;
            }
            
            info!("完成恢复 {} 个像素", pixels_count);
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
    _cli_ws_url: Option<String>, // 使用下划线前缀表示未使用
    sync_manager: &BoardSyncManager,
    enable_incremental: bool,
    target_image_data: &ProcessedImageData,
    start_x: i32,
    start_y: i32,
    monitor_interval: u64,
    restore_delay: u64,
    max_batch_size: usize, // 添加批处理大小参数
) -> Result<(), Box<dyn std::error::Error>> {
    if enable_incremental {
        info!("启用增量修改模式");
        
        // 初始化绘版客户端
        let config = winter_paintboard_sdk::config::Config::default();
        let mut client = create_client(config).await?;
        
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