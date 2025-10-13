//! Data models for the Winter Paintboard SDK

mod board;
mod auth;
mod paint;
mod protocol;
mod common;

pub use board::{Board, Pixel};
pub use auth::{TokenResponse, AuthRequest};
pub use paint::{PaintOperation, PaintResult, PaintStatus};
pub use protocol::{ProtocolMessage, OpCode};
pub use common::{Rgb, Pos};