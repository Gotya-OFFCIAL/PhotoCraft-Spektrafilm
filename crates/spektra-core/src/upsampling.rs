//! hanatos2025 光谱上采样：RGB → 胶片 raw 曝光。
//!
//! 移植自上游 spektrafilm `utils/spectral_upsampling.py` 与 `utils/fast_interp_lut.py`：
//! - 预计算光谱 LUT（tc 色度索引，N×N×81）经胶片灵敏度投影为 raw LUT（N×N×3）
//! - 运行时：线性 RGB →（CAT16 合成矩阵）→ XYZ → xy 色度 → tc → Mitchell 双三次插值 → ×亮度
//!
//! 数值域 f64，运算顺序严格对齐上游；无 fastmath。

use crate::types::{RawLut, SpectraLut, Tables};

/// Mitchell–Netravali 权重（B=C=1/3），与 fast_interp_lut.mitchell_weight 一致。
#[inline]
pub fn mitchell_weight(t: f64) -> f64 {
    const B: f64 = 1.0 / 3.0;
    const C: f64 = 1.0 / 3.0;
    let x = t.abs();
    if x < 1.0 {
        (1.0 / 6.0)
            * ((12.0 - 9.0 * B - 6.0 * C) * x * x * x
                + (-18.0 + 12.0 * B + 6.0 * C) * x * x
                + (6.0 - 2.0 * B))
    } else if x < 2.0 {
        (1.0 / 6.0)
            * ((-B - 6.0 * C) * x * x * x
                + (6.0 * B + 30.0 * C) * x * x
                + (-12.0 * B - 48.0 * C) * x
                + (8.0 * B + 24.0 * C))
    } else {
        0.0
    }
}

/// 镜像边界索引折返（safe_index）。
#[inline]
fn safe_index(idx: i64, len: usize) -> usize {
    let l = len as i64;
    let mut i = idx;
    // 折返周期 2L，先归一到 [0, 2L)
    i = i.rem_euclid(2 * l);
    if i >= l {
        i = 2 * l - 1 - i;
    }
    i as usize
}

/// 坐标钳制到 LUT 定义域 [0, L-1]（clamp_coordinate）。
#[inline]
fn clamp_coordinate(coord: f64, len: usize) -> f64 {
    if coord <= 0.0 {
        return 0.0;
    }
    let upper = (len - 1) as f64;
    if coord >= upper {
        upper
    } else {
        coord
    }
}

/// 映射到 [0, L-2] 的立方插值单元（cubic_coordinate_base_fraction）。
#[inline]
fn cubic_cell(coord: f64, len: usize) -> (usize, f64) {
    let c = clamp_coordinate(coord, len);
    if c >= (len - 1) as f64 {
        (len - 2, 1.0)
    } else {
        let base = c.floor() as usize;
        (base, c - base as f64)
    }
}

/// 三角坐标 → 方坐标（_tri2quad）。
pub fn tri2quad(tc: [f64; 2]) -> [f64; 2] {
    let (tx, ty) = (tc[0], tc[1]);
    let y = ty / (1.0 - tx).max(1e-10);
    let x = (1.0 - tx) * (1.0 - tx);
    [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)]
}

/// 方坐标 → 三角坐标（_quad2tri）。
pub fn quad2tri(xy: [f64; 2]) -> [f64; 2] {
    let (x, y) = (xy[0], xy[1]);
    [1.0 - x.sqrt(), y * x.sqrt()]
}

/// 2D raw LUT 的 Mitchell 双三次查找（_cubic_interp_lut_at_2d + _apply_lut_cubic_2d）：
/// 输入 tc ∈ [0,1]²，内部乘 (L-1)，4×4 核、镜像边界、权重和归一。
pub fn lut_lookup_cubic_2d(lut: &RawLut, tc: [f64; 2]) -> [f64; 3] {
    let size = lut.n;
    let scale = (size - 1) as f64;
    let x_in = tc[0] * scale;
    let y_in = tc[1] * scale;
    let (x_base, x_frac) = cubic_cell(x_in, size);
    let (y_base, y_frac) = cubic_cell(y_in, size);

    let mut wx = [0.0f64; 4];
    let mut wy = [0.0f64; 4];
    wx[0] = mitchell_weight(x_frac + 1.0);
    wx[1] = mitchell_weight(x_frac);
    wx[2] = mitchell_weight(x_frac - 1.0);
    wx[3] = mitchell_weight(x_frac - 2.0);
    wy[0] = mitchell_weight(y_frac + 1.0);
    wy[1] = mitchell_weight(y_frac);
    wy[2] = mitchell_weight(y_frac - 1.0);
    wy[3] = mitchell_weight(y_frac - 2.0);

    let mut out = [0.0f64; 3];
    let mut weight_sum = 0.0f64;
    for i in 0..4usize {
        let xi = safe_index(x_base as i64 - 1 + i as i64, size);
        for j in 0..4usize {
            let yj = safe_index(y_base as i64 - 1 + j as i64, size);
            let w = wx[i] * wy[j];
            weight_sum += w;
            let v = lut.at(xi, yj);
            out[0] += w * v[0];
            out[1] += w * v[1];
            out[2] += w * v[2];
        }
    }
    if weight_sum != 0.0 {
        out[0] /= weight_sum;
        out[1] /= weight_sum;
        out[2] /= weight_sum;
    }
    out
}

