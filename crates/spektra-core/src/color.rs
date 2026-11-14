//! 色彩空间数学：CIE XYZ ↔ 线性 sRGB ↔ sRGB 编码。
//!
//! 全部为线性代数与标准分段伽马，无外部依赖。
//! TODO(parity)：M2 里程碑对齐上游的 ProPhoto/Rec2020 工作空间选择。

/// XYZ（D65）→ 线性 sRGB，标准 sRGB 矩阵。
pub fn xyz_to_linear_srgb(xyz: &[f64; 3]) -> [f32; 3] {
    const M: [[f64; 3]; 3] = [
        [3.2404542, -1.5371385, -0.4985314],
        [-0.9692660, 1.8760108, 0.0415560],
        [0.0556434, -0.2040259, 1.0572252],
    ];
    let mut out = [0.0f32; 3];
    for (i, row) in M.iter().enumerate() {
        out[i] = (row[0] * xyz[0] + row[1] * xyz[1] + row[2] * xyz[2]) as f32;
    }
    out
}

/// 线性 sRGB → XYZ（D65）。
pub fn linear_srgb_to_xyz(rgb: &[f32; 3]) -> [f64; 3] {
    const M: [[f64; 3]; 3] = [
        [0.4124564, 0.3575761, 0.1804375],
        [0.2126729, 0.7151522, 0.0721750],
        [0.0193339, 0.1191920, 0.9503041],
    ];
    let mut out = [0.0f64; 3];
    for (i, row) in M.iter().enumerate() {
        out[i] = row[0] * rgb[0] as f64 + row[1] * rgb[1] as f64 + row[2] * rgb[2] as f64;
    }
    out
}

/// sRGB 分段传递函数：线性 → 编码。
pub fn encode_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB 分段传递函数：编码 → 线性。
pub fn decode_srgb(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn srgb_roundtrip() {
        for i in 0..=100 {
            let c = i as f32 / 100.0;
            let lin = decode_srgb(c);
            assert!(close(encode_srgb(lin), c, 1e-5), "c={c}");
        }
    }

    #[test]
    fn white_point_roundtrip() {
        // D65 白（Y=1）经 XYZ→sRGB 后三通道应相等且 ≈ 1
        let xyz = [0.950_47, 1.0, 1.088_83];
        let rgb = xyz_to_linear_srgb(&xyz);
        assert!(close(rgb[0], rgb[1], 1e-3) && close(rgb[1], rgb[2], 1e-3));
        assert!(close(rgb[1], 1.0, 1e-3), "R={} G={} B={}", rgb[0], rgb[1], rgb[2]);
    }

    #[test]
    fn matrix_inverse() {
        let rgb = [0.9, 0.5, 0.2];
        let xyz = linear_srgb_to_xyz(&rgb);
        let back = xyz_to_linear_srgb(&xyz);
        for i in 0..3 {
            assert!(close(back[i], rgb[i], 1e-4));
        }
    }
}
