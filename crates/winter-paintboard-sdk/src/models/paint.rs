//! `paint` 模块定义了与画板绘画操作相关的请求、响应和状态数据结构。

use crate::models::{OpCode, Pos, Rgb};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 绘画操作的结果。
///
/// 服务器在响应绘画请求时会返回此结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaintResult {
    /// 绘画操作的唯一 ID。
    pub drawing_id: u32,
    /// 绘画操作的状态，指示成功或失败原因。
    pub status: PaintStatus,
    /// 伴随绘画结果的可选消息，通常用于提供更多详情。
    pub message: String,
}

/// 绘画操作的可能状态。
///
/// 这些状态由服务器返回，指示绘画请求的处理结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PaintStatus {
    /// 绘画操作成功完成。
    Success,
    /// 操作处于冷却期，请求被拒绝。
    Cooldown,
    /// 提供的令牌无效或已过期。
    InvalidToken,
    /// 提供的坐标超出画板范围或格式不正确。
    InvalidCoordinate,
    /// 没有执行绘画操作的权限。
    NoPermission,
    /// 服务器内部错误导致操作失败。
    ServerError,
    /// 绘画操作超时。
    Timeout,
    /// 未知的绘画状态。
    Unknown,
}

impl From<u8> for PaintStatus {
    /// 将 `u8` 值转换为对应的 `PaintStatus` 枚举变体。
    ///
    /// 这个实现用于解析来自服务器的二进制协议中表示绘画状态的字节。
    ///
    /// # 参数
    /// - `value`: 表示绘画状态的字节值。
    ///
    /// # 返回
    /// 对应的 `PaintStatus` 枚举变体。如果字节值未知，则返回 `PaintStatus::Unknown`。
    fn from(value: u8) -> Self {
        match value {
            0xef => PaintStatus::Success,
            0xee => PaintStatus::Cooldown,
            0xed => PaintStatus::InvalidToken,
            0xec => PaintStatus::InvalidCoordinate,
            0xeb => PaintStatus::NoPermission,
            0xea => PaintStatus::ServerError,
            // 协议中没有明确的超时状态字节，但为了客户端逻辑一致性而添加。
            // 0xe9 => PaintStatus::Timeout,
            _ => PaintStatus::Unknown,
        }
    }
}

/// 发送到服务器的绘画操作。
///
/// 包含绘画所需的坐标、颜色、用户令牌和绘画 ID。
#[derive(Debug, Clone)]
pub struct PaintOperation {
    /// 绘画操作的目标位置坐标。
    pub pos: Pos,
    /// 要在指定位置绘制的颜色。
    pub color: Rgb,
    /// 令牌的 UID 部分（将拆分为 3 字节）。
    pub token_uid: u32,
    /// 完整的 UUID 令牌字符串。
    pub token: String,
    /// 绘画操作的唯一 ID，通常用于区分不同的绘画请求。
    pub paint_id: u32,
}

impl PaintOperation {
    /// 将绘画操作转换为二进制格式，以便通过 WebSocket 传输。
    ///
    /// 格式如下：
    /// - 1 字节: 操作码 (`OpCode::Paint` 即 `0xfe`)
    /// - 2 字节: X 坐标 (小端序 `u16`)
    /// - 2 字节: Y 坐标 (小端序 `u16`)
    /// - 3 字节: RGB 颜色 (`r`, `g`, `b` `u8`)
    /// - 3 字节: 令牌 UID (小端序 `u32` 的前 3 字节)
    /// - 16 字节: UUID 令牌 (二进制格式)
    /// - 4 字节: 绘画 ID (小端序 `u32`)
    ///
    /// # 返回
    /// 包含绘画操作二进制数据的 `Vec<u8>`。
    pub fn to_binary(&self) -> Vec<u8> {
        let mut data = vec![OpCode::Paint as u8]; // 0xfe
        data.extend_from_slice(&self.pos.x.to_le_bytes()); // X 坐标 (uint16)
        data.extend_from_slice(&self.pos.y.to_le_bytes()); // Y 坐标 (uint16)
        data.extend_from_slice(&[self.color.r, self.color.g, self.color.b]); // RGB 值

        // 下面 23 字节: 3 字节令牌 UID + 16 字节令牌 + 4 字节绘画 ID

        // 根据协议将 UID 分割成 3 字节
        let uid_bytes = self.token_uid.to_le_bytes();
        data.extend_from_slice(&uid_bytes[0..3]); // 取 UID 的前 3 字节

        // 添加 16 字节令牌 - 将 UUID 字符串解析为 16 字节二进制格式
        let token_uuid = Uuid::parse_str(&self.token).unwrap_or_else(|_| Uuid::nil()); // 如果解析失败，则使用 nil UUID
        data.extend_from_slice(token_uuid.as_bytes());

        // 添加 4 字节绘画 ID
        data.extend_from_slice(&self.paint_id.to_le_bytes());

        data
    }
}
