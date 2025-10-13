//! Get board example for Winter Paintboard SDK

use winter_paintboard_sdk::{PaintboardClient, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a default configuration
    let config = Config::default();
    
    // Create a new client
    let client = PaintboardClient::new(config).await?;
    
    log::debug!("Getting the current board...");
    
    // Get the current board state
    let board = client.get_board().await?;
    
    log::debug!("Successfully retrieved board:");
    log::debug!("  Dimensions: {}x{}", board.width, board.height);
    log::debug!("  Data size: {} bytes", board.data.len());
    
    // Get a specific pixel (example: top-left corner)
    if let Ok(pixel) = board.get_pixel(0, 0) {
        log::debug!("  Top-left pixel RGB: ({}, {}, {})", pixel.r, pixel.g, pixel.b);
    }
    
    // Get another pixel (example: center)
    if let Ok(pixel) = board.get_pixel(500, 300) {
        log::debug!("  Center pixel RGB: ({}, {}, {})", pixel.r, pixel.g, pixel.b);
    }
    
    Ok(())
}