use crate::models::{Rgb, Pos, OpCode};
use serde::{Deserialize, Serialize};

/// Result of a paint operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintResult {
    pub status: PaintStatus,
    pub message: String,
}

/// Status of a paint operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaintStatus {
    Success,
    Cooldown,
    InvalidToken,
    InvalidCoordinate,
    RateLimit,
    ServerError,
    Unknown,
}

impl From<u8> for PaintStatus {
    fn from(value: u8) -> Self {
        match value {
            0xef => PaintStatus::Success,
            0xee => PaintStatus::Cooldown,
            0xed => PaintStatus::InvalidToken,
            0xec => PaintStatus::InvalidCoordinate,
            0xeb => PaintStatus::RateLimit,
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
    pub uid: u32,
    pub token: String,
    pub paint_id: Option<u64>,
}

impl PaintOperation {
    /// Convert the paint operation to binary format for WebSocket transmission
    pub fn to_binary(&self) -> Vec<u8> {
        let mut data = vec![OpCode::Paint as u8]; // 0xfe
        data.extend_from_slice(&self.pos.to_bytes());
        data.extend_from_slice(&self.color.to_bytes());
        data.extend_from_slice(&self.uid.to_le_bytes());
        data.extend(self.token.as_bytes());
        if let Some(id) = self.paint_id {
            data.extend_from_slice(&id.to_le_bytes());
        }
        data
    }
}