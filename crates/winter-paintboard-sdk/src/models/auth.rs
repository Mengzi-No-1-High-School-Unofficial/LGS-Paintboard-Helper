use serde::{Deserialize, Serialize};

/// 获取认证令牌的请求体。
#[derive(Debug, Serialize)]
pub struct AuthRequest {
    /// 用户的唯一标识符 (UID)。
    pub uid: u32,
    /// 用户的访问密钥。
    pub access_key: String,
}

/// 认证令牌响应的内部数据结构。
/// 包含了实际的认证令牌。
#[derive(Debug, Deserialize)]
pub struct TokenData {
    /// 用户的认证令牌。
    pub token: String,
}

/// 获取认证令牌的完整响应结构。
/// 包含了状态码和令牌数据。
#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    /// HTTP 状态码，例如 200 表示成功。
    #[serde(rename = "code")]
    pub status_code: i32,
    /// 包含实际令牌的 `TokenData` 结构。
    pub data: TokenData,
}
