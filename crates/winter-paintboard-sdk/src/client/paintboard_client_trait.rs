use crate::{
    error::PaintboardError, 
    models::{Board, Rgb, Pos, PaintResult}, 
    config::Config
};
use async_trait::async_trait;

/// 定义 Paintboard 客户端的通用接口
/// 所有客户端实现（基础客户端、连接池客户端等）都需要实现此 trait
#[async_trait]
pub trait PaintboardClientTrait {
    /// 创建一个新的客户端实例
    async fn new(config: Config) -> Result<Self, PaintboardError> 
    where 
        Self: Sized;

    /// 设置认证信息（UID 和 token）
    fn set_auth(&mut self, uid: u32, token: String);

    /// 获取当前画板数据
    async fn get_board(&self) -> Result<Board, PaintboardError>;

    /// 通过 UID 和 access_key 获取 token
    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError>;

    /// 在指定位置绘制像素
    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError>;

    /// 批量绘制多个像素
    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError>;
}