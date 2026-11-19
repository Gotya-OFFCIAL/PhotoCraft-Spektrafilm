//! 显影（特性曲线插值）+ 光谱密度合成 + 负片直扫。
//!
//! 移植自上游 model/develop.py、model/density_curves.py（fast_interp）、
//! runtime/stages/scanning.py（scan_film 分支）、utils/conversions.py（density_to_light）。
//! f64，运算顺序严格对齐上游。

use crate::types::{FilmProfile, Tables};

/// f64 sRGB 分段编码（output_cctf_encoding=True）。
/// 上游 np.where 语义：c ≤ 0.0031308（含负值）走 12.92c，否则走幂支。
pub fn encode_srgb_f64(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// 逐通道 NaN 最小值（np.nanmin 语义）。
fn nanmin(values: impl Iterator<Item = f64>) -> f64 {
    let mut m = f64::INFINITY;
    for v in values {
        if v.is_finite() && v < m {
            m = v;
        }
    }
    // 全 NaN 时 np.nanmin 返回 NaN（伴随警告）；此处同样返回 NaN
    if m.is_infinite() {
        f64::NAN
    } else {
        m
    }
}

/// 显影：曲线逐通道去最小值（nanmin 归一化）后按 log 曝光插值 → CMY 密度。
///
/// 上游 develop()（grain/couplers 关闭时）：
/// `normalized = density_curves - nanmin(density_curves, axis=0)`，
/// `density_cmy = fast_interp(log_raw, log_exposure, normalized)`。
pub fn develop(log_raw: [f64; 3], profile: &FilmProfile) -> [f64; 3] {
    let curves = &profile.data.density_curves;
    let mins = [
        nanmin(curves.iter().map(|r| r[0])),
        nanmin(curves.iter().map(|r| r[1])),
        nanmin(curves.iter().map(|r| r[2])),
    ];
    let xa = &profile.data.log_exposure;
    let mut out = [0.0f64; 3];
    for c in 0..3 {
        // 归一化曲线逐点构建（256 点，逐像素复用代价可接受；批量版在 develop_image 优化）
        // 为性能起见，这里直接内联：y = curves[i][c] - mins[c]
        let x = log_raw[c];
        out[c] = interp_y_normalized(x, xa, curves, mins[c], c);
    }
    out
}

#[inline]
fn interp_y_normalized(x: f64, xa: &[f64], curves: &[[f64; 3]], min: f64, c: usize) -> f64 {
    let k = xa.len();
    if x <= xa[0] {
        return curves[0][c] - min;
    }
    if x >= xa[k - 1] {
        return curves[k - 1][c] - min;
    }
    let mut lo = 0usize;
    let mut hi = k;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if xa[mid] <= x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let low = lo - 1;
    let t = (x - xa[low]) / (xa[low + 1] - xa[low]);
    let y0 = curves[low][c] - min;
    let y1 = curves[low + 1][c] - min;
    y0 + t * (y1 - y0)
}

/// 批量显影（预归一化曲线，避免逐像素重建）。
pub fn develop_image(log_raw: &[[f64; 3]], profile: &FilmProfile) -> Vec<[f64; 3]> {
    let curves = &profile.data.density_curves;
    let mins = [
        nanmin(curves.iter().map(|r| r[0])),
        nanmin(curves.iter().map(|r| r[1])),
        nanmin(curves.iter().map(|r| r[2])),
    ];
    let xa = &profile.data.log_exposure;
    log_raw
        .iter()
        .map(|px| {
            let mut out = [0.0f64; 3];
            for c in 0..3 {
                out[c] = interp_y_normalized(px[c], xa, curves, mins[c], c);
            }
            out
        })
        .collect()
}

/// develop_simple：在**未归一化**曲线上插值（上游 develop_simple / 
/// FilmingStage._simple_rgb_to_density_spectral 语义，与 develop() 的 nanmin 归一化不同！）。
pub fn develop_simple(log_raw: [f64; 3], profile: &FilmProfile) -> [f64; 3] {
    let curves = &profile.data.density_curves;
    let xa = &profile.data.log_exposure;
    let mut out = [0.0f64; 3];
    for c in 0..3 {
        out[c] = interp_y_raw(log_raw[c], xa, curves, c);
    }
    out
}

#[inline]
fn interp_y_raw(x: f64, xa: &[f64], curves: &[[f64; 3]], c: usize) -> f64 {
    let k = xa.len();
    if x <= xa[0] {
        return curves[0][c];
    }
    if x >= xa[k - 1] {
        return curves[k - 1][c];
    }
    let mut lo = 0usize;
    let mut hi = k;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if xa[mid] <= x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let low = lo - 1;
    let t = (x - xa[low]) / (xa[low + 1] - xa[low]);
    curves[low][c] + t * (curves[low + 1][c] - curves[low][c])
}

/// 显影 + DIR 成色剂校正（develop() 完整路径，couplers 开启时）。
/// 流程：归一化曲线显影 → 成色剂 log 曝光校正 → 在成色剂前曲线上重新插值。
pub fn develop_with_couplers_image(
    log_raw: &[[f64; 3]],
    profile: &FilmProfile,
    couplers: &crate::couplers::DirCouplers,
    positive: bool,
) -> Vec<[f64; 3]> {
    let cmy = develop_image(log_raw, profile);
    // 上游 develop() 把 nanmin 归一化后的曲线传给成色剂校正（不是原始曲线！）
    let normalized = normalized_curves(profile);
    crate::couplers::apply_density_correction_image(log_raw, &cmy, &normalized, &profile.data.log_exposure, couplers, positive)
}

/// 归一化曲线：curves − nanmin（逐通道，忽略 NaN）。
pub fn normalized_curves(profile: &FilmProfile) -> Vec<[f64; 3]> {
    let curves = &profile.data.density_curves;
    let mins = [
        nanmin(curves.iter().map(|r| r[0])),
        nanmin(curves.iter().map(|r| r[1])),
        nanmin(curves.iter().map(|r| r[2])),
    ];
    curves.iter().map(|r| [r[0] - mins[0], r[1] - mins[1], r[2] - mins[2]]).collect()
}

/// 单像素版（与上同语义）。
pub fn develop_with_couplers(
    log_raw: [f64; 3],
    profile: &FilmProfile,
    couplers: &crate::couplers::DirCouplers,
    positive: bool,
) -> [f64; 3] {
    let cmy = develop(log_raw, profile);
    let normalized = normalized_curves(profile);
    let matrix = couplers.matrix();
    let curves0 =
        crate::couplers::density_curves_before_couplers(&normalized, &profile.data.log_exposure, &matrix, positive);
    crate::couplers::apply_density_correction(log_raw, cmy, &normalized, &profile.data.log_exposure, couplers, positive, &curves0)
}

/// 光谱密度合成（compute_density_spectral）：
/// `ds[l] = Σ_k cmy[k]·channel_density[l][k] + base_density[l]`，NaN 传播。
pub fn density_spectral(cmy: [f64; 3], profile: &FilmProfile) -> [f64; 81] {
    let cd = &profile.data.channel_density;
    let base = &profile.data.base_density;
    let mut out = [0.0f64; 81];
    for l in 0..81 {
        out[l] = cmy[0] * cd[l][0] + cmy[1] * cd[l][1] + cmy[2] * cd[l][2] + base[l];
    }
    out
}

/// 负片直扫单像素（scanning.py scan_film 分支，use_lut=False 直算路径）。
///
/// 1) ds = density_spectral(cmy)
/// 2) light[l] = ds[l] 为 NaN ? 0 : ill_D50[l]·10^(−ds[l])     // density_to_light
/// 3) norm = Σ ill·cmfs[:,1]；xyz[k] = Σ light·cmfs[:,k] / norm
/// 4) log_xyz = log10(max(xyz,0)+1e-10)；xyz2 = 10^log_xyz     // 上游显式往返
/// 5) rgb_lin = M_view·xyz2（CAT02 合成矩阵）
/// 6) sRGB 编码
pub fn scan_film_rgb(cmy: [f64; 3], profile: &FilmProfile, tables: &Tables) -> [f64; 3] {
    scan_film_rgb_out(cmy, profile, tables, None)
}

/// scan_film + 输出色域压缩（M2-D2）。output_gamut = Some 时启用 cam16ucs 压缩。
pub fn scan_film_rgb_out(
    cmy: [f64; 3],
    profile: &FilmProfile,
    tables: &Tables,
    output_gamut: Option<&crate::gamut::OutputGamutCompressor>,
) -> [f64; 3] {
    let ds = density_spectral(cmy, profile);
    let ill = tables
        .illuminant(&profile.info.viewing_illuminant)
        .cloned()
        .unwrap_or_else(|| tables.illuminant("D50").cloned().expect("D50 illuminant"));
    let cmfs = &tables.cmfs;

    // norm = Σ_l ill·cmfs[l][1]（viewing 光源 Y）
    let mut norm = 0.0f64;
    for l in 0..81 {
        norm += ill[l] * cmfs[l][1];
    }

    // light → xyz
    let mut xyz = [0.0f64; 3];
    for l in 0..81 {
        if ds[l].is_nan() {
            continue; // 10^(-NaN) = NaN → 上游置 0
        }
        let light = ill[l] * 10f64.powf(-ds[l]);
        xyz[0] += light * cmfs[l][0];
        xyz[1] += light * cmfs[l][1];
        xyz[2] += light * cmfs[l][2];
    }
    for v in &mut xyz {
        *v /= norm;
    }

    // 显式 log10 → 10^ 往返（与上游一致，注意这引入了往返舍入）
    let mut xyz2 = [0.0f64; 3];
    for k in 0..3 {
        xyz2[k] = 10f64.powf((xyz[k].max(0.0) + 1e-10).log10());
    }

    // XYZ → 线性 sRGB（CAT02 合成矩阵）
    let rgb = match output_gamut {
        Some(c) => c.apply_to_linear_pure(&tables.mat_xyz_to_rgb_view_pure, xyz2),
        None => {
            let m = &tables.mat_xyz_to_rgb_view;
            [
                m[0][0] * xyz2[0] + m[0][1] * xyz2[1] + m[0][2] * xyz2[2],
                m[1][0] * xyz2[0] + m[1][1] * xyz2[1] + m[1][2] * xyz2[2],
                m[2][0] * xyz2[0] + m[2][1] * xyz2[1] + m[2][2] * xyz2[2],
            ]
        }
    };

    [encode_srgb_f64(rgb[0]), encode_srgb_f64(rgb[1]), encode_srgb_f64(rgb[2])]
}

/// 批量负片直扫。
pub fn scan_film_image(cmy: &[[f64; 3]], profile: &FilmProfile, tables: &Tables) -> Vec<[f64; 3]> {
    scan_film_image_out(cmy, profile, tables, None)
}

/// 批量负片直扫（带输出色域压缩）。
pub fn scan_film_image_out(
    cmy: &[[f64; 3]],
    profile: &FilmProfile,
    tables: &Tables,
    output_gamut: Option<&crate::gamut::OutputGamutCompressor>,
) -> Vec<[f64; 3]> {
    cmy.iter().map(|&px| scan_film_rgb_out(px, profile, tables, output_gamut)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{load_profile, load_tables};
    use crate::oracle_dir;

    const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_vectors.json");

    fn parse_px(v: &serde_json::Value) -> Vec<[f64; 3]> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|px| {
                let a = px.as_array().unwrap();
                [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
            })
            .collect()
    }

    fn golden() -> serde_json::Value {
        let raw = std::fs::read_to_string(GOLDEN).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    fn stats(mine: &[[f64; 3]], gold: &[[f64; 3]]) -> (f64, f64) {
        let mut max_abs = 0.0f64;
        let mut sq = 0.0f64;
        let mut n = 0;
        for (a, b) in mine.iter().zip(gold.iter()) {
            for k in 0..3 {
                let d = (a[k] - b[k]).abs();
                if d.is_finite() {
                    max_abs = max_abs.max(d);
                    sq += d * d;
                    n += 1;
                }
            }
        }
        (max_abs, (sq / n as f64).sqrt())
    }

    #[test]
    fn encode_srgb_boundaries() {
        assert!((encode_srgb_f64(0.0031308) - 12.92 * 0.0031308).abs() < 1e-12);
        assert!(encode_srgb_f64(0.0031309) > 0.0031309); // 幂支
        assert!((encode_srgb_f64(1.0) - 1.0).abs() < 1e-12);
        assert!((encode_srgb_f64(-0.5) - 12.92 * -0.5).abs() < 1e-12);
    }

    #[test]
    fn base_density_nan_propagates() {
        let p = load_profile("kodak_portra_400").unwrap();
        let ds = density_spectral([0.1, 0.2, 0.3], &p);
        // Portra 380–400nm 段基底密度为 NaN → 光谱密度应为 NaN
        let nan_head = ds.iter().take(5).filter(|v| v.is_nan()).count();
        assert!(nan_head > 0, "短波端应为 NaN，得到 {ds:?}");
        assert!(ds[40].is_finite());
    }

    #[test]
    fn develop_matches_golden() {
        let p = load_profile("kodak_portra_400").unwrap();
        let g = golden();
        let (names, log_e, cmy): (Vec<String>, Vec<[f64; 3]>, Vec<[f64; 3]>) = {
            let pv = &g["patches"];
            (
                pv["names"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect(),
                parse_px(&pv["log_e_film"]),
                parse_px(&pv["cmy_film"]),
            )
        };
        let mine = develop_image(&log_e, &p);
        let (max_abs, rms) = stats(&mine, &cmy);
        println!("[patches] develop max_abs={max_abs:.3e} rms={rms:.3e}");
        assert!(max_abs <= 1e-4, "显影对拍超标: {max_abs:.3e}");
        assert_eq!(names.len(), 18);

        let log_e_ramp = parse_px(&g["ramp"]["log_e_film"]);
        let cmy_ramp = parse_px(&g["ramp"]["cmy_film"]);
        let mine_r = develop_image(&log_e_ramp, &p);
        let (max_abs_r, _) = stats(&mine_r, &cmy_ramp);
        println!("[ramp] develop max_abs={max_abs_r:.3e}");
        assert!(max_abs_r <= 1e-4, "显影(灰阶)对拍超标: {max_abs_r:.3e}");
    }

    #[test]
    fn scan_matches_golden() {
        let p = load_profile("kodak_portra_400").unwrap();
        let t = load_tables(&oracle_dir().display().to_string()).unwrap();
        let g = golden();
        let cmy = parse_px(&g["patches"]["cmy_film"]);
        let rgb_gold = parse_px(&g["patches"]["rgb_out"]);
        let mine = scan_film_image(&cmy, &p, &t);
        let (max_abs, rms) = stats(&mine, &rgb_gold);
        println!("[patches] scan max_abs={max_abs:.3e} rms={rms:.3e}");
        assert!(max_abs <= 1e-4, "扫描对拍超标: {max_abs:.3e}");

        let cmy_r = parse_px(&g["ramp"]["cmy_film"]);
        let rgb_r = parse_px(&g["ramp"]["rgb_out"]);
        let mine_r = scan_film_image(&cmy_r, &p, &t);
        let (max_abs_r, _) = stats(&mine_r, &rgb_r);
        println!("[ramp] scan max_abs={max_abs_r:.3e}");
        assert!(max_abs_r <= 1e-4, "扫描(灰阶)对拍超标: {max_abs_r:.3e}");
    }
}

#[cfg(test)]
mod debug_scan {
    use super::*;
    use crate::profile::{load_profile, load_tables};
    use crate::oracle_dir;

    #[test]
    #[ignore = "手工 bisect 用"]
    fn debug_intermediates() {
        let p = load_profile("kodak_portra_400").unwrap();
        let t = load_tables(&oracle_dir().display().to_string()).unwrap();
        let golden = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_vectors.json")).unwrap();
        let g: serde_json::Value = serde_json::from_str(&golden).unwrap();
        let names: Vec<String> = g["patches"]["names"].as_array().unwrap().iter().map(|x| x.as_str().unwrap().into()).collect();
        let parse = |i: usize| -> [f64; 3] {
            let a = g["patches"]["cmy_film"].as_array().unwrap()[i].as_array().unwrap();
            [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
        };
        let mut worst = (0usize, 0.0f64);
        for i in 0..names.len() {
            let out = scan_film_rgb(parse(i), &p, &t);
            let gold = {
                let a = g["patches"]["rgb_out"].as_array().unwrap()[i].as_array().unwrap();
                [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
            };
            let e = (0..3).map(|k| (out[k] - gold[k]).abs()).fold(0.0f64, f64::max);
            println!("{:>14} err={:.3e}", names[i], e);
            if e > worst.1 { worst = (i, e); }
        }
        println!("WORST: {} err={:.3e}", names[worst.0], worst.1);
        let idx = worst.0;
        let cmy: [f64; 3] = {
            let a = g["patches"]["cmy_film"].as_array().unwrap()[idx].as_array().unwrap();
            [a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()]
        };
        println!("cmy = {:.17e} {:.17e} {:.17e}", cmy[0], cmy[1], cmy[2]);
        let ds = density_spectral(cmy, &p);
        let ill = t.illuminant("D50").unwrap();
        let cmfs = &t.cmfs;
        let mut norm = 0.0;
        for l in 0..81 { norm += ill[l] * cmfs[l][1]; }
        println!("norm = {norm:.17e}");
        let mut xyz = [0.0f64; 3];
        for l in 0..81 {
            if ds[l].is_nan() { continue; }
            let light = ill[l] * 10f64.powf(-ds[l]);
            xyz[0] += light * cmfs[l][0];
            xyz[1] += light * cmfs[l][1];
            xyz[2] += light * cmfs[l][2];
        }
        println!("xyz_raw = {:.17e} {:.17e} {:.17e}", xyz[0], xyz[1], xyz[2]);
        for v in &mut xyz { *v /= norm; }
        println!("xyz = {:.17e} {:.17e} {:.17e}", xyz[0], xyz[1], xyz[2]);
        let mut xyz2 = [0.0f64; 3];
        for k in 0..3 { xyz2[k] = 10f64.powf((xyz[k].max(0.0) + 1e-10).log10()); }
        println!("xyz2 = {:.17e} {:.17e} {:.17e}", xyz2[0], xyz2[1], xyz2[2]);
        let m = &t.mat_xyz_to_rgb_view;
        let mut rgb = [0.0f64; 3];
        for i in 0..3 { rgb[i] = m[i][0]*xyz2[0] + m[i][1]*xyz2[1] + m[i][2]*xyz2[2]; }
        println!("rgb_lin = {:.17e} {:.17e} {:.17e}", rgb[0], rgb[1], rgb[2]);
        println!("rgb_enc = {:?}", [encode_srgb_f64(rgb[0]), encode_srgb_f64(rgb[1]), encode_srgb_f64(rgb[2])]);
    }
}

// ---------------------------------------------------------------------------
// M3a：管线级预计算 + 逐像素零分配快速路径
// ---------------------------------------------------------------------------

/// 管线级预计算（构建一次，逐像素零堆分配）。
pub struct Precomputed {
    /// 层密度插值 x 轴（per ch）：归一化密度曲线
    pub layer_xs: [Vec<f64>; 3],
    /// 层密度插值 y 值：index = sl*3+ch
    pub layer_ys: [Vec<f64>; 9],
    /// 显影插值 y 值（per ch）：归一化密度曲线，x 轴 = log_exposure
    pub curves_ys: [Vec<f64>; 3],
    /// 成色剂作用前曲线 y 值（per ch），x 轴 = log_exposure
    pub couplers_curves0_ys: [Vec<f64>; 3],
    pub couplers_matrix: [[f64; 3]; 3],
    pub couplers_active: bool,
}

/// 预计算（含成色剂曲线变换，构建一次）。
///
/// `positive` 暂未使用：本 vendored 切片只实现了负片方向的 DIR 成色剂预计算
/// （silver = 曲线本身）。上游对正片的 couplers 分支是 silver = nanmax − 曲线
/// （反射），在 filmSim v1（成色剂关）下不进入；启用正片成色剂时需先补此分支。
/// 注意：扫描链本身对 positive 无独立分支（见 parity_positive_velvia_scan_film
/// 机器精度对拍），正片直扫语义完全由 profile 数据承载。
pub fn precompute(
    profile: &FilmProfile,
    couplers: Option<&crate::couplers::DirCouplers>,
    _positive: bool,
) -> Precomputed {
    let curves_norm = normalized_curves(profile);
    let le = &profile.data.log_exposure;

    let mut layer_xs: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let mut layer_ys: [Vec<f64>; 9] = [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    for ch in 0..3 {
        layer_xs[ch] = curves_norm.iter().map(|r| r[ch]).collect();
        for sl in 0..3 {
            layer_ys[sl * 3 + ch] = profile.data.density_curves_layers.iter().map(|row| row[sl][ch]).collect();
        }
    }
    let curves_ys = [
        curves_norm.iter().map(|r| r[0]).collect(),
        curves_norm.iter().map(|r| r[1]).collect(),
        curves_norm.iter().map(|r| r[2]).collect(),
    ];

    let (couplers_curves0_ys, couplers_matrix, couplers_active) = match couplers {
        Some(c) => {
            let matrix = c.matrix();
            // compute_density_curves_before_dir_couplers（负片：silver = 曲线本身）
            // upstream: amount[j][m] = Σ_k curves[j][k]·M[k][m]
            //           xs[j] = le[j] − amount[j][c]
            //           curves0[j][c] = np.interp(le[j], xs[:,c], curves[:,c])
            let n = le.len();
            let mut amounts = vec![[0.0f64; 3]; n];
            for (j, row) in curves_norm.iter().enumerate() {
                for m in 0..3 {
                    for k in 0..3 {
                        amounts[j][m] += row[k] * matrix[k][m];
                    }
                }
            }
            let mut curves0: Vec<[f64; 3]> = Vec::with_capacity(n);
            for (j, _row) in curves_norm.iter().enumerate() {
                let mut vals = [0.0f64; 3];
                for c in 0..3 {
                    let xs: Vec<f64> = (0..n).map(|i| le[i] - amounts[i][c]).collect();
                    let ys: Vec<f64> = curves_norm.iter().map(|r| r[c]).collect();
                    vals[c] = interp_clamped_scalar(le[j], &xs, &ys);
                }
                curves0.push(vals);
            }
            let ys0 = [
                curves0.iter().map(|r| r[0]).collect(),
                curves0.iter().map(|r| r[1]).collect(),
                curves0.iter().map(|r| r[2]).collect(),
            ];
            (ys0, matrix, true)
        }
        None => (
            [Vec::new(), Vec::new(), Vec::new()],
            [[0.0; 3]; 3],
            false,
        ),
    };
    Precomputed {
        layer_xs,
        layer_ys,
        curves_ys,
        couplers_curves0_ys,
        couplers_matrix,
        couplers_active,
    }
}

/// 钳制插值（切片参数版，供预计算路径复用）。
#[inline]
pub fn interp_clamped_scalar(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let k = xs.len();
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[k - 1] {
        return ys[k - 1];
    }
    let mut lo = 0usize;
    let mut hi = k;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if xs[mid] <= x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let low = lo - 1;
    let t = (x - xs[low]) / (xs[low + 1] - xs[low]);
    ys[low] + t * (ys[low + 1] - ys[low])
}

/// 逐像素显影（零分配；pre 为管线级预计算）。
pub fn develop_pre(log_raw: [f64; 3], profile: &FilmProfile, pre: &Precomputed) -> [f64; 3] {
    let le = &profile.data.log_exposure;
    let mut cmy = [0.0f64; 3];
    for c in 0..3 {
        cmy[c] = interp_clamped_scalar(log_raw[c], le, &pre.curves_ys[c]);
    }
    if pre.couplers_active {
        let mut corr = [0.0f64; 3];
        for m in 0..3 {
            for k in 0..3 {
                corr[m] += cmy[k] * pre.couplers_matrix[k][m];
            }
        }
        for c in 0..3 {
            cmy[c] = interp_clamped_scalar(log_raw[c] - corr[c], le, &pre.couplers_curves0_ys[c]);
        }
    }
    cmy
}
