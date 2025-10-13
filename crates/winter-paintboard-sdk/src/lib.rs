//! Winter Paintboard SDK
//! 
//! A Rust SDK for interacting with the Winter Paintboard 2026 API.
//! Provides functionality for painting, authentication, and real-time events.

pub mod client;
pub mod models;
pub mod utils;
pub mod error;
pub mod config;
pub mod event;

pub use client::PaintboardClient;
pub use models::{Rgb, Pos, Board, PaintResult};
pub use error::PaintboardError;