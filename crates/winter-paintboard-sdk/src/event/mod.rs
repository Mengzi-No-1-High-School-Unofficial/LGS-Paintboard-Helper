use crate::models::{Rgb, Pos};
use tokio::sync::broadcast;
use std::sync::{Arc, OnceLock};

/// Event types for the Winter Paintboard SDK
#[derive(Debug, Clone)]
pub enum Event {
    OwnPaintEvent { pos: Pos, color: Rgb },      // 当前实例发送的绘图事件
    OtherPaintEvent { pos: Pos, color: Rgb },    // 其他人发送的绘图事件
    HeartbeatEvent,
    ConnectionOpened,
    ConnectionClosed,
    ErrorOccurred(String),
}

impl Event {
    pub fn own_paint_event(pos: Pos, color: Rgb) -> Self {
        Event::OwnPaintEvent { pos, color }
    }
    
    pub fn other_paint_event(pos: Pos, color: Rgb) -> Self {
        Event::OtherPaintEvent { pos, color }
    }
    
    // 保持向后兼容性，将 paint_event 映射为 OtherPaintEvent
    pub fn paint_event(pos: Pos, color: Rgb) -> Self {
        Event::OtherPaintEvent { pos, color }
    }
    
    pub fn error_event(message: String) -> Self {
        Event::ErrorOccurred(message)
    }
}

/// Event bus for the Winter Paintboard SDK
#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<Event>,
}

impl EventBus {
    /// Creates a new EventBus with a given channel capacity
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Sends an event to all subscribers
    pub fn send(&self, event: Event) -> Result<(), EventBusError> {
        self.sender.send(event).map(|_| ()).map_err(|e| EventBusError::SendError(e.0))
    }

    /// Subscribe to events from this event bus
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }
    
    /// Returns the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

#[derive(Debug)]
pub enum EventBusError {
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

/// Global event bus instance
static GLOBAL_EVENT_BUS: OnceLock<EventBus> = OnceLock::new();

impl EventBus {
    /// Get or create a global singleton event bus instance
    pub fn global() -> EventBus {
        GLOBAL_EVENT_BUS
            .get_or_init(|| EventBus::new(10000)) // Default capacity of 10000 events
            .clone()
    }
    
    /// Initialize the global event bus with a custom capacity (only works if not already initialized)
    pub fn init_global_with_capacity(capacity: usize) -> Result<(), EventBusError> {
        match GLOBAL_EVENT_BUS.set(EventBus::new(capacity)) {
            Ok(()) => Ok(()),
            Err(_) => Err(EventBusError::SendError(Event::ErrorOccurred("Global event bus already initialized".to_string()))),
        }
    }
}
