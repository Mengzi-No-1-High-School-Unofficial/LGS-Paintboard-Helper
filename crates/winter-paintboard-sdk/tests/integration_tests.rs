#[cfg(test)]
mod integration_tests {
    use winter_paintboard_sdk::{PaintboardClient, Pos, Rgb, config::Config};

    #[tokio::test]
    async fn test_client_creation() {
        let config = Config::default();
        let client = PaintboardClient::new(config).await;
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_pos_creation() {
        // Valid positions
        assert!(Pos::new(0, 0).is_ok());
        assert!(Pos::new(999, 599).is_ok());
        assert!(Pos::new(500, 300).is_ok());

        // Invalid positions
        assert!(Pos::new(1000, 0).is_err());  // X out of range
        assert!(Pos::new(0, 600).is_err());   // Y out of range
        assert!(Pos::new(1001, 601).is_err()); // Both out of range
    }

    #[tokio::test]
    async fn test_rgb_creation() {
        let rgb = Rgb::new(255, 128, 64);
        assert_eq!(rgb.r, 255);
        assert_eq!(rgb.g, 128);
        assert_eq!(rgb.b, 64);
    }

    #[tokio::test]
    #[ignore] // Ignore this test as it requires a real API endpoint
    async fn test_get_board() {
        let config = Config::default();
        let client = PaintboardClient::new(config).await.unwrap();
        
        // This test would require a real API endpoint to work
        let result = client.get_board().await;
        // assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore] // Ignore this test as it requires a real API endpoint and valid credentials
    async fn test_get_token() {
        let config = Config::default();
        let client = PaintboardClient::new(config).await.unwrap();
        
        // This test would require actual credentials to work
        // let result = client.get_token(12345, "test_access_key").await;
        // assert!(result.is_ok());
    }
}