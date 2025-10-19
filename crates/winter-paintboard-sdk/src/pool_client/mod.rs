//! `pool_client` 模块提供了基于连接池的客户端实现。
//! 使用 deadpool 管理多个 WriteOnly 连接，提供高效的并发绘制能力。

mod manager;
mod pool;

pub use manager::WriteOnlyManager;
pub use pool::WriteOnlyPool;

use crate::{
    basic_client::BasicClient,
    config::{Config, ConnectionMode},
    error::PaintboardError,
    models::{Board, PaintResult, Pos, Rgb},
    PaintboardClientTrait,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

/// PoolClient 基于连接池的客户端实现
/// 提供读写分离的连接管理：
/// - 读操作使用独立的 ReadOnly 连接
/// - 写操作从 WriteOnly 连接池获取连接
pub struct PoolClient {
    /// WriteOnly 连接池，用于绘制操作
    write_pool: WriteOnlyPool,
    /// ReadOnly 连接，用于获取画板数据
    read_client: Arc<Mutex<BasicClient>>,
    /// 共享配置
    config: Arc<Config>,
    /// 默认认证信息存储（用于便利操作）
    default_auth: Arc<Mutex<Option<(u32, String)>>>,
}

impl PoolClient {
    /// 创建新的 PoolClient 实例
    pub async fn new(config: Config) -> Result<Self, PaintboardError> {
        let config = Arc::new(config);

        // 创建 WriteOnly 连接池
        let write_pool = WriteOnlyPool::new(config.clone()).await?;

        // 创建 ReadOnly 连接
        let mut read_config = (*config).clone();
        read_config.connection_mode = ConnectionMode::ReadOnly;
        read_config.ws_url = format!("{}?readonly=1", read_config.ws_url);

        let read_client = Arc::new(Mutex::new(BasicClient::new_impl(read_config).await?));

        Ok(Self {
            write_pool,
            read_client,
            config,
            default_auth: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn get_write_conn(
        &self,
    ) -> Result<deadpool::managed::Object<WriteOnlyManager>, PaintboardError> {
        self.write_pool.get().await
    }
}

#[async_trait]
impl PaintboardClientTrait for PoolClient {
    async fn new(config: Config) -> Result<Self, PaintboardError>
    where
        Self: Sized,
    {
        Self::new(config).await
    }

    fn set_auth(&mut self, uid: u32, token: String) {
        // 同步设置默认认证信息，避免竞态条件
        let auth = self.default_auth.clone();
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                *auth.lock().await = Some((uid, token));
            })
        });
    }

    async fn get_board(&self) -> Result<Board, PaintboardError> {
        let client = self.read_client.lock().await;
        client.get_board().await
    }

    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError> {
        let client = self.read_client.lock().await;
        client.get_token(uid, access_key).await
    }

    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError> {
        // 从连接池获取连接
        let mut conn = self.write_pool.get().await?;

        // 获取默认认证信息
        if let Some((uid, token)) = self.default_auth.lock().await.clone() {
            // 使用认证信息绘制
            conn.paint_with_auth(pos, color, uid, &token).await
        } else {
            Err(PaintboardError::auth(
                "Authentication required. Use paint_with_auth or set default auth first."
                    .to_string(),
            ))
        }
    }

    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError> {
        // 从连接池获取连接
        let mut conn = self.write_pool.get().await?;

        // 获取默认认证信息
        if let Some((uid, token)) = self.default_auth.lock().await.clone() {
            // 使用认证信息批量绘制
            conn.paint_batch_with_auth(operations, uid, &token).await
        } else {
            Err(PaintboardError::auth(
                "Authentication required. Use paint_batch_with_auth or set default auth first."
                    .to_string(),
            ))
        }
    }

    async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<PaintResult, PaintboardError> {
        // 从连接池获取连接
        let mut conn = self.write_pool.get().await?;

        // 使用临时 token 绘制
        conn.paint_with_auth(pos, color, uid, &token).await
    }

    fn get_config(&self) -> &Config {
        &self.config
    }
}
