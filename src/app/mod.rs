//! 应用程序主模块
//!
//! 该模块包含应用程序的核心逻辑，处理命令行参数并根据不同的子命令执行相应的功能。
//! 主要功能包括多Token绘制模式、获取画板状态和显示项目信息。

pub mod board_sync;
pub mod cli;
pub mod image_processing;
pub mod ipc;
pub mod multi_token;
pub mod utils;

use std::time::Duration;
use tracing::{error, info};
use winter_paintboard_sdk::basic_client::HttpProvider;
use winter_paintboard_sdk::config::Config;

use crate::app::{
    cli::Cli,
    utils::{get_token_with_access_key, validate_auth_args},
};

use crate::app::cli::Commands;

/// 运行应用程序的主逻辑
///
/// 根据命令行参数解析出的子命令执行相应的功能：
/// - GetBoard: 获取当前画板状态
/// - About: 显示项目信息和作者信息
/// - MultiToken: 启动多Token绘制模式
///
/// # 参数
///
/// * `cli` - 解析后的命令行参数结构体
///
/// # 返回值
///
/// * `Ok(())` - 程序正常执行完成
/// * `Err` - 执行过程中发生错误
pub async fn run_app(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
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
        Commands::Master {
            ws_url,
            sync_interval,
            socket_path,
        } => run_master_mode(ws_url, sync_interval, socket_path).await,
        Commands::Worker {
            master_socket,
            ws_url,
            config,
            image,
            x,
            y,
            width,
            height,
            comparison_interval,
            canny_low_thresh,
            canny_high_thresh,
            penalty_scale,
            batch_size,
        } => {
            run_worker_mode(
                master_socket,
                ws_url,
                config,
                image,
                x,
                y,
                width,
                height,
                comparison_interval,
                canny_low_thresh,
                canny_high_thresh,
                penalty_scale,
                batch_size,
            )
            .await
        }
    }
}

/// 运行获取画板状态模式
///
/// 该模式用于获取当前画板的状态，包括画板尺寸和像素数据。
/// 可以将结果输出到控制台或保存到文件。
///
/// # 参数
///
/// * `token` - 用户认证 Token（可选）
/// * `uid` - 用户 ID
/// * `access_key` - 访问密钥（可选）
/// * `api_url` - API 服务器 URL（可选）
/// * `output` - 输出文件路径（可选）
///
/// # 返回值
///
/// * `Ok(())` - 成功获取画板状态
/// * `Err` - 获取过程中发生错误
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
    let _token = match validate_auth_args(&token, &access_key) {
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

/// 运行关于模式
///
/// 显示项目的基本信息，包括版本号、作者信息、GitHub 链接和许可证信息。
///
/// # 返回值
///
/// * `Ok(())` - 成功显示项目信息
/// * `Err` - 显示过程中发生错误
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

/// 运行 Master 模式
///
/// Master 负责同步画板数据并通过 Unix Socket 分发给 Worker
async fn run_master_mode(
    ws_url: Option<String>,
    sync_interval: u64,
    socket_path: std::path::PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("启动 Master 模式...");

    // 配置
    let mut config = Config::default();
    if let Some(url) = ws_url {
        config.ws_url = url;
    }

    // 创建客户端
    let client = winter_paintboard_sdk::get_global_client(config).await?;

    // 创建并启动 Master
    let master = ipc::SyncMaster::new(
        socket_path,
        tokio::time::Duration::from_millis(sync_interval),
    );

    master
        .start(client)
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })
}

/// 运行 Worker 模式
///
/// Worker 从 Master 接收画板数据并执行绘制任务
#[allow(clippy::too_many_arguments)]
async fn run_worker_mode(
    master_socket: std::path::PathBuf,
    ws_url: Option<String>,
    config_path: std::path::PathBuf,
    image: std::path::PathBuf,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
    comparison_interval: u64,
    canny_low_thresh: f32,
    canny_high_thresh: f32,
    penalty_scale: f32,
    batch_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("启动 Worker 模式...");

    // 设置惩罚参数
    let _ = multi_token::cli::PENALTY_SENSITIVITY
        .set(penalty_scale)
        .map_err(|_e| {
            let e = color_eyre::Report::msg("无法设置值 PENALTY_SENSITIVITY");
            error!("{}", e);
            e
        });

    // 连接到 Master
    let worker = ipc::SyncWorker::new(master_socket);
    let local_board = worker.connect().await?;

    // 配置 WebSocket (writeonly 模式)
    let mut config = Config::default();
    if let Some(url) = ws_url {
        // 添加 writeonly 参数
        if url.contains('?') {
            config.ws_url = format!("{}&writeonly=1", url);
        } else {
            config.ws_url = format!("{}?writeonly=1", url);
        }
    } else {
        // 默认 URL 添加 writeonly
        config.ws_url = format!("{}?writeonly=1", config.ws_url);
    }

    // 创建客户端 (writeonly 模式)
    let client = winter_paintboard_sdk::get_global_client(config).await?;

    // 加载 Token 配置
    let token_config: multi_token::config::TokenConfig =
        serde_json::from_reader(std::fs::File::open(&config_path)?)?;

    // 处理图像
    let processed_image = image_processing::process_image_at_all_scales(
        &image,
        width,
        height,
        x,
        y,
        canny_low_thresh,
        canny_high_thresh,
    )?;

    // 设置感兴趣区域（优化同步性能）
    let interest_pixels: Vec<winter_paintboard_sdk::models::Pos> = processed_image
        .full_scale_operations
        .iter()
        .map(|(pos, _)| *pos)
        .collect();
    local_board.set_interest_pixels(interest_pixels).await;

    // 创建并启动绘制服务
    let mut service = multi_token::MultiTokenService::with_board(
        token_config,
        processed_image.clone(),
        x,
        y,
        local_board.clone(),
        Duration::from_millis(comparison_interval),
        batch_size,
        client,
    )
    .await?;

    service.start().await?;

    let stop_signal = service.stop_signal();
    let pixel_queue = service.pixel_queue();

    // 运行比对和绘制循环
    info!("绘制 Worker 已就绪, 按 Ctrl+C 停止...");

    tokio::select! {
        _ = multi_token::MultiTokenService::run_comparison_loop(
            pixel_queue,
            local_board,
            processed_image,
            x,
            y,
            tokio::time::Duration::from_millis(comparison_interval),
            stop_signal,
        ) => {},
        _ = tokio::signal::ctrl_c() => {
            info!("接收到停止信号, 正在关闭...");
        }
    }

    service.stop().await?;

    Ok(())
}
