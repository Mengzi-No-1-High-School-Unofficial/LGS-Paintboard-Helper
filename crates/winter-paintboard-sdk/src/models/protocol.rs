use crate::{error::PaintboardError, models::{Rgb, Pos}};

/// WebSocket protocol operation codes
#[repr(u8)]
pub enum OpCode {
    HeartbeatPing = 0xfc,    // Server to client
    PaintEvent = 0xfa,       // Server to client
    PaintResult = 0xff,      // Server to client
    HeartbeatPong = 0xfb,    // Client to server
    Paint = 0xfe,            // Client to server
}

impl From<u8> for OpCode {
    fn from(value: u8) -> Self {
        match value {
            0xfc => OpCode::HeartbeatPing,
            0xfa => OpCode::PaintEvent,
            0xff => OpCode::PaintResult,
            0xfb => OpCode::HeartbeatPong,
            0xfe => OpCode::Paint,
            _ => panic!("Unknown opcode: {}", value),
        }
    }
}

/// A protocol message from the server
#[derive(Debug)]
pub enum ProtocolMessage {
    HeartbeatPing,
    PaintEvent { pos: Pos, color: Rgb },
    PaintResult { status: u8 },
    HeartbeatPong,  // Add the missing variant for client-to-server heartbeat
    Unknown { opcode: u8, data: Vec<u8> },
}

impl ProtocolMessage {
    /// Parse a binary message from the server
    pub fn parse(data: &[u8]) -> Result<Self, PaintboardError> {
        if data.is_empty() {
            return Err(PaintboardError::InvalidData);
        }
        
        let opcode = data[0];
        let payload = &data[1..];
        
        match opcode {
            0xfc => Ok(ProtocolMessage::HeartbeatPing),
            0xfa => {
                if payload.len() < 7 { // 4 bytes for pos + 3 bytes for RGB
                    return Err(PaintboardError::InvalidData);
                }
                
                let pos = Pos::from_bytes(&payload[0..4])?;
                let color = Rgb::from_bytes(&payload[4..7])?;
                
                Ok(ProtocolMessage::PaintEvent { pos, color })
            },
            0xff => {
                if payload.len() < 1 {
                    return Err(PaintboardError::InvalidData);
                }
                
                Ok(ProtocolMessage::PaintResult { status: payload[0] })
            },
            _ => Ok(ProtocolMessage::Unknown { opcode, data: payload.to_vec() }),
        }
    }
    
    /// Convert to binary for transmission
    pub fn to_binary(&self) -> Vec<u8> {
        match self {
            ProtocolMessage::HeartbeatPong => vec![OpCode::HeartbeatPong as u8],
            ProtocolMessage::PaintEvent { pos, color } => {
                let mut data = vec![OpCode::PaintEvent as u8];
                data.extend_from_slice(&pos.to_bytes());
                data.extend_from_slice(&color.to_bytes());
                data
            },
            // Other variants would be server-to-client, so we don't implement them for client transmission
            _ => vec![], // For simplicity, returning empty for non-client messages
        }
    }
}