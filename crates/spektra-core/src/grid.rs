//! 标准波长网格。
//!
//! 与上游 spektrafilm 的光谱数据对齐：CIE 1931 标准 5 nm 采样，
//! 380–780 nm 共 81 个采样点。上游 JSON 胶片/相纸数据移植时统一重采样到此网格。

/// 网格起始波长（nm）。
pub const WL_START_NM: f32 = 380.0;
/// 采样步长（nm）。
pub const WL_STEP_NM: f32 = 5.0;
/// 采样点数：(780 − 380) / 5 + 1 = 81。
pub const WL_COUNT: usize = 81;

/// 第 i 个采样点的波长（nm）。
#[inline]
pub fn wavelength(i: usize) -> f32 {
    debug_assert!(i < WL_COUNT);
    WL_START_NM + i as f32 * WL_STEP_NM
}

/// 全网格波长迭代器。
pub fn wavelengths() -> impl Iterator<Item = f32> {
    (0..WL_COUNT).map(wavelength)
}

/// 网格积分步长（μm 级，色度值不受统一缩放影响）。
#[inline]
pub fn integral_step() -> f32 {
    WL_STEP_NM / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_endpoints() {
        assert!((wavelength(0) - 380.0).abs() < 1e-6);
        assert!((wavelength(WL_COUNT - 1) - 780.0).abs() < 1e-6);
        assert_eq!(wavelengths().count(), WL_COUNT);
    }
}
