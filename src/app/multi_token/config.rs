//! 多Token模式配置模块
//!
//! 该模块定义了多Token模式的配置结构，包括Token列表、CD时间等参数，
//! 以及优先级像素结构用于网格图算法。

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Token 配置结构
///
/// 包含多Token模式运行所需的所有配置参数，包括CD时间和Token列表
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenConfig {
    /// CD 时间（毫秒）
    ///
    /// 每个Token的冷却时间，用于避免频率限制错误
    pub cd_time_ms: u64,
    /// Token 列表
    ///
    /// 包含所有可用的Token条目
    pub tokens: Vec<TokenEntry>,
}

/// Token 条目结构
///
/// 定义单个Token的配置信息，可以使用访问密钥或预获取的Token
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenEntry {
    /// 用户 UID
    ///
    /// 用户的唯一标识符
    pub uid: u32,
    /// 访问密钥（可选，如果提供则自动获取 token）
    ///
    /// 如果提供访问密钥，程序会自动获取对应的Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    /// 预获取的 Token（可选，如果提供则直接使用）
    ///
    /// 如果提供预获取的Token，程序会直接使用该Token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

impl TokenConfig {
    /// 从文件加载配置
    ///
    /// 从指定路径的JSON文件中加载Token配置
    ///
    /// # 参数
    ///
    /// * `path` - 配置文件路径
    ///
    /// # 返回值
    ///
    /// * `Ok(TokenConfig)` - 成功加载的配置
    /// * `Err` - 加载过程中发生错误
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: TokenConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// 从命令行参数构建配置
    ///
    /// 根据命令行提供的访问密钥、用户ID和CD时间构建Token配置
    ///
    /// # 参数
    ///
    /// * `access_keys` - 逗号分隔的访问密钥字符串
    /// * `uids` - 逗号分隔的用户ID字符串
    /// * `cd_time` - CD时间（毫秒）
    ///
    /// # 返回值
    ///
    /// * `Ok(TokenConfig)` - 构建的配置
    /// * `Err` - 构建过程中发生错误
    #[allow(dead_code)]
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
            .zip(uids)
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
///
/// 表示一个带优先级的绘制任务，用于网格图算法中确定绘制顺序
#[derive(Debug, Clone)]
pub struct PriorityPixel {
    /// 像素位置
    pub pos: winter_paintboard_sdk::models::Pos,
    /// 像素颜色
    pub color: winter_paintboard_sdk::models::Rgb,
    /// 优先级值
    ///
    /// 网格图算法计算出的优先级，值越大越优先绘制
    pub priority: f64,
}

impl PartialEq for PriorityPixel {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl Eq for PriorityPixel {}

impl PartialOrd for PriorityPixel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityPixel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // 正向排序: priority 值越大,优先级越高
        // BinaryHeap 是最大堆,所以 self.priority > other.priority 时返回 Greater
        self.priority
            .partial_cmp(&other.priority)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}
