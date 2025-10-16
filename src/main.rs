//! Main entry point for the Winter Paintboard Helper application
//! 
//! This application is a command-line tool for drawing images on the Luogu Paintboard.
//! It supports multiple drawing modes including incremental, loop, and one-time drawing.

use clap::Parser;
use tracing_subscriber;
use tracing::info;

/// Application module containing all the core functionality
mod app;

#[tokio::main]
/// Main async function - entry point of the application
///
/// Initializes the tracing subscriber, parses command-line arguments, and starts the application
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber with environment filter (controlled by RUST_LOG)
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Parse command line arguments using clap
    let cli = app::cli::Cli::parse();

    info!("启动绘板应用...");

    // Run the main application logic
    app::run_app(cli).await
}