//! Batch painting example for Winter Paintboard SDK

use winter_paintboard_sdk::{PaintboardClient, Pos, Rgb, config::Config, client::BatchClient};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a default configuration
    let config = Arc::new(Config::default());
    
    // Create a new client
    let mut client = PaintboardClient::new((*config).clone()).await?;
    
    // Create a batch client
    let mut batch_client = BatchClient::new(config.clone());
    
    // Add several paint operations to the batch
    for i in 0..5 {
        let pos = Pos::new(100 + i as u16, 50)?;
        let color = Rgb::new((50 * i as u16) as u8, (100 * i as u16) as u8, (150 * i as u16) as u8);
        
        batch_client.add_paint(pos, color)?;
        println!("Added paint operation at ({}, {}) with color RGB({}, {}, {})", 
                 pos.x, pos.y, color.r, color.g, color.b);
    }
    
    println!("Executing batch of {} operations...", batch_client.pending_count());
    
    // Execute the batch
    let results = batch_client.execute_batch().await?;
    
    println!("Batch execution completed. Results: {} results", results.len());
    for (i, result) in results.iter().enumerate() {
        println!("  Operation {}: {:?}", i, result.status);
    }
    
    Ok(())
}