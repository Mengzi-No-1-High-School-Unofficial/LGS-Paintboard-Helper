//! 主入口点模块
//!
//! 冬日绘板助手应用程序的主入口点，这是一个用于在洛谷绘板上绘制图像的命令行工具。
//! 支持多种绘制模式，包括增量绘制、循环绘制和单次绘制。

use clap::Parser;
use tracing::info;
// use tracing_subscriber; // 已在全局初始化中处理

/// 应用程序模块，包含所有核心功能
mod app;

#[tokio::main]
/// 主异步函数 - 应用程序的入口点
///
/// 初始化跟踪订阅者，解析命令行参数，并启动应用程序
async fn main() -> color_eyre::Result<()> {
    // Initialize color-eyre for better error reporting
    color_eyre::install()?;

    // Initialize tracing subscriber with environment filter (controlled by RUST_LOG)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // console_subscriber::init();

    // Parse command line arguments using clap
    let cli = app::cli::Cli::parse();

    info!("启动绘板应用...");

    // Run the main application logic
    app::run_app(cli).await.unwrap();

    Ok(())
}
