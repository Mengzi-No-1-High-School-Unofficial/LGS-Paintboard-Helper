//! IPC 协议模块
//!
//! 定义 Master-Worker 之间的通信协议,使用 Unix Socket 进行进程间通信。

use serde::{Deserialize, Serialize};

use super::metrics::WorkerMetrics;

/// Master → Worker 消息
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum MasterMessage {
    /// 初始化:发送完整画板数据
    ///
    /// 当 Worker 首次连接时发送
    FullBoard {
        width: u16,
        height: u16,
        pixels: Vec<u8>, // RGB 格式,长度 = width * height * 3
    },

    /// 单像素更新
    ///
    /// 来源: WebSocket 0xFA 事件
    PixelUpdate { x: u16, y: u16, r: u8, g: u8, b: u8 },

    /// 批量像素更新
    ///
    /// 来源: HTTP 全量同步后的差异
    BatchUpdate { updates: Vec<PixelUpdateData> },

    /// 心跳
    Heartbeat { timestamp: u64 },

    /// Master 关闭通知
    Shutdown,
}

/// 像素更新数据
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PixelUpdateData {
    pub x: u16,
    pub y: u16,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Worker → Master 消息
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum WorkerMessage {
    /// Worker 注册
    Register { worker_id: String, version: String },

    /// 心跳响应
    HeartbeatAck { timestamp: u64 },

    /// 监控数据上报
    MetricsReport { metrics: WorkerMetrics },

    /// Worker 断开连接
    Disconnect { reason: String },
}
