//! `models` 模块定义了与 Winter Paintboard API 交互所需的所有数据模型。
//! 这些数据结构用于表示画板、认证信息、绘图操作和协议消息。

/// `board` 模块定义了画板相关的数据结构，如 `Board` 和 `Pixel`。
mod board;
/// `auth` 模块定义了认证相关的数据结构，如 `TokenResponse` 和 `AuthRequest`。
mod auth;
/// `paint` 模块定义了绘图操作相关的数据结构，如 `PaintOperation` 和 `PaintResult`。
mod paint;
/// `protocol` 模块定义了 WebSocket 协议消息的数据结构，如 `ProtocolMessage` 和 `OpCode`。
mod protocol;
/// `common` 模块定义了通用的数据结构，如 `Rgb` 和 `Pos`。
mod common;

/// 导出 `board` 模块中的 `Board` 和 `Pixel` 类型。
pub use board::{Board, Pixel};
/// 导出 `auth` 模块中的 `TokenResponse` 和 `AuthRequest` 类型。
pub use auth::{TokenResponse, AuthRequest};
/// 导出 `paint` 模块中的 `PaintOperation`, `PaintResult` 和 `PaintStatus` 类型。
pub use paint::{PaintOperation, PaintResult, PaintStatus};
/// 导出 `protocol` 模块中的 `ProtocolMessage` 和 `OpCode` 类型。
pub use protocol::{ProtocolMessage, OpCode};
/// 导出 `common` 模块中的 `Rgb` 和 `Pos` 类型。
pub use common::{Rgb, Pos};