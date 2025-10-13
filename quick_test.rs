use winter_paintboard_sdk::{PaintboardClient, Rgb, Pos, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("测试 WebSocket 连接建立...");
    
    let mut config = Config::default();
    println!("WebSocket URL: {}", config.ws_url);
    
    let mut client = PaintboardClient::new(config).await?;
    
    // 这里我们不实际连接，只是测试配置
    println!("客户端创建成功，配置已加载");
    
    Ok(())
}use winter_paintboard_sdk::{PaintboardClient, Rgb, Pos, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    log::debug!("测试 WebSocket 连接建立...");
    
    let mut config = Config::default();
    log::debug!("WebSocket URL: {}", config.ws_url);
    
    let mut client = PaintboardClient::new(config).await?;
    
    // 这里我们不实际连接，只是测试配置
    log::debug!("客户端创建成功，配置已加载");
    
    Ok(())
}