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

    /// 设置客户端的认证信息。
    ///
    /// 调用此方法将更新客户端的用户 ID (UID) 和认证令牌，
    /// 这些信息将用于后续的所有认证请求。
    ///
    /// # 参数
    /// - `uid`: 用户的唯一标识符。
    /// - `token`: 用于 API 请求的认证令牌。
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

    /// 在画板的指定位置绘制一个像素。
    ///
    /// 此异步函数将向 API 发送一个绘制单个像素的请求。
    ///
    /// # 参数
    /// - `pos`: `Pos` 结构体，表示要绘制像素的 (x, y) 坐标。
    /// - `color`: `Rgb` 结构体，表示像素的颜色。
    ///
    /// # 返回
    /// `Result`，成功时包含 `PaintResult`，表示绘制操作的结果 (例如，成功或失败原因)，
    /// 失败时包含 `PaintboardError`。
    async fn paint(&mut self, pos: Pos, color: Rgb) -> Result<PaintResult, PaintboardError>;

    /// 批量绘制多个像素。
    ///
    /// 此异步函数将向 API 发送一个包含多个像素绘制操作的批量请求。
    /// 批量操作通常比发送单个像素请求更高效。
    ///
    /// # 参数
    /// - `operations`: 一个 `Vec`，其中每个元素是一个包含 `Pos` 和 `Rgb` 的元组，
    ///   表示要绘制的每个像素的位置和颜色。
    ///
    /// # 返回
    /// `Result`，成功时返回 `()` (表示操作成功但没有特定返回值)，
    /// 失败时包含 `PaintboardError`。
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
}
