// 模块: src/app/drawing/progressive_mode.rs
//! 渐进式绘制模式枚举与解析

#[derive(Debug)]
pub enum ProgressiveMode {
    None,
    Chessboard,
    Scale,
}

impl ProgressiveMode {
    pub fn from_string(s: &str) -> Self {
        match s {
            "chessboard" => ProgressiveMode::Chessboard,
            "scale" => ProgressiveMode::Scale,
            _ => ProgressiveMode::None,
        }
    }
}
