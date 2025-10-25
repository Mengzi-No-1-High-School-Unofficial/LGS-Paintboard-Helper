use log::{error, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use winter_paintboard_sdk::{config::Config, BasicClient, ClientType, PaintboardClientTrait};

use winter_paintboard_sdk::Rgb;

/// 计算两个 RGB 颜色之间的差异（欧几里得距离）
pub fn calculate_color_difference(color1: &Rgb, color2: &Rgb) -> f64 {
    let dr = (color1.r as i32 - color2.r as i32) as f64;
    let dg = (color1.g as i32 - color2.g as i32) as f64;
    let db = (color1.b as i32 - color2.b as i32) as f64;

    ((dr * dr + dg * dg + db * db) / 3.0).sqrt()
}

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

/// 处理认证参数，返回有效的token
pub async fn resolve_auth_token(
    token: Option<String>,
    uid: u32,
    access_key: Option<String>,
) -> Result<String, Box<dyn std::error::Error>> {
    let token = match validate_auth_args(&token, &access_key) {
        Ok(token) => {
            if token.is_empty() {
                if let Some(access_key) = &access_key {
                    get_token_with_access_key(uid, access_key).await?
                } else {
                    return Err("错误: 必须提供 --token 或 --access_key".into());
                }
            } else {
                token
            }
        }
        Err(e) => {
            return Err(e.into());
        }
    };
    Ok(token)
}