pub mod local_board;
pub mod sync_manager;

pub use local_board::{LocalBoard, PixelSource, PixelStatus, SyncStatus};
pub use sync_manager::BoardSyncManager;