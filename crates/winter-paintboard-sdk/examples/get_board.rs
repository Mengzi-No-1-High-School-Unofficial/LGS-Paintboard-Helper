//! Get board example for Winter Paintboard SDK

use winter_paintboard_sdk::{PaintboardClient, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a default configuration
    let config = Config::default();
    
    // Create a new client
    let client = PaintboardClient::new(config).await?;
    
    println!("Getting the current board...");
    
    // Get the current board state
    let board = client.get_board().await?;
    
    println!("Successfully retrieved board:");
    println!("  Dimensions: {}x{}", board.width, board.height);
    println!("  Data size: {} bytes", board.data.len());
    
    // Get a specific pixel (example: top-left corner)
    if let Ok(pixel) = board.get_pixel(0, 0) {
        println!("  Top-left pixel RGB: ({}, {}, {})", pixel.r, pixel.g, pixel.b);
    }
    
    // Get another pixel (example: center)
    if let Ok(pixel) = board.get_pixel(500, 300) {
        println!("  Center pixel RGB: ({}, {}, {})", pixel.r, pixel.g, pixel.b);
    }
    
    Ok(())
}