pub mod board_sync;
pub mod cli;
pub mod drawing;
pub mod export;
pub mod image_processing;
pub mod incremental;
pub mod utils;

use log::{error, info};
use tokio::time::Duration;
use winter_paintboard_sdk::client::HttpProvider;
use winter_paintboard_sdk::config::Config;

use crate::app::{
    board_sync::BoardSyncManager,
    cli::Cli,
    drawing::{create_client, ProgressiveMode},
    export::start_export_if_enabled,
    image_processing::process_image_at_all_scales,
    incremental::start_incremental_if_enabled,
    utils::{get_token_with_access_key, validate_auth_args},
};

use crate::app::cli::Commands;

/// Main application logic for drawing an image to the paintboard
pub async fn run_app(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Incremental {
            token,
            uid,
            access_key,
            ws_url,
            image,
            x,
            y,
            width,
            height,
            monitor_interval,
            restore_delay,
            max_batch_size,
            client_type,
        } => {
            // 增量修改模式
            run_incremental_mode(
                token,
                uid,
                access_key,
                ws_url,
                image,
                x,
                y,
                width,
                height,
                monitor_interval,
                restore_delay,
                max_batch_size,
                client_type,
            )
            .await
        }
        Commands::DrawLoop {
            token,
            uid,
            access_key,
            ws_url,
            image,
            x,
            y,
            width,
            height,
            delay,
            batch_mode,
            max_batch_size,
            loop_interval,
            progressive,
            client_type,
        } => {
            // 循环绘制模式
            run_draw_loop_mode(
                token,
                uid,
                access_key,
                ws_url,
                image,
                x,
                y,
                width,
                height,
                delay,
                batch_mode,
                max_batch_size,
                loop_interval,
                progressive,
                client_type,
            )
            .await
        }
        Commands::DrawOnce {
            token,
            uid,
            access_key,
            ws_url,
            image,
            x,
            y,
            width,
            height,
            delay,
            batch_mode,
            max_batch_size,
            progressive,
            wait_for_completion,
            client_type,
        } => {
            // 单次绘制模式
            run_draw_once_mode(
                token,
                uid,
                access_key,
                ws_url,
                image,
                x,
                y,
                width,
                height,
                delay,
                batch_mode,
                max_batch_size,
                progressive,
                wait_for_completion,
                client_type,
            )
            .await
        }
        Commands::GetBoard {
            token,
            uid,
            access_key,
            api_url,
            output,
        } => {
            // 获取画板状态
            run_get_board_mode(token, uid, access_key, api_url, output).await
        }
        Commands::About => {
            // 显示项目信息和作者信息
            run_about_mode().await
        }
    }
}

