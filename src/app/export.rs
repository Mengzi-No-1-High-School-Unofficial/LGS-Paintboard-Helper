//! 画板数据导出模块
//!
//! 该模块提供了将本地画板数据导出为图片文件的功能，包括完整的画板状态
//! 和热点图（显示绘制频率的可视化图）。支持定时导出功能。

use image::{Rgb, RgbImage};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

use crate::app::board_sync::{BoardSyncManager, LocalBoard};

/// 图片导出管理器
///
/// 负责将本地画板数据导出为图片文件，支持完整的画板状态和热点图导出
pub struct ExportManager {
    local_board: Arc<LocalBoard>,
    export_dir: PathBuf,
    export_interval: Duration,
}

impl ExportManager {
    /// 创建新的导出管理器
    ///
    /// # 参数
    ///
    /// * `local_board` - 本地画板的共享引用
    /// * `export_dir` - 导出文件的目录路径
    /// * `export_interval` - 导出时间间隔
    ///
    /// # 返回值
    ///
    /// 返回配置好的导出管理器实例
    pub fn new(
        local_board: Arc<LocalBoard>,
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
    ///
    /// 将当前本地画板的状态导出为PNG图片文件，文件名包含时间戳
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功导出图片
    /// * `Err` - 导出过程中发生错误
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
            let board = &self.local_board;

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
    ///
    /// 将绘制频率数据导出为可视化热点图，颜色强度表示该位置的绘制频率
    ///
    /// # 返回值
    ///
    /// * `Ok())` - 成功导出热点图
    /// * `Err` - 导出过程中发生错误
    pub async fn export_heatmap_image(&self) -> Result<(), Box<dyn std::error::Error>> {
        // 确保导出目录存在
        fs::create_dir_all(&self.export_dir)?;

        // 获取当前时间戳用于文件名
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        // 构建输出文件路径
        let output_path = self.export_dir.join(format!("heatmap_{}.png", timestamp));

        // 获取本地绘版的热点图数据并导出为图片
        {
            let board = &self.local_board;

            if !board.is_initialized() {
                warn!("绘版数据尚未初始化，跳过本次导出");
                return Ok(());
            }

            let (width, height) = board.dimensions();
            let mut img = RgbImage::new(width as u32, height as u32);

            // 读取热点图数据
            let heatmap = board.get_heatmap();

            // 遍历所有像素位置，根据热点图值设置颜色
            for y in 0..height {
                for x in 0..width {
                    let pos = winter_paintboard_sdk::models::Pos::new(x, y).unwrap();

                    // 根据热点图中的时间戳数量确定颜色强度
                    let intensity = heatmap.get(&pos).map(|v| v.len()).unwrap_or(0);

                    // 将强度值转换为颜色（强度越高越红）
                    let pixel = self.intensity_to_color(intensity as u32);
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
    ///
    /// 根据绘制频率强度值生成对应的RGB颜色，强度越高越红，表示热点区域
    ///
    /// # 参数
    ///
    /// * `intensity` - 绘制频率强度值
    ///
    /// # 返回值
    ///
    /// 对应强度值的RGB颜色
    fn intensity_to_color(&self, intensity: u32) -> Rgb<u8> {
        // 根据强度值生成颜色，强度越高越红
        // 可以根据需要调整颜色映射算法
        let max_display_intensity = 15; // 设定一个最大显示强度，超过此值颜色不再变化
        let normalized_intensity = std::cmp::min(intensity, max_display_intensity);

        // 创建一个从蓝色(低强度)到红色(高强度)的渐变
        let ratio = normalized_intensity as f32 / max_display_intensity as f32;

        let r = (255.0 * ratio) as u8;
        let g = (128.0 * (1.0 - ratio)) as u8;
        let b = (255.0 * (1.0 - ratio)) as u8;

        Rgb([r, g, b])
    }

    /// 启动定时导出服务
    ///
    /// 启动一个后台任务，按照指定的时间间隔定期导出画板图片
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 成功启动导出服务
    /// * `Err` - 启动过程中发生错误
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
///
/// 根据启用标志决定是否启动画板图片导出服务
///
/// # 参数
///
/// * `sync_manager` - 画板同步管理器
/// * `enable_export` - 是否启用导出功能
/// * `export_dir` - 导出目录路径
/// * `export_interval` - 导出时间间隔（秒）
///
/// # 返回值
///
/// * `Ok(())` - 成功启动导出服务或选择不启动
/// * `Err` - 启动过程中发生错误
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
///
/// 根据启用标志决定是否启动热点图导出服务
///
/// # 参数
///
/// * `sync_manager` - 画板同步管理器
/// * `enable_heatmap_export` - 是否启用热点图导出功能
/// * `export_dir` - 导出目录路径
/// * `export_interval` - 导出时间间隔（秒）
///
/// # 返回值
///
/// * `Ok(())` - 成功启动热点图导出服务或选择不启动
/// * `Err` - 启动过程中发生错误
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
