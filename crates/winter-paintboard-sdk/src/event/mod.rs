use crate::models::{Rgb, Pos};
use tokio::sync::broadcast;
use std::sync::{Arc, OnceLock};

/// Winter Paintboard SDK 的事件类型。
/// 这些事件由客户端发出或接收，用于通知应用程序状态变化。
#[derive(Debug, Clone)]
pub enum Event {
    /// 当前客户端实例发送的绘图事件。
    OwnPaintEvent { pos: Pos, color: Rgb },
    /// 其他客户端发送的绘图事件。
    OtherPaintEvent { pos: Pos, color: Rgb },
    /// 心跳事件，用于保持连接活跃。
    HeartbeatEvent,
    /// WebSocket 连接成功打开的事件。
    ConnectionOpened,
    /// WebSocket 连接关闭的事件。
    ConnectionClosed,
    /// 包含关闭状态码的 WebSocket 连接关闭事件。
    ConnectionClosedWithCode(u16),
    /// 发生错误事件，包含错误信息。
    ErrorOccurred(String),
}

impl Event {
    /// 创建一个表示当前客户端绘图的事件。
    ///
    /// # 参数
    /// - `pos`: 绘制的像素位置。
    /// - `color`: 绘制的像素颜色。
    pub fn own_paint_event(pos: Pos, color: Rgb) -> Self {
        Event::OwnPaintEvent { pos, color }
    }
    
    /// 创建一个表示其他客户端绘图的事件。
    ///
    /// # 参数
    /// - `pos`: 绘制的像素位置。
    /// - `color`: 绘制的像素颜色。
    pub fn other_paint_event(pos: Pos, color: Rgb) -> Self {
        Event::OtherPaintEvent { pos, color }
    }
    
    /// 保持向后兼容性，将 `paint_event` 映射为 `OtherPaintEvent`。
    ///
    /// # 参数
    /// - `pos`: 绘制的像素位置。
    /// - `color`: 绘制的像素颜色。
    pub fn paint_event(pos: Pos, color: Rgb) -> Self {
        Event::OtherPaintEvent { pos, color }
    }
    
    /// 创建一个表示发生错误的事件。
    ///
    /// # 参数
    /// - `message`: 错误的详细信息。
    pub fn error_event(message: String) -> Self {
        Event::ErrorOccurred(message)
    }
}

/// Winter Paintboard SDK 的事件总线。
///
/// 允许发布和订阅 `Event` 类型的消息，实现组件间的解耦通信。
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    /// 使用给定通道容量创建一个新的 `EventBus`。
    ///
    /// # 参数
    /// - `capacity`: 事件通道的最大容量。
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// 将一个事件发送给所有订阅者。
    ///
    /// 如果通道已满且没有接收者，事件可能会被丢弃。
    ///
    /// # 参数
    /// - `event`: 要发送的事件。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `EventBusError`。
    pub fn send(&self, event: Event) -> Result<(), EventBusError> {
        self.sender.send(event).map(|_| ()).map_err(|e| EventBusError::SendError(e.0))
    }

    /// 订阅此事件总线的事件流。
    ///
    /// 返回一个 `broadcast::Receiver`，可以用于异步接收事件。
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
    
    /// 返回当前活跃的订阅者数量。
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

/// 事件总线相关的错误类型。
#[derive(Debug)]
pub enum EventBusError {
    /// 发送事件失败，包含未能发送的事件。
    SendError(Event),
}

impl std::fmt::Display for EventBusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventBusError::SendError(event) => write!(f, "Failed to send event: {:?}", event),
        }
    }
}

impl std::error::Error for EventBusError {}

/// 全局事件总线实例。
/// 使用 `OnceLock` 确保全局实例只被初始化一次。
static GLOBAL_EVENT_BUS: OnceLock<EventBus> = OnceLock::new();
const DEFAULT_EVENT_BUS_CAPACITY: usize = 1024 * 1024;  // 1 megas

impl EventBus {
    /// 获取或创建全局单例事件总线实例。
    ///
    /// 如果全局事件总线尚未初始化，它将以默认容量 ([`DEFAULT_EVENT_BUS_CAPACITY`]) 进行初始化。
    pub fn global() -> EventBus {
        GLOBAL_EVENT_BUS
            .get_or_init(|| EventBus::new(DEFAULT_EVENT_BUS_CAPACITY))
            .clone()
    }
    
    /// 使用自定义容量初始化全局事件总线。
    ///
    /// 此函数只在全局事件总线尚未初始化时生效。
    /// 如果已初始化，则返回错误。
    ///
    /// # 参数
    /// - `capacity`: 自定义的事件通道容量。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，表示初始化成功，
    /// 失败时包含 `EventBusError::SendError`，表示全局事件总线已初始化。
    pub fn init_global_with_capacity(capacity: usize) -> Result<(), EventBusError> {
        match GLOBAL_EVENT_BUS.set(EventBus::new(capacity)) {
            Ok(()) => Ok(()),
            Err(_) => Err(EventBusError::SendError(Event::ErrorOccurred("Global event bus already initialized".to_string()))),
        }
    }
}
