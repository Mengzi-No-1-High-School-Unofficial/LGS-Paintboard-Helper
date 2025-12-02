//! 命令行参数解析模块
//!
//! 该模块定义了应用程序的命令行参数结构，使用 clap 库进行参数解析。
//! 支持多种子命令，包括获取画板状态、显示项目信息和多Token绘制模式。

use clap::{Parser, Subcommand};

/// 应用程序的根命令行参数结构
///
/// 包含所有可用的子命令选项，通过 #[command(subcommand)] 属性指定
/// 可用的子命令枚举类型 Commands。
#[derive(Parser)]
#[command(
    author = "Xyber Nova <xyber-nova@outlook.com>",
    version,
    about = "洛谷保存站冬日绘板绘制工具",
    long_about = r#"
LGS Winter Paintboard Helper

一个功能强大的命令行工具，用于在洛谷画板上绘制图像。
支持多种绘制模式，包括增量修改、循环绘制和单次绘制。

Developed by `https://github.com/Mengzi-No-1-High-School-Unofficial`
    "#
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// 获取当前画板状态
    GetBoard {
        /// 认证 Token (与 --access-key 二选一)
        #[arg(short, long, requires = "uid")]
        token: Option<String>,

        /// 用户ID (必需，用于直接提供 token 或与 access_key 配合)
        #[arg(long)]
        uid: u32,

        /// 访问密钥 (与 --token 二选一)
        #[arg(long, requires = "uid")]
        access_key: Option<String>,

        /// API基础URL (可选，默认为官方端点)
        #[arg(long)]
        api_url: Option<String>,

        /// 输出文件路径 (可选，默认输出到控制台)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// 显示项目信息和作者信息
    About,

    /// 多 Token CD 模式：使用多个 Token 并发绘制
    MultiToken {
        /// Token 配置文件路径（JSON 格式）
        #[arg(short, long)]
        config: std::path::PathBuf,

        /// 或者直接通过命令行提供 access keys（逗号分隔）
        #[arg(long, conflicts_with = "config")]
        access_keys: Option<String>,

        /// 对应的 UIDs（逗号分隔，与 access_keys 对应）
        #[arg(long, requires = "access_keys")]
        uids: Option<String>,

        /// CD 时间（毫秒）
        #[arg(long, default_value_t = 3000)]
        cd_time: u64,

        /// WebSocket 端点 URL
        #[arg(long)]
        ws_url: Option<String>,

        /// 要绘制的图片路径
        #[arg(short, long)]
        image: std::path::PathBuf,

        /// 起始 X 坐标
        #[arg(short, long, default_value_t = 0)]
        x: i32,

        /// 起始 Y 坐标
        #[arg(short, long, default_value_t = 0)]
        y: i32,

        /// 图片宽度
        #[arg(long)]
        width: Option<u32>,

        /// 图片高度
        #[arg(long)]
        height: Option<u32>,

        /// 比对间隔（毫秒）
        #[arg(long, default_value_t = 5000)]
        comparison_interval: u64,

        /// 启用绘版图片导出功能
        #[arg(long, default_value_t = false)]
        enable_export: bool,

        /// 启用热点图导出功能
        #[arg(long, default_value_t = false)]
        enable_heatmap_export: bool,

        /// 导出目录路径
        #[arg(long, default_value = "exports")]
        export_dir: String,

        /// 导出间隔（秒）
        #[arg(long, default_value_t = 30)]
        export_interval: u64,
        
        /// Canny 边缘检测低阈值
        #[arg(long, default_value_t = 20.0)]
        canny_low_thresh: f32,
        
        /// Canny 边缘检测高阈值
        #[arg(long, default_value_t = 40.0)]
        canny_high_thresh: f32,

        /// 惩罚系数
        #[arg(long, default_value_t = 60000.0)]
        penalty_scale: f32,

        /// 每个批处理的大小
        #[arg(long, default_value_t = 100)]
        batch_size: usize,
    },
}

fn parse_client_type(s: &str) -> Result<winter_paintboard_sdk::ClientType, String> {
    match s.to_lowercase().as_str() {
        "pool" | "connection_pool" => unimplemented!("Connection Pool 不存在"),
        "basic" => Ok(winter_paintboard_sdk::ClientType::Basic),
        _ => Err(format!("Invalid client type: {}", s)),
    }
}
