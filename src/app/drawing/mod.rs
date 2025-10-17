// 模块: src/app/drawing
//! 子模块组织：progressive_mode, drawing_strategies, client_utils

pub mod client_utils;
pub mod drawing_strategies;
pub mod progressive_mode;

pub use client_utils::create_client;
pub use drawing_strategies::draw_image_to_paintboard_with_client;
pub use progressive_mode::ProgressiveMode;
