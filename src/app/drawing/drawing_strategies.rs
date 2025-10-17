 // 模块: src/app/drawing/drawing_strategies.rs
 //! 渐进式绘制策略（例如棋盘格）实现

 use log::{debug, error, info, warn};
 use tokio::time::Duration;
 use winter_paintboard_sdk::PaintboardClientTrait;
 use std::sync::Arc;
 use tokio::sync::Mutex;
 use std::sync::atomic::{AtomicUsize, Ordering};

 use super::progressive_mode::ProgressiveMode;
 use crate::app::image_processing::ProcessedImageData;

 /// Draws an image to the paintboard using various modes
 pub async fn draw_image_to_paintboard(
     client: &Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
     processed_image_data: ProcessedImageData,
     progressive_mode: &ProgressiveMode,
     max_batch_size: usize,
     delay: u64,
 ) -> Result<(), Box<dyn std::error::Error>> {
     draw_image_to_paintboard_with_client(
         client,
         &processed_image_data,
         progressive_mode,
         max_batch_size,
         delay,
     )
     .await
 }

 /// Draws an image to the paintboard using various modes with processed image data
 pub async fn draw_image_to_paintboard_with_client(
     client: &Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
     processed_image_data: &ProcessedImageData,
     progressive_mode: &ProgressiveMode,
     max_batch_size: usize,
     delay: u64,
 ) -> Result<(), Box<dyn std::error::Error>> {
     match progressive_mode {
         ProgressiveMode::Chessboard => {}
         _ => {
             warn!(
                 "当前系统仅支持 Chessboard 渐近模式！{:?} 将被回退到到 Chessboard 模式",
                 progressive_mode
             )
         }
     }

     // 棋盘格渐进式绘制模式
     info!("使用棋盘格渐进式绘制模式...");
     let total_pixels = processed_image_data.full_scale_operations.len();
     let processed_progressive = Arc::new(AtomicUsize::new(0));

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

         // 并行发送当前步骤的所有批次：对每个批次 spawn 一个任务，任务内部会 lock 客户端并发送
         let mut tasks = Vec::new();

         for chunk in step_draw_operations.chunks(max_batch_size) {
             let client_clone = client.clone();
             let chunk_vec = chunk.to_vec();
             let delay_ms = delay;
             let chunk_len = chunk.len();
             let processed_progressive_clone = processed_progressive.clone();
             let step_clone = step;
             let total_pixels_clone = total_pixels;

             let task = tokio::spawn(async move {
                 // 在任务内部短暂延迟以错开请求，降低瞬时压力
                 tokio::time::sleep(Duration::from_millis(delay_ms)).await;

                 let mut client_lock = client_clone.lock().await;
                 match client_lock.as_mut().paint_batch(chunk_vec).await {
                     Ok(()) => {
                         processed_progressive_clone.fetch_add(chunk_len, Ordering::Relaxed);
                         debug!(
                             "Step {} 批次发送完成，已发送: {}/{} 像素",
                             step_clone,
                             processed_progressive_clone.load(Ordering::Relaxed),
                             total_pixels_clone
                         );
                     }
                     Err(e) => {
                         error!("Step {} 批量绘制错误: {:?}", step_clone, e);
                         // 继续处理下一个批次，而不是中断
                     }
                 }
             });

             tasks.push(task);

             // 防止短时间内产生过多并发任务
             tokio::time::sleep(Duration::from_millis(10)).await;
         }

         // 等待当前步骤的所有任务完成
         for task in tasks {
             let _ = task.await;
         }

         info!(
             "Step {} 绘制完成，累计发送: {}/{} 像素",
             step,
             processed_progressive.load(Ordering::Relaxed),
             total_pixels
         );

         // 在每个步骤之间添加延迟，以产生\"逐步清晰\"的视觉效果
         if step < steps - 1 {
             // 最后一步后不需要等待
             tokio::time::sleep(Duration::from_millis(delay)).await;
         }
     }

     info!(
         "棋盘格渐进式绘制发送完成，总共发送了 {} 个像素（不等待响应确认）",
         processed_progressive.load(Ordering::Relaxed)
     );

     Ok(())
 }
