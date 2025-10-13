use log::{debug, error, info, warn};
use tokio::time::Duration;
use winter_paintboard_sdk::{PaintboardClient, config::Config};

use crate::app::image_processing::ProcessedImageData;

/// Represents different progressive drawing modes
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
    client: &mut PaintboardClient,
    processed_image_data: ProcessedImageData,
    progressive_mode: &ProgressiveMode,
    max_batch_size: usize,
    delay: u64,
    batch_mode: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    draw_image_to_paintboard_with_client(
        client,
        &processed_image_data,
        progressive_mode,
        max_batch_size,
        delay,
        batch_mode,
    ).await
}

/// Draws an image to the paintboard using various modes with processed image data
pub async fn draw_image_to_paintboard_with_client(
    client: &mut PaintboardClient,
    processed_image_data: &ProcessedImageData,
    progressive_mode: &ProgressiveMode,
    max_batch_size: usize,
    delay: u64,
    batch_mode: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match progressive_mode {
        ProgressiveMode::Chessboard => {
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
                    if (((pos.x as u32 + pos.y as u32)) % steps) == step {
                        step_draw_operations.push((*pos, *color));
                    }
                }

                if step_draw_operations.is_empty() {
                    info!("Step {} 没有需要绘制的像素，跳过", step);
                    continue;
                }

                info!("开始绘制 Step {}: {} 个像素...", step, step_draw_operations.len());
                
                // 对当前步骤的像素进行批量发送
                for chunk in step_draw_operations.chunks(max_batch_size) {
                    match client.paint_batch(chunk.to_vec()).await {
                        Ok(()) => {
                            processed_progressive += chunk.len();
                            debug!("Step {} 批次发送完成，已发送: {}/{} 像素", step, processed_progressive, total_pixels);
                            
                            // 在批次之间添加延迟以避免速率限制
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }
                        Err(e) => {
                            error!("Step {} 批量绘制错误: {:?}", step, e);
                            // 继续处理下一个批次，而不是中断
                        }
                    }
                }

                info!("Step {} 绘制完成，累计发送: {}/{} 像素", step, processed_progressive, total_pixels);

                // 在每个步骤之间添加延迟，以产生"逐步清晰"的视觉效果
                if step < steps - 1 { // 最后一步后不需要等待
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }

            info!("棋盘格渐进式绘制发送完成，总共发送了 {} 个像素（不等待响应确认）", processed_progressive);
        }
        ProgressiveMode::Scale => {
            // 缩放渐进式绘制模式
            info!("使用缩放渐进式绘制模式...");
            let mut processed_scaled = 0;

            // 定义缩放层级 (例如: 1/4, 1/2, 1 (全尺寸))
            let scale_factors = vec![0.25, 0.5, 1.0];

            // Process each scale level
            for (scale_index, &factor) in scale_factors.iter().enumerate() {
                let level_operations = if factor == 1.0 {
                    // For the full scale, use full scale operations
                    &processed_image_data.full_scale_operations
                } else {
                    // For smaller scale factors, get from pre-calculated operations
                    // Map scale factor index (0.25 -> index 0, 0.5 -> index 1)
                    let level_idx = if factor == 0.25 { 0 } else if factor == 0.5 { 1 } else { continue; };
                    if level_idx >= processed_image_data.scale_level_operations.len() {
                        continue;
                    }
                    &processed_image_data.scale_level_operations[level_idx]
                };

                if level_operations.is_empty() {
                    info!("Scale level {} (factor {}) has no drawable pixels, skipping", scale_index, factor);
                    continue;
                }

                info!("开始绘制 Scale Level {}: Factor {}, {} 个像素...", scale_index, factor, level_operations.len());

                for chunk in level_operations.chunks(max_batch_size) {
                    match client.paint_batch(chunk.to_vec()).await {
                        Ok(()) => {
                            processed_scaled += chunk.len();
                            debug!("Scale Level {} 批次发送完成，已发送: {}/{} 像素", scale_index, processed_scaled, level_operations.len());
                            
                            // 在批次之间添加延迟以避免速率限制
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }
                        Err(e) => {
                            error!("Scale Level {} 批量绘制错误: {:?}", scale_index, e);
                            // 继续处理下一个批次，而不是中断
                        }
                    }
                }

                info!("Scale Level {} 绘制完成，累计发送: {}/{} 像素", scale_index, processed_scaled, level_operations.len());

                // 在每个层级之间添加延迟，以产生"逐步清晰"的视觉效果
                if scale_index < scale_factors.len() - 1 { // 最后一层后不需要等待
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                }
            }

            info!("缩放渐进式绘制发送完成，总共发送了 {} 个像素（不等待响应确认）", processed_scaled);
        }
        ProgressiveMode::None => {
            // 标准批量或逐个绘制模式
            let total_pixels = processed_image_data.full_scale_operations.len();
            if batch_mode && !processed_image_data.full_scale_operations.is_empty() {
                // 使用批量绘制模式（粘包机制），支持分批，不等待响应
                info!("使用批量绘制模式，最大批量大小: {}, 总共 {} 个像素...", max_batch_size, total_pixels);
                
                let mut processed = 0;
                
                // 按最大批量大小分批处理
                for chunk in processed_image_data.full_scale_operations.chunks(max_batch_size) {
                    match client.paint_batch(chunk.to_vec()).await {
                        Ok(()) => {
                            processed += chunk.len();
                            debug!("批次发送完成，已发送: {}/{} 像素", processed, total_pixels);
                            
                            // 在批次之间添加延迟以避免速率限制
                            tokio::time::sleep(Duration::from_millis(delay)).await;
                        }
                        Err(e) => {
                            error!("批量绘制错误: {:?}", e);
                            // 继续处理下一个批次，而不是中断
                        }
                    }
                }
                
                info!("批量绘制发送完成，总共发送了 {} 个像素（不等待响应确认）", processed);
            } else if !batch_mode {
                // 使用逐个绘制模式
                let mut successful_draws = 0;
                let mut failed_draws = 0;
                
                for (i, (pos, color)) in processed_image_data.full_scale_operations.iter().enumerate() {
                    match client.paint(*pos, *color).await {
                        Ok(result) => {
                            match result.status {
                                winter_paintboard_sdk::models::PaintStatus::Success => {
                                    successful_draws += 1;
                                    debug!("绘制成功 (位置: {},{}) - Drawing ID: {}", pos.x, pos.y, result.drawing_id);
                                },
                                winter_paintboard_sdk::models::PaintStatus::Cooldown => {
                                    warn!("绘制冷却中，稍等... (位置: {},{}) - Drawing ID: {}", pos.x, pos.y, result.drawing_id);
                                    failed_draws += 1;
                                    // 等待一段时间以避免速率限制
                                    tokio::time::sleep(Duration::from_millis(100)).await;
                                },
                                winter_paintboard_sdk::models::PaintStatus::InvalidToken => {
                                    error!("无效的Token: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                    failed_draws += 1;
                                },
                                winter_paintboard_sdk::models::PaintStatus::NoPermission => {
                                    error!("无权限绘制: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                    failed_draws += 1;
                                },
                                winter_paintboard_sdk::models::PaintStatus::InvalidCoordinate => {
                                    error!("无效坐标: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                    failed_draws += 1;
                                },
                                winter_paintboard_sdk::models::PaintStatus::Timeout => {
                                    error!("请求超时: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                    failed_draws += 1;
                                },
                                _ => {
                                    error!("绘制失败: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                    failed_draws += 1;
                                }
                            }
                        }
                        Err(e) => {
                            error!("绘制错误: {:?} (位置: {},{})", e, pos.x, pos.y);
                            failed_draws += 1;
                        }
                    }
                    
                    // 添加进度显示
                    if (i + 1) % 100 == 0 || i == total_pixels - 1 {
                        info!("进度: {}/{} 像素, 成功: {}, 失败: {}", i + 1, total_pixels, successful_draws, failed_draws);
                    }
                    
                    // 为避免速率限制，添加小延迟
                    if i < total_pixels - 1 {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Creates a new paintboard client with the given config
pub async fn create_client(config: Config) -> Result<PaintboardClient, Box<dyn std::error::Error>> {
    Ok(PaintboardClient::new(config).await?)
}