/// 误差函数（libm，~1 ulp，与上游 scipy.special.erf 精度对齐）。
pub fn erf(x: f64) -> f64 {
    libm::erf(x)
}

/// erf4 光谱带通窗（eval_erf4_spectral_bandpass）：
/// edge_uv = ½(1+erf((λ−c_uv)/(σ_uv√2)))；edge_ir = ½(1−erf((λ−c_ir)/(σ_ir√2)))；
/// window = edge_uv·edge_ir，三通道相同。
pub fn erf4_window(params: &[f64; 4], wavelengths: &[f64]) -> [[f64; 3]; 81] {
    let (c_uv, sigma_uv, c_ir, sigma_ir) = (params[0], params[1], params[2], params[3]);
    let sqrt2 = 2.0f64.sqrt();
    wavelengths
        .iter()
        .map(|&wl| {
            let edge_uv = 0.5 * (1.0 + erf((wl - c_uv) / (sigma_uv * sqrt2)));
            let edge_ir = 0.5 * (1.0 - erf((wl - c_ir) / (sigma_ir * sqrt2)));
            let w = edge_uv * edge_ir;
            [w, w, w]
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

/// 合成 raw LUT（compute_hanatos2025_tc_lut：window 开、surface 关、blur 0、无 gamut 压缩）。
///
/// 上游：
/// 1) `normalization[m] = Σ S·I·W / Σ S·I`，`W /= normalization`（保持白平衡）
/// 2) `raw_lut = contract('ijl,lm->ijm', spectra_lut, sensitivity * window)`
pub fn compose_raw_lut(
    spectra: &SpectraLut,
    sensitivity: &[[f64; 3]],
    window: &[[f64; 3]],
    illuminant: &[f64],
) -> RawLut {
    let wl_count = sensitivity.len();
    // 逐通道白平衡归一
    let mut norm = [0.0f64; 3];
    for m in 0..3 {
        let mut num = 0.0f64;
        let mut den = 0.0f64;
        for l in 0..wl_count {
            num += sensitivity[l][m] * illuminant[l] * window[l][m];
            den += sensitivity[l][m] * illuminant[l];
        }
        norm[m] = num / den;
    }

    let n = spectra.n;
    let mut data = vec![[0.0f64; 3]; n * n];
    for i in 0..n {
        for j in 0..n {
            let mut acc = [0.0f64; 3];
            for l in 0..wl_count {
                let s = spectra.at(i, j, l);
                if s == 0.0 {
                    continue;
                }
                for m in 0..3 {
                    acc[m] += s * sensitivity[l][m] * (window[l][m] / norm[m]);
                }
            }
            data[i * n + j] = acc;
        }
    }
    RawLut { n, data }
}

/// RGB（线性 sRGB）→ 胶片 raw 曝光（rgb_to_raw_hanatos2025 + _rgb_to_tc_b）。
///
/// XYZ = M_d55·rgb（合成矩阵含 CAT16）；b = X+Y+Z（非有限→0，上游 nan_to_num）；
/// xy = (X,Y)/max(b,1e-10)；tc = tri2quad(xy)；raw = LUT 双三次插值；raw *= b。
pub fn rgb_to_raw(rgb: [f64; 3], raw_lut: &RawLut, tables: &Tables) -> [f64; 3] {
    let m = &tables.mat_rgb_to_xyz_d55;
    let mut xyz = [0.0f64; 3];
    for i in 0..3 {
        xyz[i] = m[i][0] * rgb[0] + m[i][1] * rgb[1] + m[i][2] * rgb[2];
    }
    let mut b = xyz[0] + xyz[1] + xyz[2];
    if !b.is_finite() {
        b = 0.0;
    }
    let b_safe = b.max(1e-10);
    let xy = [xyz[0] / b_safe, xyz[1] / b_safe];
    let tc = tri2quad(xy);
    let mut raw = lut_lookup_cubic_2d(raw_lut, tc);
    raw[0] *= b;
    raw[1] *= b;
    raw[2] *= b;
    raw
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{load_profile, load_raw_lut, load_tables};
    use crate::oracle_dir;

    const PORTRA_JSON: &str =
        r"G:\AI\移植工作\参考\spektrafilm-upstream\src\spektrafilm\data\profiles\kodak_portra_400.json";

    /// 内联解析 Portra 灵敏度与窗口参数（不依赖 profile.rs，自包含）。
    fn parse_sensitivity_and_window() -> (Vec<[f64; 3]>, [f64; 4], Vec<f64>) {
        let text = std::fs::read_to_string(PORTRA_JSON).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let f = |x: &serde_json::Value| match x {
            serde_json::Value::Null => f64::NAN,
            serde_json::Value::Number(n) => n.as_f64().unwrap(),
            _ => panic!("bad value"),
        };
        let sens: Vec<[f64; 3]> = v["data"]["log_sensitivity"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let r: Vec<f64> = row.as_array().unwrap().iter().map(f).collect();
                [
                    if r[0].is_nan() { 0.0 } else { 10f64.powf(r[0]) },
                    if r[1].is_nan() { 0.0 } else { 10f64.powf(r[1]) },
                    if r[2].is_nan() { 0.0 } else { 10f64.powf(r[2]) },
                ]
            })
            .collect();
        let wp: Vec<f64> = v["data"]["hanatos2025_adaptation_window_params"]
            .as_array()
            .unwrap()
            .iter()
            .map(f)
            .collect();
        let wl: Vec<f64> = v["data"]["wavelengths"].as_array().unwrap().iter().map(f).collect();
        (sens, [wp[0], wp[1], wp[2], wp[3]], wl)
    }

    #[test]
    fn mitchell_known_values() {
        // B=C=1/3：w(0)=8/9，w(±1)=1/18，|t|≥2 为零（与上游公式一致）
        assert!((mitchell_weight(0.0) - 8.0 / 9.0).abs() < 1e-12);
        assert!((mitchell_weight(1.0) - 1.0 / 18.0).abs() < 1e-12);
        assert!((mitchell_weight(-1.0) - 1.0 / 18.0).abs() < 1e-12);
        assert!(mitchell_weight(2.5).abs() < 1e-12);
    }

    #[test]
    fn quad_tri_roundtrip() {
        // tri2quad 的定义域是三角形 {tx≥0, ty≥0, tx+ty≤1}（quad→tri 的逆映射）
        for i in 0..=10 {
            for j in 0..=10 {
                let tc = [i as f64 / 10.0, j as f64 / 10.0];
                if tc[0] + tc[1] > 1.0 - 1e-9 {
                    continue; // 三角形外：tri2quad 的 y 钳制使往返不闭合（预期）
                }
                let back = quad2tri(tri2quad(tc));
                assert!((back[0] - tc[0]).abs() < 1e-12 && (back[1] - tc[1]).abs() < 1e-12, "tc={tc:?}");
            }
        }
    }

    #[test]
    fn erf_matches_known_values() {
        // 参考值（erf(0)=0, erf(∞)=1, erf(0.5)≈0.5204999, erf(1)≈0.8427008）
        assert!(erf(0.0).abs() < 1e-9);
        assert!((erf(0.5) - 0.520_499_9).abs() < 1.5e-7);
        assert!((erf(1.0) - 0.842_700_8).abs() < 1.5e-7);
        assert!((erf(3.0) - 0.999_977_9).abs() < 1.5e-7);
    }

    #[test]
    fn composed_raw_lut_matches_oracle() {
        let (sens, wp, wl) = parse_sensitivity_and_window();
        let t = load_tables(&oracle_dir().display().to_string()).expect("tables");
        let ill_d55 = t.illuminant("D55").unwrap();
        let window = erf4_window(&wp, &wl);
        let mine = compose_raw_lut(&t.spectra_lut, &sens, &window, ill_d55);
        let oracle = load_raw_lut(&oracle_dir().display().to_string()).expect("oracle raw_lut");
        assert_eq!(mine.n, oracle.n);
        let mut max_abs = 0.0f64;
        let mut max_idx = (0usize, 0usize);
        for i in 0..mine.n {
            for j in 0..mine.n {
                let a = mine.at(i, j);
                let b = oracle.at(i, j);
                for k in 0..3 {
                    let d = (a[k] - b[k]).abs();
                    if d > max_abs {
                        max_abs = d;
                        max_idx = (i, j);
                    }
                }
            }
        }
        println!("compose_raw_lut max_abs={max_abs:.3e} at {:?}", max_idx);
        assert!(max_abs <= 1e-6, "raw LUT 合成对拍超标: {max_abs:.3e}");
    }

    #[test]
    fn rgb_to_raw_smoke() {
        let prof = load_profile("kodak_portra_400").expect("profile");
        let t = load_tables(&oracle_dir().display().to_string()).expect("tables");
        let (sens, wp, wl) = parse_sensitivity_and_window();
        let window = erf4_window(&wp, &wl);
        let ill_d55 = t.illuminant(prof.info.reference_illuminant.as_str()).unwrap();
        let raw_lut = compose_raw_lut(&t.spectra_lut, &sens, &window, ill_d55);

        let mid = rgb_to_raw([0.184, 0.184, 0.184], &raw_lut, &t);
        println!("midgray raw = {:?}", mid);
        assert!(mid.iter().all(|v| v.is_finite() && *v > 0.0), "midgray raw 应为正: {mid:?}");

        let black = rgb_to_raw([0.0, 0.0, 0.0], &raw_lut, &t);
        assert!(black.iter().all(|v| *v == 0.0), "全黑输入 → raw 全零: {black:?}");
    }
}
