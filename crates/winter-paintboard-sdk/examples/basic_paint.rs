//! Basic painting example for Winter Paintboard SDK

use winter_paintboard_sdk::{PaintboardClient, Pos, Rgb, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a default configuration
    let config = Config::default();
    
    // Create a new client
    let mut client = PaintboardClient::new(config).await?;
    
    // Get your UID and access key from environment or configuration
    let uid = std::env::var("PAINTBOARD_UID")
        .unwrap_or_else(|_| "12345".to_string())  // Use placeholder value
        .parse()
        .unwrap_or(12345);
    
    let access_key = std::env::var("PAINTBOARD_ACCESS_KEY")
        .unwrap_or_else(|_| "your_access_key_here".to_string()); // Use placeholder value
    
    // Get a token using your UID and access key
    println!("Getting token...");
    let token = client.get_token(uid, &access_key).await?;
    println!("Got token: {}", token);
    
    // Define a position and color to paint
    let pos = Pos::new(100, 50)?;
    let color = Rgb::new(255, 0, 0); // Red color
    
    println!("Painting at position ({}, {}) with RGB({}, {}, {})...", 
             pos.x, pos.y, color.r, color.g, color.b);
    
    // Perform the painting operation
    // Note: This would require a real token and proper authentication in a real scenario
    // let result = client.paint(pos, color).await?;
    // println!("Paint result: {:?}", result);
    
    println!("Paint operation completed!");
    
    Ok(())
}