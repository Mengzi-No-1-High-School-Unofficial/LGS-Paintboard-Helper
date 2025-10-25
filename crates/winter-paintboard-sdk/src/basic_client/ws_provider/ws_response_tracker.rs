use crate::models::{PaintResult, PaintStatus};
use once_cell::sync::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex as TokioMutex};
use tokio::time::{Duration, Instant};

static GLOBAL_WS_RESPONSE_TRACKER: OnceCell<WsResponseTracker> = OnceCell::new();

/// 响应追踪器：管理 paint 请求对应的 oneshot 发送端
///
/// 全局追踪器为 [`GLOBAL_WS_RESPONSE_TRACKER`]，默认 [`WsResponseTracker::new`] 返回全局追踪器（不存在则创建）
///
/// 如果需要独立的追踪器，请使用 [`WsResponseTracker::new_local`]
#[derive(Clone)]
pub struct WsResponseTracker {
    channels: Arc<TokioMutex<HashMap<u32, (oneshot::Sender<PaintResult>, Instant)>>>,
}

impl WsResponseTracker {
    /// 返回全局追踪器，如不存在则使用 [`WsResponseTracker::new_local`] 创建
    pub fn new() -> Self {
        let tracker = GLOBAL_WS_RESPONSE_TRACKER.get_or_init(|| WsResponseTracker::new_local());

        // 成员变量均为 Arc + Mutex，直接 Clone 不会导致引用的丢失
        tracker.clone()
    }

    /// 创建新的追踪器（无论是否存在全局追踪器）
    pub fn new_local() -> Self {
        Self {
            channels: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// 注册一个请求并返回接收端
    pub async fn register_request(&self, paint_id: u32) -> oneshot::Receiver<PaintResult> {
        let (tx, rx) = oneshot::channel();
        let mut guard = self.channels.lock().await;
        guard.insert(paint_id, (tx, Instant::now()));
        rx
    }

    /// 完成请求：根据 paint_id 发送结果，返回是否找到对应通道
    pub async fn complete_request(&self, paint_id: u32, result: PaintResult) -> bool {
        let mut guard = self.channels.lock().await;
        if let Some((tx, _)) = guard.remove(&paint_id) {
            // 如果发送失败（接收端被丢弃），则忽略错误
            let _ = tx.send(result);
            true
        } else {
            false
        }
    }

    /// 移除并返回是否存在（用于超时清理）
    pub async fn remove_request(&self, paint_id: u32) -> bool {
        let mut guard = self.channels.lock().await;
        guard.remove(&paint_id).is_some()
    }

    /// 获取当前挂起通道数量（仅用于监控 / 测试）
    pub async fn pending_count(&self) -> usize {
        let guard = self.channels.lock().await;
        guard.len()
    }

    /// 清理超时的请求（超过指定持续时间未响应的请求）
    pub async fn cleanup_expired_requests(&self, timeout_duration: Duration) -> usize {
        let mut guard = self.channels.lock().await;
        let mut removed_count = 0;

        let mut expired_keys = Vec::new();

        // 首先找出所有过期的请求
        for (paint_id, (_, timestamp)) in guard.iter() {
            if timestamp.elapsed() > timeout_duration {
                expired_keys.push(*paint_id);
            }
        }

        // 然后移除过期的请求并发送超时结果
        for paint_id in expired_keys {
            if let Some((tx, _)) = guard.remove(&paint_id) {
                // 请求已超时，尝试发送超时错误（如果接收端仍然存在）
                let _ = tx.send(PaintResult {
                    drawing_id: paint_id as u32, // 使用原始paint_id作为drawing_id
                    status: PaintStatus::Timeout,
                    message: "Request timed out".to_string(),
                });
                removed_count += 1;
            }
        }

        removed_count
    }
}
