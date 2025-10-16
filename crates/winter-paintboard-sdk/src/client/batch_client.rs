use crate::{
    error::PaintboardError, 
    models::{Rgb, Pos}, 
    config::Config
};
use std::sync::Arc;
use tokio::time::timeout;

/// Batch helper for sending multiple paint operations efficiently
pub struct BatchHelper {
    config: Arc<Config>,
    operations: Vec<(Pos, Rgb)>,
}

impl BatchHelper {
    /// Create a new batch helper
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            operations: Vec::new(),
        }
    }

    /// Add a paint operation to the batch
    pub fn add_paint(&mut self, pos: Pos, color: Rgb) -> Result<(), PaintboardError> {
        if self.operations.len() >= self.config.max_batch_size {
            return Err(PaintboardError::rate_limit());
        }
        
        self.operations.push((pos, color));
        Ok(())
    }

    /// Execute all operations in the batch
    pub async fn execute_batch(&mut self) -> Result<Vec<crate::models::PaintResult>, PaintboardError> {
        let operations = std::mem::take(&mut self.operations);
        let mut results = Vec::new();

        // TODO: In a real implementation, we would:
        // 1. Combine operations into fewer network requests
        // 2. Handle rate limiting
        // 3. Process responses
        
        // For now, just return dummy results
        for i in 0..operations.len() {
            results.push(crate::models::PaintResult {
                drawing_id: i as u32, // Use index as drawing ID
                status: crate::models::PaintStatus::Success,
                message: "Batch operation successful".to_string(),
            });
        }

        Ok(results)
    }

    /// Execute batch with timeout
    pub async fn execute_batch_with_timeout(&mut self) -> Result<Vec<crate::models::PaintResult>, PaintboardError> {
        timeout(
            self.config.batch_timeout,
            self.execute_batch()
        )
        .await
        .map_err(|_| PaintboardError::Timeout)?
    }

    /// Get the number of pending operations
    pub fn pending_count(&self) -> usize {
        self.operations.len()
    }

    /// Clear all pending operations
    pub fn clear(&mut self) {
        self.operations.clear();
    }
}