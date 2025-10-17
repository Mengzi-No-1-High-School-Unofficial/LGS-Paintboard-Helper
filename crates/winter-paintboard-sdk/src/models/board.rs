//! `board` 模块定义了画板及其像素的数据结构和相关操作。

use crate::error::PaintboardError;

/// 表示一个具有 RGB 颜色值的单一像素。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    /// 红色分量 (0-255)。
    pub r: u8,
    /// 绿色分量 (0-255)。
    pub g: u8,
    /// 蓝色分量 (0-255)。
    pub b: u8,
}

impl Pixel {
    /// 创建一个新的 `Pixel` 实例。
    ///
    /// # 参数
    /// - `r`: 红色分量。
    /// - `g`: 绿色分量。
    /// - `b`: 蓝色分量。
    ///
    /// # 返回
    /// 一个新的 `Pixel` 实例。
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// 画板的表示，通常是 1000x600 像素。
/// 像素数据以 `[r, g, b, r, g, b, ...]` 的形式存储在一个 `Vec<u8>` 中。
#[derive(Debug)]
pub struct Board {
    /// 画板的宽度。
    pub width: u16,
    /// 画板的高度。
    pub height: u16,
    /// 存储像素数据的原始 RGB 字节向量。
    pub data: Vec<u8>, // RGB bytes: [r, g, b, r, g, b, ...]
}

impl Board {
    /// 创建一个全新的空画板，所有像素初始化为黑色 (0, 0, 0)。
    /// 默认尺寸为 1000x600 像素。
    ///
    /// # 返回
    /// 一个新的 `Board` 实例。
    pub fn new() -> Self {
        Self {
            width: 1000,
            height: 600,
            data: vec![0; 1000 * 600 * 3], // 1,800,000 bytes
        }
    }

    /// 从原始 RGB 字节向量创建一个画板。
    ///
    /// 传入的字节向量长度必须与画板尺寸 (1000x600x3) 匹配，否则会触发 `assert_eq!` 宏的 panic。
    ///
    /// # 参数
    /// - `bytes`: 包含画板像素数据的原始 RGB 字节向量。
    ///
    /// # 返回
    /// 一个新的 `Board` 实例。
    ///
    /// # Panics
    /// 如果 `bytes` 的长度不为 `1_800_000`，则会 panic。
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        assert_eq!(
            bytes.len(),
            1_800_000,
            "Board data must be exactly 1,800,000 bytes (1000x600x3)"
        );

        Self {
            width: 1000,
            height: 600,
            data: bytes,
        }
    }

    /// 获取画板上指定位置 (x, y) 像素的颜色。
    ///
    /// # 参数
    /// - `x`: 像素的 X 坐标。
    /// - `y`: 像素的 Y 坐标。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Pixel` 结构体，表示该位置的颜色；
    /// 失败时包含 `PaintboardError::InvalidCoordinate` (坐标越界) 或 `PaintboardError::IndexOutOfRange` (内部数据索引越界)。
    pub fn get_pixel(&self, x: u16, y: u16) -> Result<Pixel, PaintboardError> {
        if x >= self.width || y >= self.height {
            return Err(PaintboardError::invalid_coordinate(x as i32, y as i32));
        }

        let index = (y as usize * self.width as usize + x as usize) * 3;
        if index + 2 >= self.data.len() {
            return Err(PaintboardError::index_out_of_range(
                index + 2,
                self.data.len(),
            ));
        }

        Ok(Pixel {
            r: self.data[index],
            g: self.data[index + 1],
            b: self.data[index + 2],
        })
    }

    /// 设置画板上指定位置 (x, y) 像素的颜色。
    ///
    /// # 参数
    /// - `x`: 像素的 X 坐标。
    /// - `y`: 像素的 Y 坐标。
    /// - `pixel`: 要设置的 `Pixel` 颜色。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`；
    /// 失败时包含 `PaintboardError::InvalidCoordinate` (坐标越界) 或 `PaintboardError::IndexOutOfRange` (内部数据索引越界)。
    pub fn set_pixel(&mut self, x: u16, y: u16, pixel: Pixel) -> Result<(), PaintboardError> {
        if x >= self.width || y >= self.height {
            return Err(PaintboardError::invalid_coordinate(x as i32, y as i32));
        }

        let index = (y as usize * self.width as usize + x as usize) * 3;
        if index + 2 >= self.data.len() {
            return Err(PaintboardError::index_out_of_range(
                index + 2,
                self.data.len(),
            ));
        }

        self.data[index] = pixel.r;
        self.data[index + 1] = pixel.g;
        self.data[index + 2] = pixel.b;

        Ok(())
    }

    /// 获取画板的原始 RGB 字节切片。
    ///
    /// # 返回
    /// 一个指向画板内部数据 `Vec<u8>` 的切片引用。
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// 将画板转换为一个二维像素向量，方便按行和列访问像素。
    ///
    /// # 返回
    /// 一个 `Vec<Vec<Pixel>>`，其中外层 Vec 表示行，内层 Vec 表示每行中的像素。
    /// 如果 `get_pixel` 失败（例如，由于内部逻辑错误），则默认使用黑色像素 (0,0,0)。
    pub fn to_2d_pixels(&self) -> Vec<Vec<Pixel>> {
        let mut result = Vec::with_capacity(self.height as usize);
        for y in 0..self.height {
            let mut row = Vec::with_capacity(self.width as usize);
            for x in 0..self.width {
                if let Ok(pixel) = self.get_pixel(x, y) {
                    row.push(pixel);
                } else {
                    row.push(Pixel::new(0, 0, 0)); // Default to black on error
                }
            }
            result.push(row);
        }
        result
    }
}

impl Default for Board {
    /// 返回一个默认的 `Board` 实例 (即一个空的 1000x600 黑色画板)。
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_board() {
        let board = Board::new();
        assert_eq!(board.width, 1000);
        assert_eq!(board.height, 600);
        assert_eq!(board.data.len(), 1_800_000); // 1000 * 600 * 3
    }

    #[test]
    fn test_set_get_pixel() {
        let mut board = Board::new();
        let pixel = Pixel::new(255, 128, 64);

        assert!(board.set_pixel(100, 50, pixel).is_ok());
        let retrieved = board.get_pixel(100, 50).unwrap();
        assert_eq!(retrieved, pixel);
    }

    #[test]
    fn test_invalid_coordinates() {
        let board = Board::new();

        // Test X out of bounds
        assert!(board.get_pixel(1000, 0).is_err());
        assert!(board.get_pixel(1001, 0).is_err());

        // Test Y out of bounds
        assert!(board.get_pixel(0, 600).is_err());
        assert!(board.get_pixel(0, 601).is_err());
    }
}
