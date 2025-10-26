use color_eyre::Report;
use image::{imageops::FilterType, open, GrayImage, RgbaImage};
use imageproc::edges::{self, canny};
use imageproc::filter::gaussian_blur_f32;
use log::{debug, info};
use rustc_hash::FxHashMap;

/// Represents processed image data for different scale factors
#[derive(Clone)]
pub struct ProcessedImageData {
    pub img_width: u32,
    pub img_height: u32,
    pub full_scale_operations: Vec<(winter_paintboard_sdk::Pos, winter_paintboard_sdk::Rgb)>,
    pub scale_level_operations: Vec<Vec<(winter_paintboard_sdk::Pos, winter_paintboard_sdk::Rgb)>>,
    /// Canny 边缘检测计算出的像素优先级 (边缘高优先级)
    pub pixel_canny_priorities: FxHashMap<winter_paintboard_sdk::Pos, f64>,
}

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
        info!(
            "正在缩放图片从 {}x{} 到 {}x{}",
            img.width(),
            img.height(),
            target_width,
            target_height
        );
        img = img.resize_exact(target_width, target_height, FilterType::Triangle);
    }

    Ok(img.into_rgba8())
}

/// Processes an image at multiple scale factors to prepare drawing operations
pub fn process_image_at_all_scales(
    image_path: &std::path::Path,
    width: Option<u32>,
    height: Option<u32>,
    start_x: i32,
    start_y: i32,
    canny_low_thresh: f32,
    canny_high_thresh: f32,
) -> Result<ProcessedImageData, Box<dyn std::error::Error>> {
    info!("正在读取并预处理图片: {:?}", image_path);
    let original_rgba = read_and_resize_image(image_path, width, height)?;
    let (img_width, img_height) = original_rgba.dimensions();

    // Prepare full scale operations (1.0 factor)
    let full_scale_operations =
        prepare_draw_operations_with_coords(&original_rgba, start_x, start_y)?;

    // Prepare scale level operations for scale progressive mode
    let scale_factors = vec![0.25, 0.5];
    let mut scale_level_operations = Vec::new();

    for &factor in &scale_factors {
        info!(
            "正在处理缩放级别: {} ({}x{})",
            factor,
            (img_width as f32 * factor) as u32,
            (img_height as f32 * factor) as u32
        );

        // Scale the image to the current factor
        let level_width = (img_width as f32 * factor) as u32;
        let level_height = (img_height as f32 * factor) as u32;

        if level_width == 0 || level_height == 0 {
            info!("Scale factor {} results in 0 size, skipping", factor);
            scale_level_operations.push(Vec::new());
            continue;
        }

        let level_img = image::imageops::resize(
            &original_rgba,
            level_width,
            level_height,
            FilterType::Triangle,
        );
        let level_operations = prepare_draw_operations_with_coords(&level_img, start_x, start_y)?;
        scale_level_operations.push(level_operations);
    }

    // Apply Canny edge detection to the original image
    let pixel_canny_priorities =
        apply_canny_edge_detection(&original_rgba, canny_low_thresh, canny_high_thresh);

    info!("图片预处理完成！原图尺寸: {}x{}", img_width, img_height);

    Ok(ProcessedImageData {
        img_width,
        img_height,
        full_scale_operations,
        scale_level_operations,
        pixel_canny_priorities,
    })
}

/// Prepares draw operations from an RGBA image with coordinates mapping
fn prepare_draw_operations_with_coords(
    rgba_img: &RgbaImage,
    start_x: i32,
    start_y: i32,
) -> Result<Vec<(winter_paintboard_sdk::Pos, winter_paintboard_sdk::Rgb)>, Box<dyn std::error::Error>>
{
    let mut draw_operations = Vec::new();
    let (_img_width, _img_height) = rgba_img.dimensions(); // Use underscore prefix for unused variables

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
            debug!("坐标({}, {})超出了画板边界，将被忽略", board_x, board_y);
            continue;
        }

        // 创建位置和颜色对象
        let pos = winter_paintboard_sdk::Pos::new(board_x as u16, board_y as u16)?;
        let color = winter_paintboard_sdk::Rgb::new(r, g, b);

        draw_operations.push((pos, color));
    }

    Ok(draw_operations)
}

/// Applies Canny edge detection to an image and returns a map of pixel positions to their edge strength (priority).
///
/// # Arguments
///
/// * `rgba_img` - The input RGBA image.
/// * `low_thresh` - The low threshold for the hysteresis procedure in Canny.
/// * `high_thresh` - The high threshold for the hysteresis procedure in Canny.
///
/// # Returns
///
/// A `FxHashMap` where keys are `winter_paintboard_sdk::Pos` and values are `f64` representing the edge strength.
pub fn apply_canny_edge_detection(
    rgba_img: &RgbaImage,
    low_thresh: f32,
    high_thresh: f32,
) -> FxHashMap<winter_paintboard_sdk::Pos, f64> {
    // Convert RGBA image to grayscale
    let gray_img: GrayImage = image::imageops::colorops::grayscale(rgba_img);

    // Apply Canny edge detection
    let edge_img = canny(&gray_img, low_thresh, high_thresh);
    let edge_img = gaussian_blur_f32(&edge_img, 3.0);

    let mut canny_map = FxHashMap::default();

    let _ = edge_img
        .save("./canny.png")
        .map_err(|e| tracing::error!("{}", Report::new(e).wrap_err("保存 Canny 处理结果失败")));

    // Iterate over the edge-detected image to extract edge strengths
    for (x, y, pixel) in edge_img.enumerate_pixels() {
        // The pixel value from the canny output represents the edge strength (0 for non-edge, >0 for edge)
        let edge_strength = pixel.0[0] as f64;
        if edge_strength > 0.0 {
            // Create position, handling potential errors from Pos::new
            if let Ok(pos) = winter_paintboard_sdk::Pos::new(x as u16, y as u16) {
                canny_map.insert(pos, edge_strength);
                // println!("Canny edge at ({}, {}) with strength {}", x, y, edge_strength);
            }
        }
    }

    canny_map
}
