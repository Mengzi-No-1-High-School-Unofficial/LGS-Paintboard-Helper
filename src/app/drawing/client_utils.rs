// 模块: src/app/drawing/client_utils.rs
//! 客户端创建工具（从原 drawing.rs 提取）

use winter_paintboard_sdk::{
    config::Config, create_client_by_type, ClientType, PaintboardClientTrait,
};

/// 创建一个 Paintboard 客户端（根据配置和客户端类型）
pub async fn create_client(
    config: Config,
    client_type: ClientType,
) -> Result<Box<dyn PaintboardClientTrait + Send>, Box<dyn std::error::Error>> {
    Ok(create_client_by_type(config, client_type).await?)
}
