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
        let payload_start = 1;

        // Determine message structure and expected length first
        let (expected_payload_len, is_known_opcode) = match opcode {
            0xfc => (0, true), // HeartbeatPing: 0 bytes payload
            0xfa => (7, true), // PaintEvent: 7 bytes payload
            0xff => (5, true), // PaintResult: 5 bytes payload
            0xfb => (0, true), // HeartbeatPong: 0 bytes payload (if we were parsing client messages)
            _ => (0, false),   // Unknown
        };

        if !is_known_opcode {
            // Unknown opcode. We don't know the length.
            // We'll treat it as 1 byte consumed (just the opcode) and return Unknown.
            // This is risky if it actually has a payload, but it's the best we can do without a length field.
            return Ok((
                ProtocolMessage::Unknown {
                    opcode,
                    data: Vec::new(),
                },
                1,
            ));
        }

        // Check if we have enough bytes for the payload
        if remaining_data.len() < 1 + expected_payload_len {
            return Err(PaintboardError::invalid_data(format!(
                "Not enough bytes for opcode 0x{:02x}, expected {} bytes payload",
                opcode, expected_payload_len
            )));
        }

        let payload = &remaining_data[payload_start..payload_start + expected_payload_len];
        let total_consumed = 1 + expected_payload_len;

        // Now parse semantics. If semantic parsing fails, we return Unknown or log and ignore,
        // but crucially we return the correct `total_consumed` so the loop can continue.
        match opcode {
            0xfc => Ok((ProtocolMessage::HeartbeatPing, total_consumed)),
            0xfa => {
                let pos_res = Pos::from_bytes(&payload[0..4]);
                let color_res = Rgb::from_bytes(&payload[4..7]);

                match (pos_res, color_res) {
                    (Ok(pos), Ok(color)) => {
                        Ok((ProtocolMessage::PaintEvent { pos, color }, total_consumed))
                    }
                    (Err(_), _) | (_, Err(_)) => {
                        // Invalid position or color, but we consumed the bytes.
                        // Return Unknown so it's logged but skipped.
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
            0xff => {
                let drawing_id =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let status = payload[4];
                Ok((
                    ProtocolMessage::PaintResult { drawing_id, status },
                    total_consumed,
                ))
            }
            _ => unreachable!("Handled by is_known_opcode check"),
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
