use once_cell::sync::OnceCell;

const DEFAULT_PENALTY_SCALE: f32 = 60000.0;
pub static PENALTY_SCALE: OnceCell<f32> = OnceCell::new();

pub fn get_penalty_scale() -> f32 {
    *PENALTY_SCALE.get_or_init(|| {
        tracing::warn!("PENALTY_SCALE 未初始化，使用默认值");
        DEFAULT_PENALTY_SCALE
    })
}
