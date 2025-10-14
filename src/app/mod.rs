pub mod cli;
pub mod image_processing;
pub mod drawing;
pub mod utils;
pub mod board_sync;

use log::{info, error};
use tokio::time::Duration;
use winter_paintboard_sdk::config::Config;

use crate::app::{
    cli::Cli,
    image_processing::{process_image_at_all_scales, ProcessedImageData},
    drawing::{draw_image_to_paintboard, ProgressiveMode, create_client},
    utils::{get_token_with_access_key, validate_auth_args},
    board_sync::{BoardSyncManager, LocalBoard},
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

    // Initialize paintboard client config
    info!("正在初始化绘板客户端...");
    let mut config = Config::default(); // 使用默认配置
    // 确保使用正确的WebSocket端点
    config.ws_url = cli.ws_url.clone().unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
    let mut client = create_client(config).await?; // 使用新的API
    
    // 设置认证信息
    client.set_auth(cli.uid, token.to_string());

    // Determine progressive mode
    let progressive_mode = ProgressiveMode::from_string(&cli.progressive);

    // 检查是否启用本地同步
    if cli.enable_local_sync {
        info!("启用本地绘版数据同步，同步间隔: {} 秒", cli.sync_interval);
        
        // 获取EventBus实例
        let event_bus = winter_paintboard_sdk::event::EventBus::global();
        
        // 创建同步管理器
        let sync_manager = BoardSyncManager::new(&event_bus);
        
        // 为同步任务创建新的客户端
        let sync_config = Config::default();
        let mut sync_client = create_client(sync_config).await?;
        sync_client.set_auth(cli.uid, token.to_string());
        
        // 启动全量同步循环
        sync_manager.start_sync_loop(
            sync_client,
            Duration::from_secs(cli.sync_interval)
        ).await?;
        
        // 启动事件监听（增量更新）
        sync_manager.start_event_listener().await?;
        
        info!("本地绘版数据同步已启动");
    }

    // 在 run_app 函数内部进行图像预处理，这样在循环模式下只需处理一次
    info!("正在预处理图片数据...");
    let processed_image_data = process_image_at_all_scales(
        &cli.image,
        cli.width,
        cli.height,
        cli.x,
        cli.y,
    )?;

    // Check if loop mode is enabled
    if cli.loop_draw {
        info!("开始循环绘制模式，时间间隔: {} 毫秒", cli.loop_interval);
        loop {
            // 在循环内部使用已预处理的数据，避免重复预处理
            if let Err(e) = crate::app::drawing::draw_image_to_paintboard_with_client(
                &mut client,
                &processed_image_data,
                &progressive_mode,
                cli.max_batch_size,
                cli.delay,
                cli.batch_mode,
            ).await {
                error!("绘制图片时发生错误: {:?}", e);
            }
            
            info!("等待 {} 毫秒后再次绘制...", cli.loop_interval);
            tokio::time::sleep(Duration::from_millis(cli.loop_interval)).await;
        }
    } else {
        // Single draw mode - 使用已预处理的数据
        crate::app::drawing::draw_image_to_paintboard_with_client(
            &mut client,
            &processed_image_data,
            &progressive_mode,
            cli.max_batch_size,
            cli.delay,
            cli.batch_mode,
        ).await?;
    }
    
    info!("图片绘制完成！图片尺寸: {}x{}, 起始坐标: ({}, {})", 
          processed_image_data.img_width, 
          processed_image_data.img_height, 
          cli.x, cli.y);
    Ok(())
}