use crate::{config::Config, error::PaintboardError, event::EventBus};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio::net::TcpStream;
use url::Url;
use tracing::{debug, error};
use futures::SinkExt; // bring SinkExt into scope so WebSocketStream::send is available

/// WsConnection 封装底层 WebSocket 连接的生命周期与发送接口。
/// 该类型是轻量的，可由 WsProvider 或其他上层协调者持有 Arc 引用。
pub struct WsConnection {
    config: Arc<Config>,
    stream: Arc<TokioMutex<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>>,
}

impl WsConnection {
    /// 创建一个新的 WsConnection（初始处于未连接状态）
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            stream: Arc::new(TokioMutex::new(None)),
        }
    }

    /// Expose internal stream Arc for use by background tasks
    pub fn stream(&self) -> Arc<TokioMutex<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>> {
        self.stream.clone()
    }

    /// 建立连接并替换当前连接（如果成功）
    pub async fn connect(&self) -> Result<(), PaintboardError> {
        let mut url = Url::parse(&self.config.ws_url).map_err(|e| {
            debug!("URL 解析失败: {}", e);
            PaintboardError::InvalidUrl(e.to_string())
        })?;

        // 根据连接模式添加查询参数
        match self.config.connection_mode {
            crate::config::ConnectionMode::ReadOnly => {
                url.set_query(Some("readonly=1"));
            }
            crate::config::ConnectionMode::WriteOnly => {
                url.set_query(Some("writeonly=1"));
            }
            crate::config::ConnectionMode::ReadWrite => {
                // 默认模式，不需要参数
            }
        }

        debug!("连接到 WebSocket: {}", url);
        match connect_async(url).await {
            Ok((ws_stream, _)) => {
                debug!("WebSocket 连接建立成功");
                let mut guard = self.stream.lock().await;
                *guard = Some(ws_stream);
                drop(guard);

                // 发送连接打开事件到事件总线
                let event_bus = EventBus::global();
                let _ = event_bus.send(crate::event::Event::ConnectionOpened);

                Ok(())
            }
            Err(e) => {
                debug!("WebSocket 连接失败: {}", e);
                let event_bus = EventBus::global();
                let _ = event_bus.send(crate::event::Event::error_event(format!(
                    "WebSocket connection failed: {}",
                    e
                )));
                Err(PaintboardError::websocket(e.to_string()))
            }
        }
    }

    /// 非阻塞检查当前是否有连接
    pub async fn is_connected(&self) -> bool {
        let guard = self.stream.lock().await;
        guard.is_some()
    }

    /// 发送二进制消息（将错误封装为 PaintboardError）
    pub async fn send_binary(&self, data: Vec<u8>) -> Result<(), PaintboardError> {
        let mut guard = self.stream.lock().await;
        let ws_stream = guard.as_mut().ok_or(PaintboardError::ConnectionClosed)?;

        match ws_stream.send(Message::Binary(data)).await {
            Ok(_) => Ok(()),
            Err(e) => {
                error!("发送二进制消息失败: {}", e);
                let event_bus = EventBus::global();
                let _ = event_bus.send(crate::event::Event::error_event(format!(
                    "Failed to send binary message: {}",
                    e
                )));
                Err(PaintboardError::websocket(e.to_string()))
            }
        }
    }

    /// 关闭连接（优雅关闭，如果没有连接则直接返回 Ok）
    pub async fn close(&self) -> Result<(), PaintboardError> {
        let mut guard = self.stream.lock().await;
        if let Some(mut ws_stream) = guard.take() {
            use tokio_tungstenite::tungstenite::protocol::{frame::coding::CloseCode, CloseFrame};
            let _ = ws_stream
                .close(Some(CloseFrame {
                    code: CloseCode::Normal,
                    reason: std::borrow::Cow::Borrowed("Client disconnecting"),
                }))
                .await;
        }
        Ok(())
    }
}