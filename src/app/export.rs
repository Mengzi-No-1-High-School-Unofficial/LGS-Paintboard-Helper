use log::{info, error, warn};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use std::path::PathBuf;
use std::fs;
use image::{ImageBuffer, RgbImage, Rgb};

use winter_paintboard_sdk::models::Board;

use crate::app::board_sync::{LocalBoard, BoardSyncManager};

/// 图片导出管理器
pub struct ExportManager {
    local_board: Arc<Mutex<LocalBoard>>,
    export_dir: PathBuf,
    export_interval: Duration,
}

impl ExportManager {
    /// 创建新的导出管理器
    pub fn new(
        local_board: Arc<Mutex<LocalBoard>>,
        export_dir: PathBuf,
        export_interval: Duration,
    ) -> Self {
        Self {
            local_board,
            export_dir,
            export_interval,
        }
    }

    /// 将本地绘版数据导出为图片
    pub async fn export_board_image(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 确保导出目录存在
        fs::create_dir_all(&self.export_dir)?;
        
        // 获取当前时间戳用于文件名
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        
        // 构建输出文件路径
        let output_path = self.export_dir.join(format!("paintboard_{}.png", timestamp));
        
        // 获取本地绘版数据并导出为图片
        {
            let board = self.local_board.lock().await;
            
            if !board.is_initialized() {
                warn!("绘版数据尚未初始化，跳过本次导出");
                return Ok(());
            }
            
            let (width, height) = board.dimensions();
            let mut img = RgbImage::new(width as u32, height as u32);
            
            // 遍历所有像素并设置到图片中
            for y in 0..height {
                for x in 0..width {
                    if let Some(color) = board.get_pixel(x, y) {
                        let pixel = Rgb([color.r, color.g, color.b]);
                        img.put_pixel(x as u32, y as u32, pixel);
                    }
                }
            }
            
            // 保存图片
            img.save(&output_path)?;
        }
        
        info!("绘版图片已成功导出到: {:?}", output_path);
        Ok(())
    }

    /// 启动定时导出服务
    pub async fn start_export_service(&self) -> Result<(), Box<dyn std::error::Error>> {
        let local_board = self.local_board.clone();
        let export_dir = self.export_dir.clone();
        let export_interval = self.export_interval;
        
        tokio::spawn(async move {
            let mut interval_timer = interval(export_interval);
            
            loop {
                interval_timer.tick().await;
                
                // 创建临时导出管理器用于单次导出
                let temp_export_manager = ExportManager {
                    local_board: local_board.clone(),
                    export_dir: export_dir.clone(),
                    export_interval: Duration::from_secs(0), // 不会在临时实例中使用
                };
                
                if let Err(e) = temp_export_manager.export_board_image().await {
                    error!("导出绘版图片时发生错误: {:?}", e);
                }
            }
        });
        
        Ok(())
    }
}

/// 便捷函数：根据参数启动导出服务
pub async fn start_export_if_enabled(
    sync_manager: &BoardSyncManager,
    enable_export: bool,
    export_dir: String,
    export_interval: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if enable_export {
        info!("启用绘版图片导出功能，导出间隔: {} 秒，导出目录: {}", 
              export_interval, export_dir);

        let export_manager = ExportManager::new(
            sync_manager.local_board(),
            PathBuf::from(export_dir),
            Duration::from_secs(export_interval),
        );
        
        export_manager.start_export_service().await?;
        
        info!("绘版图片导出服务已启动");
    }
    
    Ok(())
}