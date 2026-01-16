use once_cell::sync::OnceCell;

/// 惩罚敏感度系数
///
/// - 值越大,对热点区域的惩罚越强
/// - 建议范围: 0.1 - 10.0
/// - 默认值: 1.0 (适度惩罚)
const DEFAULT_PENALTY_SENSITIVITY: f32 = 1.0;
pub static PENALTY_SENSITIVITY: OnceCell<f32> = OnceCell::new();

pub fn get_penalty_sensitivity() -> f32 {
    *PENALTY_SENSITIVITY.get_or_init(|| {
        tracing::warn!("PENALTY_SENSITIVITY 未初始化，使用默认值");
        DEFAULT_PENALTY_SENSITIVITY
    })
}
