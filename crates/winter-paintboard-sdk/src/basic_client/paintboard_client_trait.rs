use crate::{
    config::Config,
    error::PaintboardError,
    models::{Board, PaintResult, Pos, Rgb},
};
use async_trait::async_trait;

/// `PaintboardClientTrait` 定义了与 Winter Paintboard API 交互的通用客户端接口。
/// 所有具体的客户端实现（例如 `BasicClient` 和 `PoolClient`）都必须实现此 trait，
/// 以提供统一的 API 访问方式。
#[async_trait]
pub trait PaintboardClientTrait {
    /// 创建一个新的客户端实例。
    ///
    /// 此异步函数负责根据提供的配置初始化客户端，并执行任何必要的设置。
    ///
    /// # 参数
    /// - `config`: 客户端的配置，通常包含 API 端点、超时设置等。
    ///
    /// # 返回
    /// `Result`，成功时包含实现了此 trait 的客户端实例，失败时包含 `PaintboardError`。
    async fn new(config: Config) -> Result<Self, PaintboardError>
    where
        Self: Sized;

    /// 设置客户端的认证信息 (deprecated - use methods with auth parameters)
    #[deprecated(note = "Use methods that accept auth parameters instead")]
    fn set_auth(&mut self, uid: u32, token: String);

    /// 获取当前画板的完整数据。
    ///
    /// 此异步函数将从 API 获取整个画板的像素数据。
    ///
    /// # 返回
    /// `Result`，成功时包含 `Board` 结构体，其中包含画板的宽度、高度和像素数据，
    /// 失败时包含 `PaintboardError`。
    async fn get_board(&self) -> Result<Board, PaintboardError>;

    /// 通过用户 ID (UID) 和访问密钥 (access_key) 获取认证令牌。
    ///
    /// 此异步函数通常用于首次认证以获取会话令牌。
    ///
    /// # 参数
    /// - `uid`: 用户的唯一标识符。
    /// - `access_key`: 用于验证用户身份的密钥。
    ///
    /// # 返回
    /// `Result`，成功时包含一个 `String` 类型的认证令牌，
    /// 失败时包含 `PaintboardError` (例如，认证失败或网络问题)。
    async fn get_token(&self, uid: u32, access_key: &str) -> Result<String, PaintboardError>;

    /// 在画板的指定位置绘制一个像素 (deprecated - use paint_with_auth)
    #[deprecated(note = "Use paint_with_auth instead")]
    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError>;

    /// 批量绘制多个像素 (deprecated - use paint_batch_with_auth)
    #[deprecated(note = "Use paint_batch_with_auth instead")]
    async fn paint_batch(&mut self, operations: Vec<(Pos, Rgb)>) -> Result<(), PaintboardError>;

    /// 获取客户端配置信息。
    ///
    /// # 返回
    /// `Config` 的引用，用于获取连接信息等。
    fn get_config(&self) -> &Config;

    /// 使用临时 Token 绘制像素（不修改客户端状态）。
    ///
    /// # 参数
    /// - `pos`: 像素位置。
    /// - `color`: 像素颜色。
    /// - `uid`: 临时 UID。
    /// - `token`: 临时认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，失败时包含 `PaintboardError`。
    async fn paint_with_token(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: String,
    ) -> Result<crate::models::PaintResult, PaintboardError>;

    /// 使用提供的认证信息绘制一个像素 (新方法)
    ///
    /// # 参数
    /// - `pos`: 像素位置。
    /// - `color`: 像素颜色。
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，失败时包含 `PaintboardError`。
    async fn paint_with_auth(
        &mut self,
        pos: Pos,
        color: Rgb,
        uid: u32,
        token: &str,
    ) -> Result<PaintResult, PaintboardError> {
        // 默认实现：如果实现者没有提供此方法，则返回错误
        Err(PaintboardError::auth(
            "paint_with_auth not implemented".to_string(),
        ))
    }

    /// 使用提供的认证信息批量绘制像素 (新方法)
    ///
    /// # 参数
    /// - `operations`: 包含位置和颜色的操作向量。
    /// - `uid`: 用户ID。
    /// - `token`: 认证令牌。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()`，失败时包含 `PaintboardError`。
    async fn paint_batch_with_auth(
        &mut self,
        operations: Vec<(Pos, Rgb)>,
        uid: u32,
        token: &str,
    ) -> Result<(), PaintboardError> {
        // 默认实现：如果实现者没有提供此方法，则返回错误
        Err(PaintboardError::auth(
            "paint_batch_with_auth not implemented".to_string(),
        ))
    }
}
