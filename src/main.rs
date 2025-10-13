use clap::Parser;
use env_logger;
use log::info;

mod app;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the logger with env_logger
    env_logger::init();
    
    let cli = app::cli::Cli::parse();
    
    info!("启动绘板应用...");
    
    app::run_app(cli).await
}