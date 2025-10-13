use log::{debug, error, info, warn};
use tokio::time::Duration;
use winter_paintboard_sdk::{PaintboardClient, Rgb, Pos, config::Config};

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
    draw_operations: Vec<(Pos, Rgb)>,
    progressive_mode: &ProgressiveMode,
    max_batch_size: usize,
    delay: u64,
    batch_mode: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let total_pixels = draw_operations.len();
    
    match progressive_mode {
        ProgressiveMode::Chessboard => {
            // 棋盘格渐进式绘制模式
            info!("使用棋盘格渐进式绘制模式...");
            let mut processed_progressive = 0;

            // 定义步进模式 (例如: 4步棋盘格模式)
            let steps: u32 = 4; // 明确指定类型为 u32
            let mut step_draw_operations = Vec::with_capacity(draw_operations.len() / steps as usize); // 预估容量

            for step in 0..steps {
                step_draw_operations.clear(); // 清空上一步的数据

                for (pos, color) in &draw_operations {
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
                if factor == 1.0 {
                    // For the full scale, use all operations
                    info!("开始绘制 Scale Level {}: Factor {}, {} 个像素...", scale_index, factor, draw_operations.len());

                    for chunk in draw_operations.chunks(max_batch_size) {
                        match client.paint_batch(chunk.to_vec()).await {
                            Ok(()) => {
                                processed_scaled += chunk.len();
                                debug!("Scale Level {} 批次发送完成，已发送: {}/{} 像素", scale_index, processed_scaled, total_pixels);
                                
                                // 在批次之间添加延迟以避免速率限制
                                tokio::time::sleep(Duration::from_millis(delay)).await;
                            }
                            Err(e) => {
                                error!("Scale Level {} 批量绘制错误: {:?}", scale_index, e);
                                // 继续处理下一个批次，而不是中断
                            }
                        }
                    }
                } else {
                    // For smaller scale factors, we need to create a downscaled version
                    // This would require recreating the image at different scales
                    // For now, we'll skip this complexity but note that in a full implementation,
                    // we'd need to use the original RGBA image and scale it down
                    info!("Scale level {} (factor {}) requires downscaling which needs original image access", scale_index, factor);
                    // In a full implementation, we would:
                    // 1. Scale down the original image to the target factor
                    // 2. Generate draw operations for the scaled image
                    // 3. Send those operations to the client
                    continue;
                }

                info!("Scale Level {} 绘制完成，累计发送: {}/{} 像素", scale_index, processed_scaled, total_pixels);
            }

            info!("缩放渐进式绘制发送完成，总共发送了 {} 个像素（不等待响应确认）", processed_scaled);
        }
        ProgressiveMode::None => {
            // 标准批量或逐个绘制模式
            if batch_mode && !draw_operations.is_empty() {
                // 使用批量绘制模式（粘包机制），支持分批，不等待响应
                info!("使用批量绘制模式，最大批量大小: {}, 总共 {} 个像素...", max_batch_size, total_pixels);
                
                let mut processed = 0;
                
                // 按最大批量大小分批处理
                for chunk in draw_operations.chunks(max_batch_size) {
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
                
                for (i, (pos, color)) in draw_operations.into_iter().enumerate() {
                    match client.paint(pos, color).await {
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