async fn run_incremental_mode(
    token: Option<String>,
    uid: u32,
    access_key: Option<String>,
    ws_url: Option<String>,
    image: std::path::PathBuf,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
    monitor_interval: u64,
    restore_delay: u64,
    max_batch_size: usize,
    client_type: winter_paintboard_sdk::ClientType,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate authentication arguments
    let token = match validate_auth_args(&token, &access_key) {
        Ok(token) => {
            if token.is_empty() {
                // Need to get token using access key
                if let Some(access_key) = &access_key {
                    get_token_with_access_key(uid, access_key).await?
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
    config.ws_url = ws_url
        .clone()
        .unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
    let client_box = create_client(config, client_type).await?;
    let client = std::sync::Arc::new(tokio::sync::Mutex::new(client_box));
    {
        // 设置认证信息（通过加锁设置）
        let mut cl = client.lock().await;
        cl.as_mut().set_auth(uid, token.to_string());
    }

    // 检查是否启用本地同步
    info!("启用本地绘版数据同步...");

    // 获取EventBus实例
    let event_bus = winter_paintboard_sdk::event::EventBus::global();

    // 创建同步管理器
    let sync_manager = BoardSyncManager::new(&event_bus);

    // 为同步任务创建新的客户端
    let mut sync_config = Config::default();
    sync_config.ws_url = ws_url
        .clone()
        .unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
    let mut sync_client = create_client(sync_config, client_type).await?;
    sync_client.as_mut().set_auth(uid, token.to_string());

    // 启动增量同步循环
    sync_manager
        .start_incremental_sync_loop(
            sync_client,
            Duration::from_secs(monitor_interval), // 使用命令行传入的监控间隔作为同步间隔
        )
        .await?;

    // 启动事件监听（增量更新）
    sync_manager.start_event_listener().await?;

    info!("本地绘版数据同步已启动");

    // 启动绘版图片导出服务（如果启用）
    start_export_if_enabled(
        &sync_manager,
        true,                  // 增量模式下默认启用导出
        "exports".to_string(), // 可以考虑从CLI添加导出目录参数
        monitor_interval,      // 使用命令行传入的监控间隔作为导出间隔
    )
    .await?;

    // 启动增量修改模式
    // 在增量模式下，我们先预处理图像数据
    info!("正在预处理图片数据...");
    let processed_image_data = process_image_at_all_scales(&image, width, height, x, y)?;

    start_incremental_if_enabled(
        Some(token.clone()), // 传递token
        uid,
        ws_url, // 传递ws_url参数（现在会使用）
        &sync_manager,
        true, // 启用增量模式
        &processed_image_data,
        x,
        y,
        monitor_interval,
        restore_delay,
        max_batch_size, // 传递批处理大小参数
        client,         // 传递主客户端，支持连接池
    )
    .await?;

    // 在增量模式下，我们不再执行常规的绘图流程
    // 而是保持程序运行以持续监控和修复
    info!("增量修改模式已启动，程序将持续运行以监控和修复绘版...");
    // 保持程序运行
    tokio::signal::ctrl_c().await.expect("等待信号处理失败");
    info!("接收到中断信号，正在退出...");
    Ok(())
}

async fn run_draw_loop_mode(
    token: Option<String>,
    uid: u32,
    access_key: Option<String>,
    ws_url: Option<String>,
    image: std::path::PathBuf,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
    delay: u64,
    batch_mode: bool,
    max_batch_size: usize,
    loop_interval: u64,
    progressive: String,
    client_type: winter_paintboard_sdk::ClientType,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate authentication arguments
    let token = match validate_auth_args(&token, &access_key) {
        Ok(token) => {
            if token.is_empty() {
                // Need to get token using access key
                if let Some(access_key) = &access_key {
                    get_token_with_access_key(uid, access_key).await?
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
    config.ws_url = ws_url
        .clone()
        .unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
    let client_box = create_client(config, client_type).await?;
    let client = std::sync::Arc::new(tokio::sync::Mutex::new(client_box));
    {
        // 设置认证信息（通过加锁设置）
        let mut cl = client.lock().await;
        cl.as_mut().set_auth(uid, token.to_string());
    }

    // Determine progressive mode
    let progressive_mode = ProgressiveMode::from_string(&progressive);

    // 在 run_draw_loop_mode 函数内部进行图像预处理，这样在循环模式下只需处理一次
    info!("正在预处理图片数据...");
    let processed_image_data = process_image_at_all_scales(&image, width, height, x, y)?;

    info!("开始循环绘制模式，时间间隔: {} 毫秒", loop_interval);
    loop {
        // 在循环内部使用已预处理的数据，避免重复预处理
        if let Err(e) = crate::app::drawing::draw_image_to_paintboard_with_client(
            &client,
            &processed_image_data,
            &progressive_mode,
            max_batch_size,
            delay,
        )
        .await
        {
            error!("绘制图片时发生错误: {:?}", e);
        }

        info!("等待 {} 毫秒后再次绘制...", loop_interval);
        tokio::time::sleep(Duration::from_millis(loop_interval)).await;
    }
}

async fn run_draw_once_mode(
    token: Option<String>,
    uid: u32,
    access_key: Option<String>,
    ws_url: Option<String>,
    image: std::path::PathBuf,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
    delay: u64,
    batch_mode: bool,
    max_batch_size: usize,
    progressive: String,
    wait_for_completion: bool,
    client_type: winter_paintboard_sdk::ClientType,
) -> Result<(), Box<dyn std::error::Error>> {
    // Validate authentication arguments
    let token = match validate_auth_args(&token, &access_key) {
        Ok(token) => {
            if token.is_empty() {
                // Need to get token using access key
                if let Some(access_key) = &access_key {
                    get_token_with_access_key(uid, access_key).await?
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
    config.ws_url = ws_url
        .clone()
        .unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
    let client_box = create_client(config, client_type).await?;
    let client = std::sync::Arc::new(tokio::sync::Mutex::new(client_box));
    {
        // 设置认证信息（通过加锁设置）
        let mut cl = client.lock().await;
        cl.as_mut().set_auth(uid, token.to_string());
    }

    // Determine progressive mode
    let progressive_mode = ProgressiveMode::from_string(&progressive);

    // 在 run_draw_once_mode 函数内部进行图像预处理
    info!("正在预处理图片数据...");
    let processed_image_data = process_image_at_all_scales(&image, width, height, x, y)?;

    // 单次绘制模式 - 使用已预处理的数据
    crate::app::drawing::draw_image_to_paintboard_with_client(
        &client,
        &processed_image_data,
        &progressive_mode,
        max_batch_size,
        delay,
    )
    .await?;

    info!(
        "图片绘制完成！图片尺寸: {}x{}, 起始坐标: ({}, {})",
        processed_image_data.img_width, processed_image_data.img_height, x, y
    );

    if wait_for_completion {
        info!("等待所有操作完成...");
        // 这里可以添加一些逻辑来等待操作完成
    }

    Ok(())
}

async fn run_get_board_mode(
    token: Option<String>,
    uid: u32,
    access_key: Option<String>,
    api_url: Option<String>,
    output: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 这部分需要实现获取画板状态的逻辑
    info!("获取画板状态...");

    // Validate authentication arguments
    let token = match validate_auth_args(&token, &access_key) {
        Ok(token) => {
            if token.is_empty() {
                // Need to get token using access key
                if let Some(access_key) = &access_key {
                    get_token_with_access_key(uid, access_key).await?
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

    // 初始化客户端配置
    let mut config = Config::default();
    if let Some(url) = api_url {
        config.api_base_url = url;
    }

    // 创建HTTP客户端来获取画板数据
    let http_client = HttpProvider::new(std::sync::Arc::new(config))?;

    let board = http_client.get_board().await?;

    match output {
        Some(path) => {
            // 保存到文件
            info!("将画板数据保存到: {}", path);
            // 这里可以添加将画板数据保存为图像文件的逻辑
        }
        None => {
            // 输出到控制台
            info!("画板尺寸: {}x{}", board.width, board.height);
            info!("成功获取画板数据");
        }
    }

    Ok(())
}

async fn run_about_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        r#"
Winter Paintboard Helper
========================

Version: {}
Author:  Xyber Nova <xyber-nova@outlook.com>
GitHub:  https://github.com/Mengzi-No-1-High-School-Unofficial/LGS-Paintboard-Helper
License: AGPL-3.0

这是一个用于在洛谷画板上绘制图像的命令行工具。
支持增量修改、循环绘制和单次绘制等多种模式。

功能特性:
- 支持单次绘制、循环绘制和增量修改模式
- 连接池管理，提高性能和稳定性
- 本地画板数据同步功能
- 渐进式绘制模式（棋盘、缩放等方式）
- 可配置的批量绘制和速率限制

如需帮助，请使用 --help 参数。
    "#,
        env!("CARGO_PKG_VERSION")
    );

    Ok(())
}
