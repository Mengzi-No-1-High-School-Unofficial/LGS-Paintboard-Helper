use log::info;
use winter_paintboard_sdk::{config::Config, BasicClient, PaintboardClientTrait};

/// Gets a token using UID and access key
pub async fn get_token_with_access_key(
    uid: u32,
    access_key: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    info!("正在使用 UID 和访问密钥获取 Token...");
    let config = Config::default();
    let http_client = BasicClient::new(config).await?;
    let token = http_client.get_token(uid, access_key).await?;
    info!("成功获取 Token: {}...", &token[..8]); // 显示开头部分
    Ok(token)
}

/// Validates that either token or access key is provided
pub fn validate_auth_args(
    token: &Option<String>,
    access_key: &Option<String>,
) -> Result<String, &'static str> {
    if let Some(access_key) = access_key {
        if access_key.is_empty() {
            return Err("Access key cannot be empty");
        }
    }

    if let Some(token) = token {
        if token.is_empty() {
            return Err("Token cannot be empty");
        }
        return Ok(token.clone());
    }

    if access_key.is_some() {
        return Ok(String::new()); // Will be obtained later
    }

    Err("必须提供 --token 或 --access-key")
}
