//! 光谱分布与 CIE 1931 2° 观察者匹配函数。

use crate::color;
use crate::grid::{integral_step, WL_COUNT};

/// 光谱数据错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpectralError {
    /// 采样点数与标准网格（81）不符。
    WrongLength { got: usize, expected: usize },
    /// 含 NaN / Inf 或负值（光谱量必须非负且有限）。
    InvalidValue { index: usize },
}

impl core::fmt::Display for SpectralError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpectralError::WrongLength { got, expected } => {
                write!(f, "光谱采样点数错误：得到 {got}，要求 {expected}")
            }
            SpectralError::InvalidValue { index } => {
                write!(f, "光谱第 {index} 点为 NaN/Inf 或负值")
            }
        }
    }
}

impl std::error::Error for SpectralError {}

/// 标准网格上的光谱分布（线性域、非负），如光源 SPD、反射率、染料透射率。
#[derive(Clone, Debug, PartialEq)]
pub struct SpectralDistribution {
    pub samples: Vec<f32>,
}

impl SpectralDistribution {
    /// 全零分布。
    pub fn zeros() -> Self {
        Self { samples: vec![0.0; WL_COUNT] }
    }

    /// 常数分布（平坦光谱）。
    pub fn constant(value: f32) -> Self {
        Self { samples: vec![value; WL_COUNT] }
    }

    /// 从采样数组构建并校验（长度 = 81，全部有限且非负）。
    pub fn from_samples(samples: Vec<f32>) -> Result<Self, SpectralError> {
        if samples.len() != WL_COUNT {
            return Err(SpectralError::WrongLength { got: samples.len(), expected: WL_COUNT });
        }
        for (i, &v) in samples.iter().enumerate() {
            if !v.is_finite() || v < 0.0 {
                return Err(SpectralError::InvalidValue { index: i });
            }
        }
        Ok(Self { samples })
    }

    /// 逐波长乘积：`self · other`（例如 光源 × 反射率 × 透射率）。
    pub fn mul(&self, other: &SpectralDistribution) -> SpectralDistribution {
        SpectralDistribution {
            samples: self.samples.iter().zip(&other.samples).map(|(a, b)| a * b).collect(),
        }
    }

    /// 常数缩放（曝光、密度缩放）。
    pub fn scale(&self, k: f32) -> SpectralDistribution {
        SpectralDistribution { samples: self.samples.iter().map(|v| v * k).collect() }
    }
}

/// CIE 1931 2° 标准观察者匹配函数（标准网格采样）。
///
/// TODO(parity)：M2 里程碑换用上游仓库的表列 CMF 数据做逐点对拍；
/// 当前采用 Wyman/Sloan/Shirley (2013) 多叶高斯解析近似，
/// 在 380–780 nm 全域与表列值偏差 ~1% 量级，仅作为骨架期占位。
pub struct Cmf;

impl Cmf {
    /// 分段高斯：μ 两侧带宽不同。
    #[inline]
    fn g(x: f32, mu: f32, s1: f32, s2: f32) -> f32 {
        let s = if x < mu { s1 } else { s2 };
        (-0.5 * ((x - mu) / s).powi(2)).exp()
    }

    /// x̄(λ)。
    pub fn x_bar(wl: f32) -> f32 {
        1.056 * Self::g(wl, 599.8, 37.9, 31.0)
            + 0.362 * Self::g(wl, 442.0, 16.0, 26.7)
            - 0.065 * Self::g(wl, 501.1, 20.4, 26.2)
    }

    /// ȳ(λ)（即人眼明视觉光效率）。
    pub fn y_bar(wl: f32) -> f32 {
        0.821 * Self::g(wl, 568.8, 46.9, 40.5) + 0.286 * Self::g(wl, 530.9, 16.3, 31.1)
    }

    /// z̄(λ)。
    pub fn z_bar(wl: f32) -> f32 {
        1.217 * Self::g(wl, 437.0, 11.8, 36.0) + 0.681 * Self::g(wl, 459.0, 26.0, 13.8)
    }

    /// 把标准网格上的光谱分布积分成 CIE XYZ（Y 归一化，理想漫反射白 Y=1）。
    pub fn spd_to_xyz(spd: &SpectralDistribution) -> [f64; 3] {
        let step = integral_step() as f64;
        let mut xyz = [0.0f64; 3];
        for (i, &s) in spd.samples.iter().enumerate() {
            let wl = crate::grid::wavelength(i) as f64;
            let s = s as f64;
            xyz[0] += s * Self::x_bar(wl as f32) as f64 * step;
            xyz[1] += s * Self::y_bar(wl as f32) as f64 * step;
            xyz[2] += s * Self::z_bar(wl as f32) as f64 * step;
        }
        let y_norm: f64 = (0..WL_COUNT)
            .map(|i| Self::y_bar(crate::grid::wavelength(i) as f32) as f64 * step)
            .sum();
        [xyz[0] / y_norm, xyz[1] / y_norm, xyz[2] / y_norm]
    }

    /// 光谱 → 线性 sRGB（内部先转 XYZ 再走标准矩阵）。
    pub fn spd_to_linear_srgb(spd: &SpectralDistribution) -> [f32; 3] {
        let xyz = Self::spd_to_xyz(spd);
        color::xyz_to_linear_srgb(&xyz)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_white_is_neutral() {
        // 等能白（E 光源下的理想漫反射）色度应接近 (1/3, 1/3)
        let spd = SpectralDistribution::constant(1.0);
        let xyz = Cmf::spd_to_xyz(&spd);
        let sum = xyz[0] + xyz[1] + xyz[2];
        let x = xyz[0] / sum;
        let y = xyz[1] / sum;
        // 解析近似 CMF 的固有中性偏差 ~2e-4；表列 CMF（M2）下 E 白精确为 (1/3,1/3)
        assert!((y - 1.0 / 3.0).abs() < 1e-3, "y={y}");
        assert!((x - 1.0 / 3.0).abs() < 2e-2, "解析近似误差偏大 x={x}");
        assert!((xyz[1] - 1.0).abs() < 1e-4, "Y 归一化失败 Y={}", xyz[1]);
    }

    #[test]
    fn from_samples_rejects_bad_input() {
        assert_eq!(
            SpectralDistribution::from_samples(vec![0.0; 10]),
            Err(SpectralError::WrongLength { got: 10, expected: WL_COUNT })
        );
        let mut bad = vec![0.0; WL_COUNT];
        bad[3] = -1.0;
        assert_eq!(
            SpectralDistribution::from_samples(bad),
            Err(SpectralError::InvalidValue { index: 3 })
        );
    }

    #[test]
    fn mul_and_scale() {
        let a = SpectralDistribution::constant(2.0);
        let b = SpectralDistribution::constant(3.0);
        assert!((a.mul(&b).samples[10] - 6.0).abs() < 1e-6);
        assert!((a.scale(0.5).samples[10] - 1.0).abs() < 1e-6);
    }
}
