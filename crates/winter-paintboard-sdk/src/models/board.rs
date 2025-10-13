use crate::error::PaintboardError;

/// A single pixel with RGB values
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Pixel {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// The paintboard representation (1000x600 pixels)
#[derive(Debug)]
pub struct Board {
    pub width: u16,
    pub height: u16,
    pub data: Vec<u8>, // RGB bytes: [r, g, b, r, g, b, ...]
}

impl Board {
    /// Create a new empty board
    pub fn new() -> Self {
        Self {
            width: 1000,
            height: 600,
            data: vec![0; 1000 * 600 * 3], // 1,800,000 bytes
        }
    }
    
    /// Create a board from raw RGB bytes
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        assert_eq!(bytes.len(), 1_800_000, "Board data must be exactly 1,800,000 bytes (1000x600x3)");
        
        Self {
            width: 1000,
            height: 600,
            data: bytes,
        }
    }
    
    /// Get the color of a pixel at position (x, y)
    pub fn get_pixel(&self, x: u16, y: u16) -> Result<Pixel, PaintboardError> {
        if x >= self.width || y >= self.height {
            return Err(PaintboardError::InvalidCoordinate);
        }
        
        let index = (y as usize * self.width as usize + x as usize) * 3;
        if index + 2 >= self.data.len() {
            return Err(PaintboardError::IndexOutOfRange);
        }
        
        Ok(Pixel {
            r: self.data[index],
            g: self.data[index + 1],
            b: self.data[index + 2],
        })
    }
    
    /// Set the color of a pixel at position (x, y)
    pub fn set_pixel(&mut self, x: u16, y: u16, pixel: Pixel) -> Result<(), PaintboardError> {
        if x >= self.width || y >= self.height {
            return Err(PaintboardError::InvalidCoordinate);
        }
        
        let index = (y as usize * self.width as usize + x as usize) * 3;
        if index + 2 >= self.data.len() {
            return Err(PaintboardError::IndexOutOfRange);
        }
        
        self.data[index] = pixel.r;
        self.data[index + 1] = pixel.g;
        self.data[index + 2] = pixel.b;
        
        Ok(())
    }
    
    /// Get raw RGB bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
    
    /// Convert to 2D vector of pixels (for easier access)
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