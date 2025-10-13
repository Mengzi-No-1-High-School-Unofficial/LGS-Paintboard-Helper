use clap::Parser;
use image::{open, RgbaImage};
use std::path::PathBuf;
use winter_paintboard_sdk::{PaintboardClient, Rgb, Pos, config::Config};
use log::{debug, error, info, warn};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// 认证 Token (与 --access-key 二选一)
    #[arg(short, long, requires = "uid")]
    token: Option<String>,

    /// 用户ID (必需，用于直接提供 token 或与 access_key 配合)
    #[arg(long)]
    uid: u32,

    /// 访问密钥 (与 --token 二选一)
    #[arg(long, requires = "uid")]
    access_key: Option<String>,

    /// WebSocket端点URL (可选，默认为官方端点)
    #[arg(long)]
    ws_url: Option<String>,

    /// 要绘制的PNG图片路径
    #[arg(short, long)]
    image: PathBuf,

    /// 图片在画板上的起始X坐标 (可选，默认为0)
    #[arg(short, long, default_value_t = 0)]
    x: i32,

    /// 图片在画板上的起始Y坐标 (可选，默认为0)
    #[arg(short, long, default_value_t = 0)]
    y: i32,

    /// 图片绘制时的宽度 (可选，默认为原图宽度)
    #[arg(long)]
    width: Option<u32>,

    /// 图片绘制时的高度 (可选，默认为原图高度)
    #[arg(long)]
    height: Option<u32>,
    
    /// 绘制每个像素之间的延迟（毫秒）(可选，默认为10毫秒，减少此值会增加发送速率)
    #[arg(long, default_value_t = 10)]
    delay: u64,
    
    /// 使用批量绘制模式（粘包发送）(可选，默认为启用)
    #[arg(long, default_value_t = true)]
    batch_mode: bool,
    
    /// 批量模式下每次发送的最大像素数量 (可选，默认为50)
    #[arg(long, default_value_t = 50)]
    max_batch_size: usize,
    
    /// 是否循环绘制图片 (可选，默认为禁用)
    #[arg(long, default_value_t = false)]
    loop_draw: bool,
    
    /// 循环绘制的时间间隔（毫秒）(可选，默认为60000毫秒，即60秒)
    #[arg(long, default_value_t = 60000)]
    loop_interval: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the logger with env_logger
    env_logger::init();
    
    let cli = Cli::parse();

    // 循环绘制图片的函数
    async fn draw_image(cli: &Cli, token: &str) -> Result<(), Box<dyn std::error::Error>> {
        // 1. 读取并解码PNG图片
        info!("正在读取图片: {:?}", cli.image);
        let mut img = open(&cli.image)?;
        
        // 2. 根据用户指定的尺寸进行缩放
        let target_width = cli.width.unwrap_or_else(|| img.width());
        let target_height = cli.height.unwrap_or_else(|| img.height());
        
        if target_width != img.width() || target_height != img.height() {
            info!("正在缩放图片从 {}x{} 到 {}x{}", img.width(), img.height(), target_width, target_height);
            img = img.resize_exact(target_width, target_height, image::imageops::Triangle);
        }

        let rgba_img: RgbaImage = img.into_rgba8();
        let (img_width, img_height) = rgba_img.dimensions();

        // 3. 初始化绘板客户端配置
        info!("正在初始化绘板客户端...");
        let mut config = Config::default(); // 使用默认配置
        // 确保使用正确的WebSocket端点
        config.ws_url = cli.ws_url.clone().unwrap_or_else(|| "wss://paintboard.luogu.me/api/paintboard/ws".to_string());
        let mut client = PaintboardClient::new(config).await?; // 使用新的API
        
        // 设置认证信息
        client.set_auth(cli.uid, token.to_string());

        // 4. 将图片像素数据转换为绘板绘制请求
        info!("正在准备绘制数据...");
        let mut draw_operations = Vec::new();
        for (x, y, pixel) in rgba_img.enumerate_pixels() {
            // 获取像素的RGBA值
            let [r, g, b, a] = pixel.0;
            
            // 如果Alpha值为0，则跳过该像素（透明区域不绘制）
            if a == 0 {
                continue;
            }

            // 计算在画板上的实际坐标
            let board_x = (cli.x as u32).saturating_add(x);
            let board_y = (cli.y as u32).saturating_add(y);

            // 将坐标转换为有效的u16值（0-999 for x, 0-599 for y） 
            if board_x >= 1000 || board_y >= 600 {
                warn!("坐标({}, {})超出了画板边界，将被忽略", board_x, board_y);
                continue;
            }
            
            // 创建位置和颜色对象
            let pos = Pos::new(board_x as u16, board_y as u16)?;
            let color = Rgb::new(r, g, b);

            draw_operations.push((pos, color));
        }
        
        info!("总共准备了 {} 个绘制操作", draw_operations.len());

        // 5. 调用SDK进行绘制
        info!("正在绘制图片到画板...");
        let total_pixels = draw_operations.len();
        
        if cli.batch_mode && !draw_operations.is_empty() {
            // 使用批量绘制模式（粘包机制），支持分批，不等待响应
            info!("使用批量绘制模式，最大批量大小: {}, 总共 {} 个像素...", cli.max_batch_size, total_pixels);
            
            let mut processed = 0;
            
            // 按最大批量大小分批处理
            for chunk in draw_operations.chunks(cli.max_batch_size) {
                match client.paint_batch(chunk.to_vec()).await {
                    Ok(()) => {
                        processed += chunk.len();
                        debug!("批次发送完成，已发送: {}/{} 像素", processed, total_pixels);
                        
                        // 在批次之间添加延迟以避免速率限制
                        tokio::time::sleep(tokio::time::Duration::from_millis(cli.delay)).await;
                    }
                    Err(e) => {
                        error!("批量绘制错误: {:?}", e);
                        // 继续处理下一个批次，而不是中断
                    }
                }
            }
            
            info!("批量绘制发送完成，总共发送了 {} 个像素（不等待响应确认）", processed);
        } else if !cli.batch_mode {
            // 使用逐个绘制模式
            let mut successful_draws = 0;
            let mut failed_draws = 0;
            
            for (i, (pos, color)) in draw_operations.into_iter().enumerate() {
                match client.paint(pos, color).await {
                    Ok(result) => {
                        match result.status {
                            winter_paintboard_sdk::models::PaintStatus::Success => {
                                successful_draws += 1;
                                debug!("绘制成功 (位置: {},{}) - Drawing ID: {}", pos.x, pos.y, result.drawing_id);
                            },
                            winter_paintboard_sdk::models::PaintStatus::Cooldown => {
                                warn!("绘制冷却中，稍等... (位置: {},{}) - Drawing ID: {}", pos.x, pos.y, result.drawing_id);
                                failed_draws += 1;
                                // 等待一段时间以避免速率限制
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            },
                            winter_paintboard_sdk::models::PaintStatus::InvalidToken => {
                                error!("无效的Token: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                failed_draws += 1;
                            },
                            winter_paintboard_sdk::models::PaintStatus::NoPermission => {
                                error!("无权限绘制: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                failed_draws += 1;
                            },
                            winter_paintboard_sdk::models::PaintStatus::InvalidCoordinate => {
                                error!("无效坐标: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                failed_draws += 1;
                            },
                            winter_paintboard_sdk::models::PaintStatus::Timeout => {
                                error!("请求超时: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                failed_draws += 1;
                            },
                            _ => {
                                error!("绘制失败: {:?} (位置: {},{}) - Drawing ID: {}", result, pos.x, pos.y, result.drawing_id);
                                failed_draws += 1;
                            }
                        }
                    }
                    Err(e) => {
                        error!("绘制错误: {:?} (位置: {},{})", e, pos.x, pos.y);
                        failed_draws += 1;
                    }
                }
                
                // 添加进度显示
                if (i + 1) % 100 == 0 || i == total_pixels - 1 {
                    info!("进度: {}/{} 像素, 成功: {}, 失败: {}", i + 1, total_pixels, successful_draws, failed_draws);
                }
                
                // 为避免速率限制，添加小延迟
                if i < total_pixels - 1 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(cli.delay)).await;
                }
            }
        }

        info!("图片绘制完成！图片尺寸: {}x{}, 起始坐标: ({}, {})", img_width, img_height, cli.x, cli.y);
        Ok(())
    }

    // 获取或使用提供的 Token
    let token = if let Some(access_key) = &cli.access_key {
        // 使用 UID 和访问密钥获取新 Token
        info!("正在使用 UID 和访问密钥获取 Token...");
        let config = Config::default();
        let http_client = PaintboardClient::new(config).await?;
        let token = http_client.get_token(cli.uid, access_key).await?;
        info!("成功获取 Token: {}...", &token[..8]); // 显示开头部分
        token
    } else if let Some(provided_token) = &cli.token {
        // 直接使用提供的 Token
        provided_token.clone()
    } else {
        error!("错误: 必须提供 --token 或 --access-key");
        std::process::exit(1);
    };

    // 判断是否需要循环绘制
    if cli.loop_draw {
        info!("开始循环绘制模式，时间间隔: {} 毫秒", cli.loop_interval);
        loop {
            if let Err(e) = draw_image(&cli, &token).await {
                error!("绘制图片时发生错误: {:?}", e);
            }
            
            info!("等待 {} 毫秒后再次绘制...", cli.loop_interval);
            tokio::time::sleep(tokio::time::Duration::from_millis(cli.loop_interval)).await;
        }
    } else {
        // 单次绘制模式
        draw_image(&cli, &token).await?;
    }
    
    Ok(())
}