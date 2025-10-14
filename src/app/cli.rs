use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// 认证 Token (与 --access-key 二选一)
    #[arg(short, long, requires = "uid")]
    pub token: Option<String>,

    /// 用户ID (必需，用于直接提供 token 或与 access_key 配合)
    #[arg(long)]
    pub uid: u32,

    /// 访问密钥 (与 --token 二选一)
    #[arg(long, requires = "uid")]
    pub access_key: Option<String>,

    /// WebSocket端点URL (可选，默认为官方端点)
    #[arg(long)]
    pub ws_url: Option<String>,

    /// 要绘制的PNG图片路径
    #[arg(short, long)]
    pub image: PathBuf,

    /// 图片在画板上的起始X坐标 (可选，默认为0)
    #[arg(short, long, default_value_t = 0)]
    pub x: i32,

    /// 图片在画板上的起始Y坐标 (可选，默认为0)
    #[arg(short, long, default_value_t = 0)]
    pub y: i32,

    /// 图片绘制时的宽度 (可选，默认为原图宽度)
    #[arg(long)]
    pub width: Option<u32>,

    /// 图片绘制时的高度 (可选，默认为原图高度)
    #[arg(long)]
    pub height: Option<u32>,
    
    /// 绘制每个像素之间的延迟（毫秒）(可选，默认为10毫秒，减少此值会增加发送速率)
    #[arg(long, default_value_t = 10)]
    pub delay: u64,
    
    /// 使用批量绘制模式（粘包发送）(可选，默认为启用)
    #[arg(long, default_value_t = true)]
    pub batch_mode: bool,
    
    /// 批量模式下每次发送的最大像素数量 (可选，默认为50)
    #[arg(long, default_value_t = 50)]
    pub max_batch_size: usize,
    
    /// 是否循环绘制图片 (可选，默认为禁用)
    #[arg(long, default_value_t = false)]
    pub loop_draw: bool,
    
    /// 循环绘制的时间间隔（毫秒）(可选，默认为60000毫秒，即60秒)
    #[arg(long, default_value_t = 60000)]
    pub loop_interval: u64,

    /// 渐进式绘制模式 (可选，'none', 'chessboard', 'scale')
    #[arg(long, default_value = "none")]
    pub progressive: String,

    /// 是否启用本地绘版数据同步 (可选，默认为禁用)
    #[arg(long, default_value_t = false)]
    pub enable_local_sync: bool,

    /// 本地绘版数据同步间隔（秒）(可选，默认为300秒，即5分钟)
    #[arg(long, default_value_t = 300)]
    pub sync_interval: u64,

    /// 是否启用绘版图片导出功能 (可选，默认为禁用)
    #[arg(long, default_value_t = false)]
    pub enable_export: bool,

    /// 图片导出间隔（秒）(可选，默认为600秒，即10分钟)
    #[arg(long, default_value_t = 600)]
    pub export_interval: u64,

    /// 图片导出目录 (可选，默认为当前目录下的exports文件夹)
    #[arg(long, default_value = "exports")]
    pub export_dir: String,
}