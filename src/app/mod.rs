pub mod cli;
pub mod image_processing;
pub mod drawing;
pub mod utils;

use log::{info, error};
use tokio::time::Duration;
use winter_paintboard_sdk::config::Config;

use crate::app::{
    cli::Cli,
    image_processing::{read_and_resize_image, prepare_draw_operations},
    drawing::{draw_image_to_paintboard, ProgressiveMode, create_client},
    utils::{get_token_with_access_key, validate_auth_args},
};

/// Main application logic for drawing an image to the paintboard
pub async fn run_app(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    // Validate authentication arguments
    let token = match validate_auth_args(&cli.token, &cli.access_key) {
        Ok(token) => {
            if token.is_empty() {
                // Need to get token using access key
                if let Some(access_key) = &cli.access_key {
                    get_token_with_access_key(cli.uid, access_key).await?
                } else {
                    error!("错误: 必须提供 --token 或 --access_key");
                    std::process::exit(1);
                }
            } else {
                token
            }
        }
        Err(e) => {
            error!("{}", e);
            std::process::exit(1);
        }
    };

    // Determine progressive mode
    let progressive_mode = ProgressiveMode::from_string(&cli.progressive);

    // Check if loop mode is enabled
    if cli.loop_draw {
        info!("开始循环绘制模式，时间间隔: {} 毫秒", cli.loop_interval);
        loop {
            if let Err(e) = draw_single_image(&cli, &token, &progressive_mode).await {
                error!("绘制图片时发生错误: {:?}", e);
            }
            
            info!("等待 {} 毫秒后再次绘制...", cli.loop_interval);
            tokio::time::sleep(Duration::from_millis(cli.loop_interval)).await;
        }
    } else {
        // Single draw mode
        draw_single_image(&cli, &token, &progressive_mode).await?;
    }
    
    Ok(())
}

/// Draws a single image to the paintboard
async fn draw_single_image(
    cli: &Cli,
    token: &str,
    progressive_mode: &ProgressiveMode,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read and resize the image
    let rgba_img = read_and_resize_image(&cli.image, cli.width, cli.height)?;
    let (img_width, img_height) = rgba_img.dimensions();

    // Prepare draw operations
    info!("正在准备绘制数据...");
    let draw_operations = prepare_draw_operations(&rgba_img, cli.x, cli.y)?;
    info!("总共准备了 {} 个绘制操作", draw_operations.len());

    // Initialize paintboard client config
    info!("正在初始化绘板客户端...");
    let mut config = Config::default(); // 使用默认配置
    // 确保使用正确的WebSocket端点
    config.ws_url = cli.ws_url.clone().unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
    let mut client = create_client(config).await?; // 使用新的API
    
    // 设置认证信息
    client.set_auth(cli.uid, token.to_string());

    // Draw the image based on the selected mode
    draw_image_to_paintboard(
        &mut client,
        draw_operations,
        progressive_mode,
        cli.max_batch_size,
        cli.delay,
        cli.batch_mode,
    ).await?;

    info!("图片绘制完成！图片尺寸: {}x{}, 起始坐标: ({}, {})", img_width, img_height, cli.x, cli.y);
    Ok(())
}