use serde::{Deserialize, Serialize};
use std::path::Path;

/// Token 配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenConfig {
    /// CD 时间（毫秒）
    pub cd_time_ms: u64,
    /// Token 列表
    pub tokens: Vec<TokenEntry>,
}

/// Token 条目
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenEntry {
    /// 用户 UID
    pub uid: u32,
    /// 访问密钥（可选，如果提供则自动获取 token）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    /// 预获取的 Token（可选，如果提供则直接使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl TokenConfig {
    /// 从文件加载配置
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: TokenConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// 从命令行参数构建配置
    pub fn from_cli_args(
        access_keys: String,
        uids: String,
        cd_time: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let keys: Vec<&str> = access_keys.split(',').collect();
        let uids: Vec<u32> = uids
            .split(',')
            .map(|s| s.trim().parse())
            .collect::<Result<Vec<_>, _>>()?;

        if keys.len() != uids.len() {
            return Err("access_keys 和 uids 数量不匹配".into());
        }

        let tokens = keys
            .into_iter()
            .zip(uids.into_iter())
            .map(|(key, uid)| TokenEntry {
                uid,
                access_key: Some(key.to_string()),
                token: None,
            })
            .collect();

        Ok(TokenConfig {
            cd_time_ms: cd_time,
            tokens,
        })
    }
}

/// 优先级像素 - 带优先级的绘制任务
#[derive(Debug, Clone)]
pub struct PriorityPixel {
    pub pos: winter_paintboard_sdk::models::Pos,
    pub color: winter_paintboard_sdk::models::Rgb,
    pub priority: f64, // 颜色差异值，越大越优先
}

impl PartialEq for PriorityPixel {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PriorityPixel {}

impl PartialOrd for PriorityPixel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // 优先级高的排在前面（大顶堆）
        other.priority.partial_cmp(&self.priority)
    }
}

impl Ord for PriorityPixel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .priority
            .partial_cmp(&self.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}
