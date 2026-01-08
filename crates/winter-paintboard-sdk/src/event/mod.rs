//! 全局事件总线模块
//!
//! 提供一个全局的事件总线，用于在程序的不同部分之间解耦通信。
//! 基于 `tokio::sync::broadcast` 实现。

use crate::models::{Pos, Rgb};
use once_cell::sync::Lazy;
use tokio::sync::broadcast::{self, Receiver, Sender};

/// 包含事件具体信息的枚举。
#[derive(Debug, Clone, Copy)]
pub enum PaintEvent {
    /// 绘制成功事件。
    Success { uid: u32, pos: Pos, color: Rgb },
    /// 像素更新事件（来自 WebSocket 0xFA 消息）。
    PixelUpdate { pos: Pos, color: Rgb },
    /// 绘制失败事件。
    Failure { uid: u32, pos: Pos },
}

/// 全局事件总线。
pub struct EventBus {
    /// `broadcast` channel 的发送端。
    sender: Sender<PaintEvent>,
}

/// 全局唯一的事件总线实例。
static EVENT_BUS: Lazy<EventBus> = Lazy::new(|| {
    let (sender, _) = broadcast::channel(1024); // Channel 容量为 1024
    EventBus { sender }
});

/// 订阅事件。
///
/// 返回一个 `Receiver`，可以用来接收 `PaintEvent`。
pub fn subscribe() -> Receiver<PaintEvent> {
    EVENT_BUS.sender.subscribe()
}

/// 发布一个 `PaintEvent`。
///
/// # 参数
/// - `event`: 要发布的事件实例。
pub fn post(event: PaintEvent) {
    // 发送失败是一个可接受的错误，因为它只意味着当前没有活跃的订阅者。
    let _ = EVENT_BUS.sender.send(event);
}
