use log::{debug, error, info, warn};
use tokio::time::Duration;
use winter_paintboard_sdk::{
    config::Config, create_client_by_type, BasicClient, ClientType, PaintboardClientTrait,
};

use crate::app::image_processing::ProcessedImageData;

/// Represents different progressive drawing modes
#[derive(Debug)]
pub enum ProgressiveMode {
    None,
    Chessboard,
    Scale,
}

impl ProgressiveMode {
    pub fn from_string(s: &str) -> Self {
        match s {
            "chessboard" => ProgressiveMode::Chessboard,
            "scale" => ProgressiveMode::Scale,
            _ => ProgressiveMode::None,
        }
    }
}

/// Draws an image to the paintboard using various modes
pub async fn draw_image_to_paintboard(
    client: &mut BasicClient,
    processed_image_data: ProcessedImageData,
    progressive_mode: &ProgressiveMode,
    max_batch_size: usize,
    delay: u64
) -> Result<(), Box<dyn std::error::Error>> {
    draw_image_to_paintboard_with_client(
        client,
        &processed_image_data,
        progressive_mode,
        max_batch_size,
        delay
    )
    .await
}

/// Draws an image to the paintboard using various modes with processed image data
pub async fn draw_image_to_paintboard_with_client(
    client: &mut dyn PaintboardClientTrait,
    processed_image_data: &ProcessedImageData,
    progressive_mode: &ProgressiveMode,
    max_batch_size: usize,
    delay: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    match progressive_mode {
        ProgressiveMode::Chessboard => {},
        _ => {
            warn!("当前系统仅支持 Chessboard 渐近模式！{:?} 将被回退到到 Chessboard 模式", progressive_mode)
        }
    }

    // 棋盘格渐进式绘制模式
    info!("使用棋盘格渐进式绘制模式...");
    let total_pixels = processed_image_data.full_scale_operations.len();
    let mut processed_progressive = 0;

    // 定义步进模式 (例如: 4步棋盘格模式)
    let steps: u32 = 4; // 明确指定类型为 u32
    let mut step_draw_operations = Vec::with_capacity(total_pixels / steps as usize); // 预估容量

    for step in 0..steps {
        step_draw_operations.clear(); // 清空上一步的数据

        for (pos, color) in &processed_image_data.full_scale_operations {
            // 根据像素坐标和当前步数决定是否绘制
            // 这里使用棋盘格模式的变种：Step 0 绘制 (pos.x + pos.y) % 4 == 0 的点，
            // Step 1 绘制 (pos.x + pos.y) % 4 == 1 的点，以此类推
            if ((pos.x as u32 + pos.y as u32) % steps) == step {
                step_draw_operations.push((*pos, *color));
            }
        }

        if step_draw_operations.is_empty() {
            info!("Step {} 没有需要绘制的像素，跳过", step);
            continue;
        }

        info!(
            "开始绘制 Step {}: {} 个像素...",
            step,
            step_draw_operations.len()
        );

        // 对当前步骤的像素进行批量发送
        for chunk in step_draw_operations.chunks(max_batch_size) {
            match client.paint_batch(chunk.to_vec()).await {
                Ok(()) => {
                    processed_progressive += chunk.len();
                    debug!(
                        "Step {} 批次发送完成，已发送: {}/{} 像素",
                        step, processed_progressive, total_pixels
                    );

                    // 在批次之间添加延迟以避免速率限制
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
                Err(e) => {
                    error!("Step {} 批量绘制错误: {:?}", step, e);
                    // 继续处理下一个批次，而不是中断
                }
            }
        }

        info!(
            "Step {} 绘制完成，累计发送: {}/{} 像素",
            step, processed_progressive, total_pixels
        );

        // 在每个步骤之间添加延迟，以产生"逐步清晰"的视觉效果
        if step < steps - 1 {
            // 最后一步后不需要等待
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
    }

    info!(
        "棋盘格渐进式绘制发送完成，总共发送了 {} 个像素（不等待响应确认）",
        processed_progressive
    );

    Ok(())
}

/// Creates a new paintboard client with the given config and client type
pub async fn create_client(
    config: Config,
    client_type: ClientType,
) -> Result<Box<dyn PaintboardClientTrait + Send>, Box<dyn std::error::Error>> {
    Ok(create_client_by_type(config, client_type).await?)
}
