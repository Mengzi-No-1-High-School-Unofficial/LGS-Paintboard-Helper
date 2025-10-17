// 模块: src/app/incremental/pixel_comparison.rs
//! 颜色差异计算工具

use winter_paintboard_sdk::Rgb;

/// 计算两个 RGB 颜色之间的差异（欧几里得距离）
pub fn calculate_color_difference(color1: &Rgb, color2: &Rgb) -> f64 {
    let dr = (color1.r as i32 - color2.r as i32) as f64;
    let dg = (color1.g as i32 - color2.g as i32) as f64;
    let db = (color1.b as i32 - color2.b as i32) as f64;

    ((dr * dr + dg * dg + db * db) / 3.0).sqrt()
}
