//! `common` 模块定义了在 Winter Paintboard SDK 中通用的数据结构，如 RGB 颜色和位置坐标。

use crate::error::PaintboardError;

/// RGB 颜色表示。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    /// 红色分量 (0-255)。
    pub r: u8,
    /// 绿色分量 (0-255)。
    pub g: u8,
    /// 蓝色分量 (0-255)。
    pub b: u8,
}

impl Rgb {
    /// 创建一个新的 `Rgb` 颜色实例。
    ///
    /// # 参数
    /// - `r`: 红色分量。
    /// - `g`: 绿色分量。
    /// - `b`: 蓝色分量。
    ///
    /// # 返回
    /// 一个新的 `Rgb` 实例。
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    
    /// 将 `Rgb` 颜色转换为 RGB 顺序的字节数组。
    ///
    /// # 返回
    /// 一个包含红、绿、蓝分量的 `[u8; 3]` 数组。
    pub fn to_bytes(&self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
    
    /// 从字节切片创建 `Rgb` 颜色。
    ///
    /// 要求字节切片至少包含 3 个字节。
    ///
    /// # 参数
    /// - `bytes`: 包含 RGB 颜色数据的字节切片。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Rgb` 实例；
    /// 失败时包含 `PaintboardError::InvalidData`。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PaintboardError> {
        if bytes.len() < 3 {
            return Err(PaintboardError::invalid_data("Not enough bytes for RGB color"));
        }
        Ok(Self {
            r: bytes[0],
            g: bytes[1],
            b: bytes[2],
        })
    }
}

/// 画板上的位置坐标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    /// X 坐标 (0-999)。
    pub x: u16,
    /// Y 坐标 (0-599)。
    pub y: u16,
}

impl Pos {
    /// 创建一个带坐标验证的新 `Pos` 实例。
    ///
    /// 坐标 `x` 必须在 [0, 999] 范围内，`y` 必须在 [0, 599] 范围内。
    ///
    /// # 参数
    /// - `x`: X 坐标。
    /// - `y`: Y 坐标。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Pos` 实例；
    /// 失败时包含 `PaintboardError::InvalidCoordinate`。
    pub fn new(x: u16, y: u16) -> Result<Self, PaintboardError> {
        if x >= 1000 || y >= 600 {
            return Err(PaintboardError::invalid_coordinate(x as i32, y as i32));
        }
        Ok(Self { x, y })
    }
    
    /// 将 `Pos` 坐标转换为小端字节数组。
    ///
    /// 格式为 `[x_byte_0, x_byte_1, y_byte_0, y_byte_1]`。
    ///
    /// # 返回
    /// 一个包含 X 和 Y 坐标的小端字节表示的 `[u8; 4]` 数组。
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.x.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.y.to_le_bytes());
        bytes
    }
    
    /// 从小端字节切片创建 `Pos` 坐标。
    ///
    /// 要求字节切片至少包含 4 个字节。
    ///
    /// # 参数
    /// - `bytes`: 包含位置数据的小端字节切片。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Pos` 实例；
    /// 失败时包含 `PaintboardError::InvalidData` 或 `PaintboardError::InvalidCoordinate`。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PaintboardError> {
        if bytes.len() < 4 {
            return Err(PaintboardError::invalid_data("Not enough bytes for position"));
        }
        let x = u16::from_le_bytes([bytes[0], bytes[1]]);
        let y = u16::from_le_bytes([bytes[2], bytes[3]]);
        Self::new(x, y)  // This will validate the coordinates
    }
}