use crate::{
    config::Config, error::PaintboardError, BasicClient, PaintboardClientTrait
};
use async_trait::async_trait;

/// 客户端类型枚举
#[derive(Debug, Clone, Copy)]
pub enum ClientType {
    Basic,          // 基础单连接客户端
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
    }
}
