//! 示例：使用连接池客户端
//! 
//! 要运行此示例，请使用以下命令：
//! 
//! ```bash
//! cargo run -- --help  # 查看所有选项，包括新的 --client-type
//! ```

use winter_paintboard_sdk::{
    create_client_by_type, 
    ClientType, 
    config::Config,
    Pos, 
    Rgb
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 创建配置
    let config = Config::default();
    
    // 使用连接池客户端（最小2个连接，最大7个连接）
    let mut client = create_client_by_type(config, ClientType::ConnectionPool).await?;
    
    // 设置认证信息（需要替换为实际的uid和token）
    // client.set_auth(12345, "your_token_here".to_string());
    
    // 示例：绘制单个像素（这将使用连接池中的一个连接）
    // let result = client.paint(
    //     Pos { x: 100, y: 100 }, 
    //     Rgb { r: 255, g: 0, b: 0 }
    // ).await;
    // 
    // match result {
    //     Ok(paint_result) => println!("绘制成功: {:?}", paint_result),
    //     Err(e) => eprintln!("绘制失败: {:?}", e),
    // }
    
    println!("连接池客户端创建成功！");
    println!("当前连接池大小: {:?}", 0); // 这里需要实际方法来获取连接池大小
    
    Ok(())
}