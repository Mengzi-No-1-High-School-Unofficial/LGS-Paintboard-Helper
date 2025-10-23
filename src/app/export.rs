use image::{Rgb, RgbImage};
use log::{error, info, warn};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{interval, Duration};

use crate::app::board_sync::{BoardSyncManager, LocalBoard};

/// 图片导出管理器
pub struct ExportManager {
    local_board: Arc<RwLock<LocalBoard>>,
    export_dir: PathBuf,
    export_interval: Duration,
}

impl ExportManager {
    /// 创建新的导出管理器
    pub fn new(
        local_board: Arc<RwLock<LocalBoard>>,
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
        let output_path = self
            .export_dir
            .join(format!("paintboard_{}.png", timestamp));

        // 获取本地绘版数据并导出为图片
        {
            let board = self.local_board.read().await;

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

    /// 将热点图数据导出为图片
    pub async fn export_heatmap_image(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 确保导出目录存在
        fs::create_dir_all(&self.export_dir)?;

        // 获取当前时间戳用于文件名
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        // 构建输出文件路径
        let output_path = self
            .export_dir
            .join(format!("heatmap_{}.png", timestamp));

        // 获取本地绘版的热点图数据并导出为图片
        {
            let board = self.local_board.read().await;

            if !board.is_initialized() {
                warn!("绘版数据尚未初始化，跳过本次导出");
                return Ok(());
            }

            let (width, height) = board.dimensions();
            let mut img = RgbImage::new(width as u32, height as u32);

            // 读取热点图数据
            let heatmap = board.get_heatmap();
            let heatmap_data = heatmap.read().await;

            // 遍历所有像素位置，根据热点图值设置颜色
            for y in 0..height {
                for x in 0..width {
                    let pos = winter_paintboard_sdk::models::Pos::new(x, y).unwrap();
                    
                    // 根据热点图值确定颜色强度
                    let intensity = *heatmap_data.get(&pos).unwrap_or(&0);
                    
                    // 将强度值转换为颜色（强度越高越红）
                    let pixel = self.intensity_to_color(intensity);
                    img.put_pixel(x as u32, y as u32, pixel);
                }
            }

            // 保存图片
            img.save(&output_path)?;
        }

        info!("热点图已成功导出到: {:?}", output_path);
        Ok(())
    }

    /// 将强度值转换为RGB颜色
    /// 强度值越大，颜色越偏向红色，表示热点区域
    fn intensity_to_color(&self, intensity: u32) -> Rgb<u8> {
        // 根据强度值生成颜色，强度越高越红
        // 可以根据需要调整颜色映射算法
        let max_display_intensity = 100; // 设定一个最大显示强度，超过此值颜色不再变化
        let normalized_intensity = std::cmp::min(intensity, max_display_intensity);
        
        // 创建一个从蓝色(低强度)到红色(高强度)的渐变
        let ratio = normalized_intensity as f32 / max_display_intensity as f32;
        
        let r = (255.0 * ratio) as u8;
        let g = (128.0 * (1.0 - ratio)) as u8;
        let b = (255.0 * (1.0 - ratio)) as u8;
        
        Rgb([r, g, b])
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
        info!(
            "启用绘版图片导出功能，导出间隔: {} 秒，导出目录: {}",
            export_interval, export_dir
        );

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

/// 便捷函数：根据参数启动热点图导出服务
pub async fn start_heatmap_export_if_enabled(
    sync_manager: &BoardSyncManager,
    enable_heatmap_export: bool,
    export_dir: String,
    export_interval: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    if enable_heatmap_export {
        info!(
            "启用热点图导出功能，导出间隔: {} 秒，导出目录: {}",
            export_interval, export_dir
        );

        // 启动热点图导出服务
        let local_board = sync_manager.local_board();
        let export_dir_clone = PathBuf::from(export_dir.clone()); // 克隆字符串以避免移动问题
        let export_interval_duration = Duration::from_secs(export_interval);

        tokio::spawn(async move {
            let mut interval_timer = interval(export_interval_duration);

            loop {
                interval_timer.tick().await;

                // 创建临时导出管理器用于单次导出
                let temp_export_manager = ExportManager {
                    local_board: local_board.clone(),
                    export_dir: export_dir_clone.clone(),
                    export_interval: Duration::from_secs(0), // 不会在临时实例中使用
                };

                if let Err(e) = temp_export_manager.export_heatmap_image().await {
                    error!("导出热点图时发生错误: {:?}", e);
                }
            }
        });

        info!("热点图导出服务已启动");
    }

    Ok(())
}
