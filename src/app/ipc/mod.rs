//! IPC 模块
//!
//! 提供 Master-Worker 进程间通信功能

pub mod master;
pub mod protocol;
pub mod worker;

pub use master::SyncMaster;
pub use worker::SyncWorker;
