use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Winter Paintboard CLI client - 在洛谷画板上绘制图像的工具
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
    /// 增量修改模式：持续监控并更新画板
    Incremental {
        /// 认证 Token (与 --access-key 二选一)
        #[arg(short, long, requires = "uid")]
        token: Option<String>,

        /// 用户ID (必需，用于直接提供 token 或与 access_key 配合)
        #[arg(long)]
        uid: u32,

        /// 访问密钥 (与 --token 二选一)
        #[arg(long, requires = "uid")]
        access_key: Option<String>,

        /// WebSocket端点URL (可选，默认为官方端点)
        #[arg(long)]
        ws_url: Option<String>,

        /// 要绘制的PNG图片路径
        #[arg(short, long)]
        image: PathBuf,

        /// 图片在画板上的起始X坐标 (可选，默认为0)
        #[arg(short, long, default_value_t = 0)]
        x: i32,

        /// 图片在画板上的起始Y坐标 (可选，默认为0)
        #[arg(short, long, default_value_t = 0)]
        y: i32,

        /// 图片绘制时的宽度 (可选，默认为原图宽度)
        #[arg(long)]
        width: Option<u32>,

        /// 图片绘制时的高度 (可选，默认为原图高度)
        #[arg(long)]
        height: Option<u32>,

        /// 监控间隔时间（毫秒）(可选，默认为200毫秒)
        #[arg(long, default_value_t = 200)]
        monitor_interval: u64,

        /// 恢复像素时的延迟（毫秒）(可选，默认为1毫秒)
        #[arg(long, default_value_t = 1)]
        restore_delay: u64,

        /// 批量模式下每次发送的最大像素数量 (可选，默认为10240)
        #[arg(long, default_value_t = 10240)]
        max_batch_size: usize,

        /// 客户端类型 (可选，'basic' 或 'pool'，默认为 'pool')
        #[arg(long, default_value = "pool", value_parser = parse_client_type)]
        client_type: winter_paintboard_sdk::ClientType,
    },

    /// 循环绘制模式：按周期重复绘制
    DrawLoop {
        /// 认证 Token (与 --access-key 二选一)
        #[arg(short, long, requires = "uid")]
        token: Option<String>,

        /// 用户ID (必需，用于直接提供 token 或与 access_key 配合)
        #[arg(long)]
        uid: u32,

        /// 访问密钥 (与 --token 二选一)
        #[arg(long, requires = "uid")]
        access_key: Option<String>,

        /// WebSocket端点URL (可选，默认为官方端点)
        #[arg(long)]
        ws_url: Option<String>,

        /// 要绘制的PNG图片路径
        #[arg(short, long)]
        image: PathBuf,

        /// 图片在画板上的起始X坐标 (可选，默认为0)
        #[arg(short, long, default_value_t = 0)]
        x: i32,

        /// 图片在画板上的起始Y坐标 (可选，默认为0)
        #[arg(short, long, default_value_t = 0)]
        y: i32,

        /// 图片绘制时的宽度 (可选，默认为原图宽度)
        #[arg(long)]
        width: Option<u32>,

        /// 图片绘制时的高度 (可选，默认为原图高度)
        #[arg(long)]
        height: Option<u32>,

        /// 绘制每个像素之间的延迟（毫秒）(可选，默认为10毫秒，减少此值会增加发送速率)
        #[arg(long, default_value_t = 10)]
        delay: u64,

        /// 使用批量绘制模式（粘包发送）(可选，默认为启用)
        #[arg(long, default_value_t = true)]
        batch_mode: bool,

        /// 批量模式下每次发送的最大像素数量 (可选，默认为10240)
        #[arg(long, default_value_t = 10240)]
        max_batch_size: usize,

        /// 循环绘制的时间间隔（毫秒）(可选，默认为10000毫秒，即10秒)
        #[arg(long, default_value_t = 10000)]
        loop_interval: u64,

        /// 渐进式绘制模式 (可选，'none', 'chessboard', 'scale')
        #[arg(long, default_value = "chessboard")]
        progressive: String,

        /// 客户端类型 (可选，'basic' 或 'pool'，默认为 'pool')
        #[arg(long, default_value = "pool", value_parser = parse_client_type)]
        client_type: winter_paintboard_sdk::ClientType,
    },

    /// 单次绘制模式：执行一次绘制操作
    DrawOnce {
        /// 认证 Token (与 --access-key 二选一)
        #[arg(short, long, requires = "uid")]
        token: Option<String>,

        /// 用户ID (必需，用于直接提供 token 或与 access_key 配合)
        #[arg(long)]
        uid: u32,

        /// 访问密钥 (与 --token 二选一)
        #[arg(long, requires = "uid")]
        access_key: Option<String>,

        /// WebSocket端点URL (可选，默认为官方端点)
        #[arg(long)]
        ws_url: Option<String>,

        /// 要绘制的PNG图片路径
        #[arg(short, long)]
        image: PathBuf,

        /// 图片在画板上的起始X坐标 (可选，默认为0)
        #[arg(short, long, default_value_t = 0)]
        x: i32,

        /// 图片在画板上的起始Y坐标 (可选，默认为0)
        #[arg(short, long, default_value_t = 0)]
        y: i32,

        /// 图片绘制时的宽度 (可选，默认为原图宽度)
        #[arg(long)]
        width: Option<u32>,

        /// 图片绘制时的高度 (可选，默认为原图高度)
        #[arg(long)]
        height: Option<u32>,

        /// 绘制每个像素之间的延迟（毫秒）(可选，默认为100毫秒，减少此值会增加发送速率)
        #[arg(long, default_value_t = 100)]
        delay: u64,

        /// 使用批量绘制模式（粘包发送）(可选，默认为启用)
        #[arg(long, default_value_t = true)]
        batch_mode: bool,

        /// 批量模式下每次发送的最大像素数量 (可选，默认为10240)
        #[arg(long, default_value_t = 10240)]
        max_batch_size: usize,

        /// 渐进式绘制模式 (可选，'none', 'chessboard', 'scale')
        #[arg(long, default_value = "chessboard")]
        progressive: String,

        /// 是否等待所有操作完成
        #[arg(long)]
        wait_for_completion: bool,

        /// 客户端类型 (可选，'basic' 或 'pool'，默认为 'pool')
        #[arg(long, default_value = "pool", value_parser = parse_client_type)]
        client_type: winter_paintboard_sdk::ClientType,
    },

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
    },
}

fn parse_client_type(s: &str) -> Result<winter_paintboard_sdk::ClientType, String> {
    match s.to_lowercase().as_str() {
        "pool" | "connection_pool" => unimplemented!("Connection Pool 不存在"),
        "basic" => Ok(winter_paintboard_sdk::ClientType::Basic),
        _ => Err(format!("Invalid client type: {}", s)),
    }
}
