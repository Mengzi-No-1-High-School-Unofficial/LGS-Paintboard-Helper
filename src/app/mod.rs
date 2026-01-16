//! 应用程序主模块
//!
//! 该模块包含应用程序的核心逻辑，处理命令行参数并根据不同的子命令执行相应的功能。
//! 主要功能包括多Token绘制模式、获取画板状态和显示项目信息。

pub mod board_sync;
pub mod cli;
pub mod export;
pub mod image_processing;
pub mod multi_token;
pub mod utils;

use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use winter_paintboard_sdk::basic_client::HttpProvider;
use winter_paintboard_sdk::config::Config;

use crate::app::{
    cli::Cli,
    export::start_export_if_enabled,
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
        Commands::MultiToken {
            config,
            access_keys: _,
            uids: _,
            cd_time,
            ws_url,
            image,
            x,
            y,
            width,
            height,
            comparison_interval,
            enable_export,
            enable_heatmap_export,
            export_dir,
            export_interval,
            canny_low_thresh,
            canny_high_thresh,
            penalty_scale,
            batch_size,
        } => {
            // 多 Token 模式
            run_multi_token_mode(
                config,
                ws_url,
                image,
                x,
                y,
                width,
                height,
                cd_time,
                comparison_interval,
                enable_export,
                enable_heatmap_export,
                export_dir,
                export_interval,
                canny_low_thresh,
                canny_high_thresh,
                penalty_scale,
                batch_size,
            )
            .await
        }
    }
}

/// 运行多 Token 绘制模式
///
/// 该模式使用多个用户 Token 并发绘制图像，通过网格图算法优化绘制优先级，
/// 并支持本地画板同步、绘制结果导出等功能。
///
/// # 参数
///
/// * `config_path` - Token 配置文件路径
/// * `ws_url` - WebSocket 服务器 URL（可选）
/// * `image` - 要绘制的图像文件路径
/// * `x` - 绘制起始 X 坐标
/// * `y` - 绘制起始 Y 坐标
/// * `width` - 图像宽度（可选，用于缩放）
/// * `height` - 图像高度（可选，用于缩放）
/// * `cd_time` - Token 冷却时间（毫秒）
/// * `comparison_interval` - 画板状态比对间隔（毫秒）
/// * `enable_export` - 是否启用画板导出功能
/// * `enable_heatmap_export` - 是否启用热点图导出功能
/// * `export_dir` - 导出目录路径
/// * `export_interval` - 导出时间间隔（秒）
/// * `canny_low_thresh` - 网格图算法低阈值
/// * `canny_high_thresh` - 网格图算法高阈值
/// * `penalty_scale` - 惩罚系数，用于避免重复绘制同一位置
///
/// # 返回值
///
/// * `Ok(())` - 多 Token 模式正常执行完成
/// * `Err` - 执行过程中发生错误
#[allow(clippy::too_many_arguments)]
pub async fn run_multi_token_mode(
    config_path: PathBuf,
    ws_url: Option<String>,
    image: PathBuf,
    x: i32,
    y: i32,
    width: Option<u32>,
    height: Option<u32>,
    _cd_time: u64,
    comparison_interval: u64,
    enable_export: bool,
    enable_heatmap_export: bool,
    export_dir: String,
    export_interval: u64,
    canny_low_thresh: f32,
    canny_high_thresh: f32,
    penalty_scale: f32,
    batch_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::app::board_sync::BoardSyncManager;
    use crate::app::image_processing::process_image_at_all_scales;
    use crate::app::multi_token::multi_token_service::MultiTokenService;
    use winter_paintboard_sdk::{config::Config, get_global_client, PaintboardClientTrait};

    info!("启动多 Token 绘制模式...");

    let _ = multi_token::cli::PENALTY_SENSITIVITY
        .set(penalty_scale)
        .map_err(|_e| {
            let e = color_eyre::Report::msg("无法设置值 PENALTY_SENSITIVITY");
            error!("{}", e);
            e
        });

    // 加载配置
    let token_config = crate::app::multi_token::config::TokenConfig::from_file(&config_path)?;

    // 创建同步管理器
    let sync_manager = BoardSyncManager::new();

    // 为同步任务创建新的客户端
    let mut sync_config = Config::default();
    if let Some(url) = &ws_url {
        sync_config.ws_url = url.clone();
    }

    let sync_client: Arc<dyn PaintboardClientTrait + Send + Sync> =
        get_global_client(sync_config).await?;

    // 处理图片
    info!("正在预处理图片数据...");
    let processed_image_data = process_image_at_all_scales(
        &image,
        width,
        height,
        x,
        y,
        canny_low_thresh,
        canny_high_thresh,
    )?;

    // 使用第一个 token 的认证信息
    if let Some(first_token) = token_config.tokens.first() {
        let _token = match (&first_token.token, &first_token.access_key) {
            (Some(t), _) => t.clone(),
            (None, Some(ak)) => get_token_with_access_key(first_token.uid, ak).await?,
            _ => {
                error!("配置文件中的第一个 Token 条目缺少 token 或 access_key");
                return Err("无效的 Token 配置".into());
            }
        };
    }

    // 启动增量同步循环
    sync_manager
        .start_sync_loop(
            sync_client,
            tokio::time::Duration::from_millis(std::cmp::max(comparison_interval, 2500)),
        )
        .await?;

    info!("本地绘版数据同步已启动");

    // 启动绘版图片导出服务（如果启用）
    start_export_if_enabled(
        &sync_manager,
        enable_export,
        export_dir.clone(),
        export_interval,
    )
    .await?;

    // 启动热点图导出服务（如果启用）
    crate::app::export::start_heatmap_export_if_enabled(
        &sync_manager,
        enable_heatmap_export,
        export_dir.clone(),
        export_interval,
    )
    .await?;

    // 创建并启动多 Token 服务
    let mut service = MultiTokenService::with_canny_thresholds(
        token_config,
        ws_url,
        sync_manager.local_board(),
        processed_image_data,
        x,
        y,
        tokio::time::Duration::from_millis(comparison_interval),
        canny_low_thresh,
        canny_high_thresh,
        batch_size,
    )
    .await?;

    service.start().await?;

    info!("多 Token 模式已启动，按 Ctrl+C 停止...");
    tokio::signal::ctrl_c().await?;

    // 清理
    service.stop().await?;

    Ok(())
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
