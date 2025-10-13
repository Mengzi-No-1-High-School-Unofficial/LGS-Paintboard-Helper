# Winter Paintboard SDK

A Rust SDK for interacting with the Winter Paintboard 2026 API. This SDK provides a convenient way to get the current board state, authenticate, and paint pixels on the collaborative canvas.

## Features

- **HTTP API**: Get the current board state and authenticate with UID/access key
- **WebSocket API**: Real-time painting with proper binary protocol handling
- **Batch Operations**: Efficiently send multiple paint operations
- **Type-safe API**: Using `Pos` and `Rgb` structs for clear, type-safe operations
- **Rate Limit Handling**: Automatic handling of rate limits and connection management
- **Event System**: Listen for real-time updates from the server

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
winter-paintboard-sdk = { path = "crates/winter-paintboard-sdk" }  # If using local version
# Or when published:
# winter-paintboard-sdk = "0.1.0"
```

## Usage

### Basic Painting

```rust
use winter_paintboard_sdk::{PaintboardClient, Pos, Rgb, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client with default configuration
    let config = Config::default();
    let mut client = PaintboardClient::new(config).await?;
    
    // Get a token (you'll need your UID and access key)
    let uid = 12345; // Your user ID
    let access_key = "your_access_key"; // Your access key
    let token = client.get_token(uid, &access_key).await?;
    
    // Paint a red pixel at position (100, 50)
    let pos = Pos::new(100, 50)?;
    let color = Rgb::new(255, 0, 0); // Red
    
    let result = client.paint(pos, color).await?;
    println!("Paint result: {:?}", result);
    
    Ok(())
}
```

### Getting the Board

```rust
use winter_paintboard_sdk::{PaintboardClient, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::default();
    let client = PaintboardClient::new(config).await?;
    
    // Get the current board state
    let board = client.get_board().await?;
    println!("Board size: {}x{}", board.width, board.height);
    
    // Get a specific pixel
    let pixel = board.get_pixel(0, 0)?;
    println!("Top-left pixel: RGB({}, {}, {})", pixel.r, pixel.g, pixel.b);
    
    Ok(())
}
```

## API Reference

### Core Types

- `Pos`: Represents a position on the board (x, y coordinates)
- `Rgb`: Represents an RGB color (r, g, b values)
- `Board`: Represents the full 1000x600 pixel board

### Main Client

- `PaintboardClient::new(config)`: Create a new client instance
- `client.get_board()`: Get the current board state
- `client.get_token(uid, access_key)`: Get authentication token
- `client.paint(pos, color)`: Paint a pixel at the given position

## Examples

See the [examples](examples/) directory for more detailed usage examples:

- `basic_paint.rs`: Basic painting example
- `get_board.rs`: Get and examine the current board
- `batch_paint.rs`: Batch painting operations

## Configuration

The SDK can be configured with custom settings:

```rust
use winter_paintboard_sdk::config::Config;
use std::time::Duration;

let config = Config::new(
    "https://paintboard.luogu.me".to_string(),  // API base URL
    "wss://paintboard.luogu.me".to_string(),    // WebSocket URL
    Duration::from_secs(30),                    // Heartbeat interval
    3,                                          // Max retries
    Duration::from_secs(1),                     // Retry delay
    Duration::from_millis(20),                  // Batch timeout
    100,                                        // Max batch size
);
```

## Rate Limits

The Winter Paintboard API has the following rate limits:

- Max 128 packets per second per WebSocket connection
- Max 7 WebSocket connections per IP address
- Recommended: Send merged packets every 20ms (50 packets/second)

The SDK handles these limits automatically where possible.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.