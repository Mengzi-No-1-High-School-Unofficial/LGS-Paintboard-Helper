use crate::{
    error::PaintboardError,
    models::{Pos, Rgb},
};
use tracing::warn;

/// WebSocket protocol operation codes
#[repr(u8)]
pub enum OpCode {
    HeartbeatPing = 0xfc, // Server to client
    PaintEvent = 0xfa,    // Server to client
    PaintResult = 0xff,   // Server to client
    HeartbeatPong = 0xfb, // Client to server
    Paint = 0xfe,         // Client to server
    UnknownOpCode = 0xaf, // Used for unknown opcodes
}

impl From<u8> for OpCode {
    fn from(value: u8) -> Self {
        match value {
            0xfc => OpCode::HeartbeatPing,
            0xfa => OpCode::PaintEvent,
            0xff => OpCode::PaintResult,
            0xfb => OpCode::HeartbeatPong,
            0xfe => OpCode::Paint,
            _ => {
                warn!("Unknown opcode: {}", value);
                OpCode::UnknownOpCode
            }
        }
    }
}

/// A protocol message from the server
#[derive(Debug)]
pub enum ProtocolMessage {
    HeartbeatPing,
    PaintEvent { pos: Pos, color: Rgb },
    PaintResult { drawing_id: u32, status: u8 },
    HeartbeatPong, // Add the missing variant for client-to-server heartbeat
    Unknown { opcode: u8, data: Vec<u8> },
}

impl ProtocolMessage {
    /// Parse a single binary message from the server (non-sticky packet)
    pub fn parse(data: &[u8]) -> Result<Self, PaintboardError> {
        if data.is_empty() {
            return Err(PaintboardError::invalid_data(
                "Empty data for protocol message parsing",
            ));
        }

        let opcode = data[0];
        let payload = &data[1..];

        match opcode {
            0xfc => Ok(ProtocolMessage::HeartbeatPing),
            0xfa => {
                if payload.len() < 7 {
                    // 4 bytes for pos + 3 bytes for RGB
                    return Err(PaintboardError::invalid_data(
                        "Not enough bytes for PaintEvent payload",
                    ));
                }

                let pos = Pos::from_bytes(&payload[0..4])?;
                let color = Rgb::from_bytes(&payload[4..7])?;

                Ok(ProtocolMessage::PaintEvent { pos, color })
            }
            0xff => {
                if payload.len() < 5 {
                    // 4 bytes for drawing_id + 1 byte for status
                    return Err(PaintboardError::invalid_data(
                        "Not enough bytes for PaintResult payload",
                    ));
                }

                let drawing_id =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let status = payload[4];

                Ok(ProtocolMessage::PaintResult { drawing_id, status })
            }
            _ => Ok(ProtocolMessage::Unknown {
                opcode,
                data: payload.to_vec(),
            }),
        }
    }

    /// Parse a batch of sticky packets (multiple messages concatenated in one binary message)
    pub fn parse_batch(data: &[u8]) -> Result<Vec<Self>, PaintboardError> {
        let mut messages = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            // Parse a single message and get how many bytes it consumed
            let message = ProtocolMessage::parse_single_message_at_offset(data, offset)?;
            let (protocol_message, consumed) = message;
            messages.push(protocol_message);
            offset += consumed;
        }

        if messages.is_empty() {
            return Err(PaintboardError::invalid_data(
                "No valid protocol messages found in data",
            ));
        }

        Ok(messages)
    }

    /// Parse a single message at a specific offset in the data
    /// Returns the parsed message and the number of bytes consumed
    fn parse_single_message_at_offset(
        data: &[u8],
        offset: usize,
    ) -> Result<(Self, usize), PaintboardError> {
        if offset >= data.len() {
            return Err(PaintboardError::invalid_data("Offset beyond data length"));
        }

        let remaining_data = &data[offset..];
        if remaining_data.is_empty() {
            return Err(PaintboardError::invalid_data(
                "Empty data for protocol message parsing",
            ));
        }

        let opcode = remaining_data[0];
        let payload = &remaining_data[1..];

        match opcode {
            0xfc => Ok((ProtocolMessage::HeartbeatPing, 1)), // 1 byte for opcode
            0xfa => {
                if payload.len() < 7 {
                    return Err(PaintboardError::invalid_data(
                        "Not enough bytes for PaintEvent payload",
                    ));
                }

                let pos = Pos::from_bytes(&payload[0..4])?;
                let color = Rgb::from_bytes(&payload[4..7])?;

                Ok((ProtocolMessage::PaintEvent { pos, color }, 1 + 7)) // 1 byte opcode + 7 bytes payload
            }
            0xff => {
                if payload.len() < 5 {
                    return Err(PaintboardError::invalid_data(
                        "Not enough bytes for PaintResult payload",
                    ));
                }

                let drawing_id =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let status = payload[4];

                Ok((ProtocolMessage::PaintResult { drawing_id, status }, 1 + 5))
                // 1 byte opcode + 5 bytes payload
            }
            _ => {
                // For unknown opcodes, return the opcode and payload
                // We need to determine how many bytes to consume based on the opcode
                // For now, we'll return a conservative estimate based on common message sizes
                let total_consumed = match opcode {
                    0xfb => 1, // HeartbeatPong
                    _ => {
                        // For unknown opcodes, we can't determine the exact length
                        // We'll return an error to prevent consuming all remaining data
                        return Err(PaintboardError::invalid_data(format!(
                            "Unknown opcode: 0x{:02x}",
                            opcode
                        )));
                    }
                };

                Ok((
                    ProtocolMessage::Unknown {
                        opcode,
                        data: payload.to_vec(),
                    },
                    total_consumed,
                ))
            }
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
            }
            // Other variants would be server-to-client, so we don't implement them for client transmission
            _ => vec![], // For simplicity, returning empty for non-client messages
        }
    }
}
