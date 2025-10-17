use crate::{
    config::Config, error::PaintboardError, BasicClient, PaintboardClientTrait, PoolClient,
};
use async_trait::async_trait;

/// 客户端类型枚举
#[derive(Debug, Clone, Copy)]
pub enum ClientType {
    Basic,          // 基础单连接客户端
    ConnectionPool, // 连接池客户端
}

/// 便捷函数：根据类型创建客户端
pub async fn create_client_by_type(
    config: Config,
    client_type: ClientType,
) -> Result<Box<dyn PaintboardClientTrait + Send>, PaintboardError> {
    match client_type {
        ClientType::Basic => {
            let client = BasicClient::new(config).await?;
            Ok(Box::new(client))
        }
        ClientType::ConnectionPool => {
            let client = PoolClient::new(config, 2, 7).await?; // 默认最小4个，最大7个连接
            Ok(Box::new(client))
        }
    }
}
