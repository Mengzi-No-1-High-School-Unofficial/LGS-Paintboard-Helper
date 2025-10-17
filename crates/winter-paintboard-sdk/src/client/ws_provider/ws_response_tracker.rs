use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex as TokioMutex};
use crate::models::PaintResult;

/// 响应追踪器：管理 paint 请求对应的 oneshot 发送端
pub struct WsResponseTracker {
    channels: Arc<TokioMutex<HashMap<u64, oneshot::Sender<PaintResult>>>>,
}

impl WsResponseTracker {
    pub fn new() -> Self {
        Self {
            channels: Arc::new(TokioMutex::new(HashMap::new())),
        }
    }

    /// 注册一个请求并返回接收端
    pub async fn register_request(&self, paint_id: u64) -> oneshot::Receiver<PaintResult> {
        let (tx, rx) = oneshot::channel();
        let mut guard = self.channels.lock().await;
        guard.insert(paint_id, tx);
        rx
    }

    /// 完成请求：根据 paint_id 发送结果，返回是否找到对应通道
    pub async fn complete_request(&self, paint_id: u64, result: PaintResult) -> bool {
        let mut guard = self.channels.lock().await;
        if let Some(tx) = guard.remove(&paint_id) {
            // 如果发送失败（接收端被丢弃），则忽略错误
            let _ = tx.send(result);
            true
        } else {
            false
        }
    }

    /// 完成第一个挂起的请求（回退策略）
    /// 有些协议情况下，服务器返回的 drawing_id 无法直接映射到我们发送的 paint_id，
    /// 这里保留原始实现的回退行为：将结果派发到第一个可用的挂起通道。
    pub async fn complete_first_request(&self, result: PaintResult) -> bool {
        let mut guard = self.channels.lock().await;
        // 取第一个 key
        if let Some((&first_key, _)) = guard.iter().next() {
            if let Some(tx) = guard.remove(&first_key) {
                let _ = tx.send(result);
                return true;
            }
        }
        false
    }

    /// 移除并返回是否存在（用于超时清理）
    pub async fn remove_request(&self, paint_id: u64) -> bool {
        let mut guard = self.channels.lock().await;
        guard.remove(&paint_id).is_some()
    }

    /// 获取当前挂起通道数量（仅用于监控 / 测试）
    pub async fn pending_count(&self) -> usize {
        let guard = self.channels.lock().await;
        guard.len()
    }
}