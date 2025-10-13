use image::{open, RgbaImage, imageops::FilterType};
use log::{info, warn};

/// Reads and resizes an image from the given path
pub fn read_and_resize_image(
    image_path: &std::path::Path,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<RgbaImage, Box<dyn std::error::Error>> {
    info!("正在读取图片: {:?}", image_path);
    let mut img = open(image_path)?;
    
    // 根据用户指定的尺寸进行缩放
    let target_width = width.unwrap_or_else(|| img.width());
    let target_height = height.unwrap_or_else(|| img.height());
    
    if target_width != img.width() || target_height != img.height() {
        info!("正在缩放图片从 {}x{} 到 {}x{}", img.width(), img.height(), target_width, target_height);
        img = img.resize_exact(target_width, target_height, FilterType::Triangle);
    }

    Ok(img.into_rgba8())
}

/// Prepares draw operations from an RGBA image
pub fn prepare_draw_operations(
    rgba_img: &RgbaImage,
    start_x: i32,
    start_y: i32,
) -> Result<Vec<(winter_paintboard_sdk::Pos, winter_paintboard_sdk::Rgb)>, Box<dyn std::error::Error>> {
    let mut draw_operations = Vec::new();
    for (x, y, pixel) in rgba_img.enumerate_pixels() {
        // 获取像素的RGBA值
        let [r, g, b, a] = pixel.0;
        
        // 如果Alpha值为0，则跳过该像素（透明区域不绘制）
        if a == 0 {
            continue;
        }

        // 计算在画板上的实际坐标
        let board_x = (start_x as u32).saturating_add(x);
        let board_y = (start_y as u32).saturating_add(y);

        // 将坐标转换为有效的u16值（0-999 for x, 0-599 for y） 
        if board_x >= 1000 || board_y >= 600 {
            warn!("坐标({}, {})超出了画板边界，将被忽略", board_x, board_y);
            continue;
        }
        
        // 创建位置和颜色对象
        let pos = winter_paintboard_sdk::Pos::new(board_x as u16, board_y as u16)?;
        let color = winter_paintboard_sdk::Rgb::new(r, g, b);

        draw_operations.push((pos, color));
    }
    
    Ok(draw_operations)
}