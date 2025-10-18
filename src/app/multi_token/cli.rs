use std::path::PathBuf;

/// 多 Token CD 模式：使用多个 Token 并发绘制
#[derive(clap::Args)]
pub struct MultiTokenArgs {
    /// Token 配置文件路径（JSON 格式）
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    
    /// 或者直接通过命令行提供 access keys（逗号分隔）
    #[arg(long, conflicts_with = "config")]
    pub access_keys: Option<String>,
    
    /// 对应的 UIDs（逗号分隔，与 access_keys 对应）
    #[arg(long, requires = "access_keys")]
    pub uids: Option<String>,
    
    /// CD 时间（毫秒）
    #[arg(long, default_value_t = 3000)]
    pub cd_time: u64,
    
    /// WebSocket 端点 URL
    #[arg(long)]
    pub ws_url: Option<String>,
    
    /// 要绘制的图片路径
    #[arg(short, long)]
    pub image: PathBuf,
    
    /// 起始 X 坐标
    #[arg(short, long, default_value_t = 0)]
    pub x: i32,
    
    /// 起始 Y 坐标
    #[arg(short, long, default_value_t = 0)]
    pub y: i32,
    
    /// 图片宽度
    #[arg(long)]
    pub width: Option<u32>,
    
    /// 图片高度
    #[arg(long)]
    pub height: Option<u32>,
    
    /// 比对间隔（毫秒）
    #[arg(long, default_value_t = 5000)]
    pub comparison_interval: u64,
}