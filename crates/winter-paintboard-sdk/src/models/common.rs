use crate::error::PaintboardError;

/// RGB color representation
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Create a new RGB color
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
    
    /// Convert RGB to bytes in RGB order
    pub fn to_bytes(&self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
    
    /// Create RGB from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PaintboardError> {
        if bytes.len() < 3 {
            return Err(PaintboardError::InvalidData);
        }
        Ok(Self {
            r: bytes[0],
            g: bytes[1],
            b: bytes[2],
        })
    }
}

/// Position on the paintboard
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pos {
    pub x: u16,  // 0-999
    pub y: u16,  // 0-599
}

impl Pos {
    /// Create a new position with validation
    pub fn new(x: u16, y: u16) -> Result<Self, PaintboardError> {
        if x >= 1000 || y >= 600 {
            return Err(PaintboardError::InvalidCoordinate);
        }
        Ok(Self { x, y })
    }
    
    /// Convert position to little-endian bytes
    pub fn to_bytes(&self) -> [u8; 4] {
        let mut bytes = [0u8; 4];
        bytes[0..2].copy_from_slice(&self.x.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.y.to_le_bytes());
        bytes
    }
    
    /// Create position from little-endian bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PaintboardError> {
        if bytes.len() < 4 {
            return Err(PaintboardError::InvalidData);
        }
        let x = u16::from_le_bytes([bytes[0], bytes[1]]);
        let y = u16::from_le_bytes([bytes[2], bytes[3]]);
        Self::new(x, y)  // This will validate the coordinates
    }
}