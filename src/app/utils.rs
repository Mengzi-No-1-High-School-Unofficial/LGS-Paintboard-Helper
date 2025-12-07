//! 应用程序实用工具模块
//!
//! 该模块包含各种实用函数，如获取认证Token、验证认证参数等。

use log::info;
use winter_paintboard_sdk::{config::Config, get_global_client, PaintboardClientTrait};

use color_eyre::Report;
use winter_paintboard_sdk::Rgb;

/// 通过访问密钥获取认证 Token
///
/// 使用用户ID和访问密钥向服务器请求认证Token
///
/// # 参数
///
/// * `uid` - 用户ID
/// * `access_key` - 访问密钥
///
/// # 返回值
///
/// * `Ok(String)` - 成功获取的认证Token
/// * `Err` - 获取过程中发生错误
pub async fn get_token_with_access_key(uid: u32, access_key: &str) -> Result<String, Report> {
    info!("正在使用 UID 和访问密钥获取 Token...");
    let config = Config::default();
    let http_client = get_global_client(config).await?;
    let token = http_client.get_token(uid, access_key).await;

    if let Err(e) = token {
        tracing::error!("获取 Token 失败: {:?}", e);
        return Err(e.into());
    }

    let token = token.unwrap();

    info!("成功获取 Token: {}...", &token[..8]); // 显示开头部分
    Ok(token)
}

/// 验证认证参数
///
/// 验证提供的Token和访问密钥参数是否有效
///
/// # 参数
///
/// * `token` - 可选的认证Token
/// * `access_key` - 可选的访问密钥
///
/// # 返回值
///
/// * `Ok(String)` - 验证通过的Token
/// * `Err` - 验证失败的原因
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

/// 解析认证Token，如果提供了访问密钥则自动获取Token
///
/// 根据提供的参数解析出有效的认证Token，如果只提供了访问密钥则自动获取Token
///
/// # 参数
///
/// * `token` - 可选的预获取Token
/// * `uid` - 用户ID
/// * `access_key` - 可选的访问密钥
///
/// # 返回值
///
/// * `Ok(String)` - 解析出的有效Token
/// * `Err` - 解析过程中发生错误
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
