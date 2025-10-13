use crate::models::{Rgb, Pos, OpCode};
use serde::{Deserialize, Serialize};

/// Result of a paint operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintResult {
    pub drawing_id: u32,
    pub status: PaintStatus,
    pub message: String,
}

/// Status of a paint operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaintStatus {
    Success,
    Cooldown,
    InvalidToken,
    InvalidCoordinate,  // Changed from RequestFormatError to match the protocol
    NoPermission,
    ServerError,
    Timeout,
    Unknown,
}

impl From<u8> for PaintStatus {
    fn from(value: u8) -> Self {
        match value {
            0xef => PaintStatus::Success,
            0xee => PaintStatus::Cooldown,
            0xed => PaintStatus::InvalidToken,
            0xec => PaintStatus::InvalidCoordinate,  // Request format error
            0xeb => PaintStatus::NoPermission,       // No permission
            0xea => PaintStatus::ServerError,
            _ => PaintStatus::Unknown,
        }
    }
}

/// A paint operation to be sent to the server
#[derive(Debug, Clone)]
pub struct PaintOperation {
    pub pos: Pos,
    pub color: Rgb,
    pub token_uid: u32,  // The UID part of the token (will be split into 3 bytes)
    pub token: String,   // The 16-byte token as string
    pub paint_id: u32,   // Changed from u64 to u32 to match protocol
}

impl PaintOperation {
    /// Convert the paint operation to binary format for WebSocket transmission
    pub fn to_binary(&self) -> Vec<u8> {
        // First 8 bytes: opcode + position + color
        let mut data = vec![OpCode::Paint as u8]; // 0xfe
        data.extend_from_slice(&self.pos.x.to_le_bytes());  // X coordinate (uint16)
        data.extend_from_slice(&self.pos.y.to_le_bytes());  // Y coordinate (uint16)
        data.extend_from_slice(&[self.color.r, self.color.g, self.color.b]); // RGB values
        
        // Next 21 bytes: 3-byte token UID + 16-byte token + 4-byte drawing ID
        // Split UID into 3 bytes according to protocol (as mentioned in the doc)
        // We'll use the lower 3 bytes of the u32 UID
        let uid_bytes = self.token_uid.to_le_bytes();
        data.extend_from_slice(&uid_bytes[0..3]);  // Take first 3 bytes of UID
        
        // Add 16-byte token (assuming it's a UUID string that should be 16 bytes when parsed)
        let mut token_bytes = self.token.as_bytes().to_vec();
        if token_bytes.len() > 16 {
            token_bytes.truncate(16);
        } else if token_bytes.len() < 16 {
            token_bytes.resize(16, 0); // Pad with zeros if needed
        }
        data.extend_from_slice(&token_bytes);
        
        // Add 4-byte drawing ID
        data.extend_from_slice(&self.paint_id.to_le_bytes());
        
        data
    }
}