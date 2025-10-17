// 模块: src/app/incremental/restoration_manager.rs
//! 批量恢复像素的实现（从原 `incremental.rs` 提取）
//! 提供按批次发送像素恢复请求并等待任务完成的工具函数。

use log::{debug, error, info};
use std::error::Error;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use winter_paintboard_sdk::{PaintboardClientTrait, Pos, Rgb};

/// 按批次恢复像素并并发发送，每个批次在发送前会等待 `restore_delay`。
pub async fn restore_pixels_in_batches(
    client: &Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
    pixels: Vec<(Pos, Rgb)>,
    restore_delay: Duration,
    max_batch_size: usize,
) -> Result<(), Box<dyn Error>> {
    if pixels.is_empty() {
        info!("没有需要恢复的像素");
        return Ok(());
    }

    let pixels_count = pixels.len();
    info!(
        "开始恢复 {} 个像素，批量大小: {}",
        pixels_count, max_batch_size
    );

    let mut tasks = Vec::new();
    for chunk in pixels.chunks(max_batch_size) {
        let client_clone = client.clone();
        let chunk_vec = chunk.to_vec();
        let delay = restore_delay;
        let chunk_len = chunk.len();

        let task = tokio::spawn(async move {
            // 短暂延迟以避免同时发送所有批次
            sleep(delay).await;

            let mut client_lock = client_clone.lock().await;
            match client_lock.as_mut().paint_batch(chunk_vec).await {
                Ok(()) => debug!("成功恢复像素批次，包含 {} 个像素", chunk_len),
                Err(e) => error!("批量恢复像素失败: {:?}", e),
            }
        });

        tasks.push(task);

        // 防止短时间内产生过多并发任务
        sleep(Duration::from_millis(10)).await;
    }

    // 等待所有任务完成
    for task in tasks {
        let _ = task.await;
    }

    info!("完成恢复 {} 个像素", pixels_count);
    Ok(())
}

/// 简洁封装：接收已排序的像素（按优先级），并调用批量恢复函数。
pub async fn restore_sorted_pixels(
    client: &Arc<Mutex<Box<dyn PaintboardClientTrait + Send>>>,
    sorted_pixels: Vec<(Pos, Rgb)>,
    restore_delay: Duration,
    max_batch_size: usize,
) -> Result<(), Box<dyn Error>> {
    restore_pixels_in_batches(client, sorted_pixels, restore_delay, max_batch_size).await
}
