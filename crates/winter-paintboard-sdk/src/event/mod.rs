use crate::models::{Rgb, Pos};

/// Event types for the Winter Paintboard SDK
#[derive(Debug, Clone)]
pub enum Event {
    PaintEvent { pos: Pos, color: Rgb },
    HeartbeatEvent,
    ConnectionOpened,
    ConnectionClosed,
    ErrorOccurred(String),
}

impl Event {
    pub fn paint_event(pos: Pos, color: Rgb) -> Self {
        Event::PaintEvent { pos, color }
    }
    
    pub fn error_event(message: String) -> Self {
        Event::ErrorOccurred(message)
    }
}

/// Event stream for handling real-time events
pub struct EventStream {
    // In a real implementation this would contain the actual stream
    // For now this is a placeholder
}