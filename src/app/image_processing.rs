//! 图像处理模块
//!
//! 该模块负责将输入的图像文件转换为画板绘制操作序列，包括图像缩放、
//! 坐标映射和网格图算法优先级计算等功能。

use color_eyre::Report;
use image::{imageops::FilterType, open, GrayImage, RgbaImage};
use imageproc::edges::{self, canny};
use imageproc::filter::gaussian_blur_f32;
use log::{debug, info};
use rustc_hash::FxHashMap;

/// 表示处理后的图像数据，用于不同缩放级别的绘制操作
///
/// 包含原始图像尺寸、完整尺寸的绘制操作序列、多级缩放的绘制操作序列
/// 以及通过网格图算法计算出的像素优先级映射
#[derive(Clone)]
pub struct ProcessedImageData {
    /// 原始图像的宽度
    pub img_width: u32,
    /// 原始图像的高度
    pub img_height: u32,
    /// 完整尺寸的绘制操作序列，包含位置和颜色信息
    pub full_scale_operations: Vec<(winter_paintboard_sdk::Pos, winter_paintboard_sdk::Rgb)>,
    /// 不同缩放级别的绘制操作序列，用于渐进式绘制
    pub scale_level_operations: Vec<Vec<(winter_paintboard_sdk::Pos, winter_paintboard_sdk::Rgb)>>,
    /// 网格图算法计算出的像素优先级映射，优先级值越高表示越重要
    pub pixel_canny_priorities: FxHashMap<winter_paintboard_sdk::Pos, f64>,
}

/// 从指定路径读取并缩放图像
///
/// 根据可选的宽度和高度参数缩放图像，使用三角滤波器以获得平滑效果
///
/// # 参数
///
/// * `image_path` - 图像文件路径
/// * `width` - 目标宽度（可选）
/// * `height` - 目标高度（可选）
///
/// # 返回值
///
/// * `Ok(RgbaImage)` - 成功读取并缩放的RGBA图像
/// * `Err` - 读取或处理过程中发生错误
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

/// 在多个缩放级别处理图像以准备绘制操作
///
/// 将图像处理为多个缩放级别，计算网格图算法优先级，并生成对应的绘制操作序列
///
/// # 参数
///
/// * `image_path` - 输入图像文件路径
/// * `width` - 目标宽度（可选）
/// * `height` - 目标高度（可选）
/// * `start_x` - 绘制起始X坐标
/// * `start_y` - 绘制起始Y坐标
/// * `canny_low_thresh` - 网格图算法低阈值
/// * `canny_high_thresh` - 网格图算法高阈值
///
/// # 返回值
///
/// * `Ok(ProcessedImageData)` - 处理后的图像数据结构
/// * `Err` - 处理过程中发生错误
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

/// 将RGBA图像转换为绘制操作序列，包含坐标映射
///
/// 遍历图像的每个像素，将其转换为画板位置和颜色的元组，
/// 同时处理透明度和边界检查
///
/// # 参数
///
/// * `rgba_img` - 输入的RGBA图像
/// * `start_x` - 绘制起始X坐标
/// * `start_y` - 绘制起始Y坐标
///
/// # 返回值
///
/// * `Ok(Vec<(Pos, Rgb)>)` - 绘制操作序列
/// * `Err` - 处理过程中发生错误
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

/// 对图像应用网格图算法并返回像素位置到优先级值的映射
///
/// 将输入图像转换为灰度图，应用网格图算法检测边缘，并将边缘强度作为像素优先级
///
/// # 参数
///
/// * `rgba_img` - 输入的RGBA图像
/// * `low_thresh` - 网格图算法的低阈值
/// * `high_thresh` - 网格图算法的高阈值
///
/// # 返回值
///
/// 返回一个映射，键为画板位置，值为对应的优先级强度值
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
