//! 颗粒模型（M2-E）——sublayers 路径的确定性数学 + 导出随机场消费。
//!
//! 移植自上游 model/grain.py 的 apply_grain → apply_grain_to_density_layers
//! → layer_particle_model（poisson_binomial 方法）。
//!
//! 随机性处理：上游用 numpy legacy RandomState（每 (ch,sl) 调用前
//! np.random.seed(ch + sl*10)，抽样顺序 poisson → binomial）。Rust 侧不复制
//! numpy 的 RNG 流，而是消费 oracle 导出的随机场（seeds/devs 数组）做位级对拍；
//! 生产路径用自有 PRNG（统计等价，颗粒观感一致）。
//!
//! 空间模糊（染料云 blur、外层 blur）：lut_mode/测试配置下均为 0 或在测试
//! 像素尺寸下退化为恒等（radius=0），对拍不含空间卷积；生产版高斯 FIR 在
//! [`gaussian_fir`] 提供。

use crate::types::FilmProfile;

/// 单 (通道, 子层) 的随机场（oracle 导出）。
#[derive(Clone, Debug)]
pub struct GrainRandomField {
    pub seeds: Vec<f64>,
    pub devs: Vec<f64>,
    pub density_max: f64,
    pub n_particles: f64,
    pub uniformity: f64,
}

/// 一张图的全部随机场：索引 [(ch,sl)]，ch 外层、sl 内层（0_0,0_1,0_2,1_0,…）。
#[derive(Clone, Debug)]
pub struct GrainFields {
    pub fields: Vec<GrainRandomField>, // 6 项（3ch × 3sl）
}

impl GrainFields {
    /// 从 golden_grain.json 的 fields 节点加载。
    pub fn from_json(v: &serde_json::Value) -> Option<GrainFields> {
        let mut fields = Vec::new();
        for ch in 0..3usize {
            for sl in 0..3usize {
                let key = format!("{ch}_{sl}");
                let node = &v[&key];
                if node.is_null() {
                    return None;
                }
                let arr = |k: &str| -> Vec<f64> {
                    node[k]
                        .as_array()
                        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
                        .unwrap_or_default()
                };
                fields.push(GrainRandomField {
                    seeds: arr("seeds"),
                    devs: arr("devs"),
                    density_max: node["density_max"].as_f64().unwrap_or(0.0),
                    n_particles: node["n_particles"].as_f64().unwrap_or(1.0),
                    uniformity: node["uniformity"].as_f64().unwrap_or(0.98),
                });
            }
        }
        Some(GrainFields { fields })
    }
}

/// 颗粒参数（GrainParams 的 Rust 子集，digest 后值）。
#[derive(Clone, Debug)]
pub struct GrainConfig {
    pub particle_area_um2: f64,
    pub particle_scale: [f64; 3],
    pub particle_scale_layers: [f64; 3],
    pub density_min: [f64; 3],
    pub uniformity: [f64; 3],
}

impl GrainConfig {
    /// 上游 GrainParams 默认值。
    pub fn default_portra() -> Self {
        Self {
            particle_area_um2: 0.2,
            particle_scale: [1.6, 1.6, 3.2],
            particle_scale_layers: [2.0, 1.0, 0.5],
            density_min: [0.03, 0.03, 0.03],
            uniformity: [0.97, 0.99, 0.97],
        }
    }
}

/// 钳制线性插值（fast_interp 语义）。
#[inline]
fn interp_clamped(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
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

/// NaN 忽略的最大值（np.nanmax）。
fn nanmax(values: impl Iterator<Item = f64>) -> f64 {
    let mut m = f64::NEG_INFINITY;
    for v in values {
        if v.is_finite() && v > m {
            m = v;
        }
    }
    if m.is_infinite() { f64::NAN } else { m }
}

/// interp_density_cmy_layers（负片分支）：plain interp(cmy, curves, layers)。
/// 上游 else 分支不取负——x 轴 = 归一化密度曲线（递增），查询点 = 像素密度。
/// （取负只出现在 positive_film 分支。）
fn interp_density_layers(
    cmy: [f64; 3],
    _log_exposure: &[f64],
    curves_norm: &[[f64; 3]],
    layers: &[[[f64; 3]; 3]],
) -> [[f64; 3]; 3] {
    let mut out = [[0.0f64; 3]; 3];
    for ch in 0..3 {
        let xs: Vec<f64> = curves_norm.iter().map(|r| r[ch]).collect();
        for sl in 0..3 {
            let ys: Vec<f64> = layers.iter().map(|row| row[sl][ch]).collect();
            out[sl][ch] = interp_clamped(cmy[ch], &xs, &ys);
        }
    }
    out
}

/// 逐像素层密度（零分配；layer_xs/layer_ys 为管线级预计算）。
#[inline]
pub fn layer_density_pre(
    cmy_ch: f64,
    sl: usize,
    ch: usize,
    layer_xs: &[Vec<f64>; 3],
    layer_ys: &[Vec<f64>; 9],
) -> f64 {
    crate::film::interp_clamped_scalar(cmy_ch, &layer_xs[ch], &layer_ys[sl * 3 + ch])
}

/// apply_grain（sublayers 路径，逐像素确定性 + 导出随机场）。
///
/// `cmy_pre`：成色剂校正后的密度（M1 develop 输出）；
/// `fields`：oracle 导出的随机场（与 profile/输入尺寸对应）；
/// `pixel_size_um`：ResizingService 语义 = film_format_mm·1000 / max(w,h)。
/// 模糊（dye-cloud/外层）在本路径为 0（oracle 配置），不实现空间卷积。
pub fn apply_grain_layers(
    cmy_pre: &[[f64; 3]],
    profile: &FilmProfile,
    pixel_size_um: f64,
    cfg: &GrainConfig,
    fields: &GrainFields,
) -> Vec<[f64; 3]> {
    let log_exposure = &profile.data.log_exposure;
    let curves_norm = crate::film::normalized_curves(profile);
    let layers = &profile.data.density_curves_layers; // 256 × [sl][ch]

    // 密度最大值（子层×通道）
    let mut max_layers = [[0.0f64; 3]; 3];
    for sl in 0..3 {
        for ch in 0..3 {
            max_layers[sl][ch] = nanmax(layers.iter().map(|row| row[sl][ch]));
        }
    }
    let total = [
        max_layers[0][ch_of(0)] + max_layers[1][ch_of(0)] + max_layers[2][ch_of(0)],
        max_layers[0][ch_of(1)] + max_layers[1][ch_of(1)] + max_layers[2][ch_of(1)],
        max_layers[0][ch_of(2)] + max_layers[1][ch_of(2)] + max_layers[2][ch_of(2)],
    ];
    let mut min_layers = [[0.0f64; 3]; 3];
    let mut max_with_min = [[0.0f64; 3]; 3];
    let mut n_pp = [[0.0f64; 3]; 3];
    let pixel_area = pixel_size_um * pixel_size_um;
    for sl in 0..3 {
        for ch in 0..3 {
            let fr = max_layers[sl][ch] / total[ch];
            min_layers[sl][ch] = fr * cfg.density_min[ch];
            max_with_min[sl][ch] = max_layers[sl][ch] + min_layers[sl][ch];
            let area = cfg.particle_area_um2 * cfg.particle_scale[ch] * cfg.particle_scale_layers[sl];
            n_pp[sl][ch] = pixel_area * fr / area;
        }
    }

    cmy_pre
        .iter()
        .enumerate()
        .map(|(px, &cmy)| {
            let cmy_layers = interp_density_layers(cmy, log_exposure, &curves_norm, layers);
            let mut out = [0.0f64; 3];
            for ch in 0..3 {
                let mut acc = 0.0f64;
                for sl in 0..3 {
                    let f = &fields.fields[ch * 3 + sl];
                    let dens = cmy_layers[sl][ch] + min_layers[sl][ch];
                    let p = (dens / f.density_max).clamp(1e-6, 1.0 - 1e-6);
                    let sat = 1.0 - p * f.uniformity * (1.0 - 1e-6);
                    let od = f.density_max / f.n_particles;
                    let dev = f.devs.get(px).copied().unwrap_or(0.0);
                    acc += dev * od * sat;
                }
                out[ch] = acc - cfg.density_min[ch];
            }
            out
        })
        .collect()
}

#[inline]
fn ch_of(i: usize) -> usize {
    i
}

/// 生产版高斯 FIR（scipy truncate=4.0 + reflect 边界；σ<=0 原样返回）。
/// 大 σ 的 IIR 路径（Young & van Vliet）暂未实现——生产如需大 σ 再补。
pub fn gaussian_fir_1d_signal(signal: &[f64], sigma: f64, truncate: f64) -> Vec<f64> {
    if sigma <= 0.0 {
        return signal.to_vec();
    }
    let radius = (truncate * sigma + 0.5) as i64 as usize;
    let size = 2 * radius + 1;
    let mut kernel = Vec::with_capacity(size);
    let mut total = 0.0;
    for i in 0..size {
        let x = i as f64 - radius as f64;
        let v = (-0.5 * (x / sigma).powi(2)).exp();
        kernel.push(v);
        total += v;
    }
    for k in &mut kernel {
        *k /= total;
    }
    let n = signal.len();
    let reflect = |i: i64| -> usize {
        let nn = n as i64;
        let mut i = i.rem_euclid(2 * nn);
        if i >= nn {
            i = 2 * nn - 1 - i;
        }
        i as usize
    };
    (0..n)
        .map(|j| {
            let mut acc = 0.0;
            for k in -(radius as i64)..=(radius as i64) {
                acc += signal[reflect(j as i64 + k)] * kernel[(k + radius as i64) as usize];
            }
            acc
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_fir_identity_at_zero_sigma() {
        let s = vec![1.0, 2.0, 3.0];
        assert_eq!(gaussian_fir_1d_signal(&s, 0.0, 4.0), s);
    }

    #[test]
    fn gaussian_fir_preserves_constant() {
        let s = vec![2.5; 32];
        let out = gaussian_fir_1d_signal(&s, 1.5, 4.0);
        for v in out {
            assert!((v - 2.5).abs() < 1e-12);
        }
    }
}

#[cfg(test)]
mod debug_grain {
    use super::*;
    use crate::profile::{load_profile, load_tables};
    use crate::oracle_dir;

    #[test]
    #[ignore = "手工 bisect 用"]
    fn debug_first_pixel() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../oracle-data/golden_grain.json")).unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let profile = crate::profile::load_profile("kodak_portra_400").unwrap();
        let _ = load_tables(&oracle_dir().display().to_string());

        let le: Vec<f64> = v["ramp"]["log_e_film"].as_array().unwrap()[0..3]
            .iter().map(|row| row[1].as_f64().unwrap()).collect();
        println!("le(G 通道前3) = {le:?}");

        let fields = GrainFields::from_json(&v["fields"]["ramp"]).unwrap();
        let cfg = GrainConfig::default_portra();
        let pixel_size = v["ramp"]["pixel_size_um"].as_f64().unwrap();
        println!("pixel_size = {pixel_size}");

        // 复现 cmy_pre（develop，couplers off）
        let log_exposure = &profile.data.log_exposure;
        let curves_norm = crate::film::normalized_curves(&profile);
        let cmy0_le = v["ramp"]["log_e_film"].as_array().unwrap()[0]
            .as_array().unwrap()
            .iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
        let cmy = [
            interp_clamped(cmy0_le[0], log_exposure, &curves_norm.iter().map(|r| r[0]).collect::<Vec<_>>()),
            interp_clamped(cmy0_le[1], log_exposure, &curves_norm.iter().map(|r| r[1]).collect::<Vec<_>>()),
            interp_clamped(cmy0_le[2], log_exposure, &curves_norm.iter().map(|r| r[2]).collect::<Vec<_>>()),
        ];
        println!("cmy_pre[0] = {cmy:?}");

        let layers = &profile.data.density_curves_layers;
        for ch in 0..3 {
            for sl in 0..3 {
                let f = &fields.fields[ch * 3 + sl];
                let neg_curve: Vec<f64> = curves_norm.iter().map(|r| -r[ch]).collect();
                let ys: Vec<f64> = layers.iter().map(|row| row[sl][ch]).collect();
                let layer_d = interp_clamped(-cmy[ch], &neg_curve, &ys);
                let min_l = {
                    let mut mx = f64::NEG_INFINITY;
                    for row in layers { mx = mx.max(row[sl][ch]); }
                    let total: f64 = (0..3).map(|s2| {
                        let mut m2 = f64::NEG_INFINITY;
                        for row in layers { m2 = m2.max(row[s2][ch]); }
                        m2
                    }).sum();
                    (mx / total) * cfg.density_min[ch]
                };
                let dens = layer_d + min_l;
                let p = (dens / f.density_max).clamp(1e-6, 1.0 - 1e-6);
                let sat = 1.0 - p * f.uniformity * (1.0 - 1e-6);
                let od = f.density_max / f.n_particles;
                let dev = f.devs.first().copied().unwrap_or(f64::NAN);
                let seedsv = f.seeds.first().copied().unwrap_or(f64::NAN);
                println!("ch{ch}sl{sl}: layer_d={layer_d:.6} min_l={min_l:.6} p={p:.6} sat={sat:.6} lam≈{:.1} seeds[0]={seedsv} devs[0]={dev} od={od:.3e} contrib={:.6}", f.n_particles / sat, dev * od * sat);
            }
        }
        let gold = v["ramp"]["cmy_film"].as_array().unwrap()[0]
            .as_array().unwrap()
            .iter().map(|x| x.as_f64().unwrap()).collect::<Vec<_>>();
        println!("golden cmy[0] = {gold:?}");
        // 逐层贡献对比（新 interp）
        let fields = GrainFields::from_json(&v["fields"]["ramp"]).unwrap();
        let cfg = GrainConfig::default_portra();
        let neg_curve0: Vec<f64> = Vec::new();
        let _ = neg_curve0;
        let mut total = [0.0f64; 3];
        for ch in 0..3 {
            for sl in 0..3 {
                let f = &fields.fields[ch * 3 + sl];
                // 用修复后的 interp 重算 layer_d（cmy 直接查）
                let log_exposure = &profile.data.log_exposure;
                let curves_norm = crate::film::normalized_curves(&profile);
                let xs: Vec<f64> = curves_norm.iter().map(|r| r[ch]).collect();
                let ys: Vec<f64> = profile.data.density_curves_layers.iter().map(|row| row[sl][ch]).collect();
                let layer_d = interp_clamped(cmy0_le[ch], &xs, &ys);
                let min_l = f.density_max * 0.0; // 占位：真实 min_l 见下
                let _ = min_l;
                // 从 fields 反推：devs[0] 已知，od = density_max/n_particles
                let od = f.density_max / f.n_particles;
                // p/sat 需 density_in：用 golden 近似反推不再可靠，打印原始量
                let dev0 = f.devs[0];
                let contrib = dev0 * od; // sat≈? 打印 dev*od 项
                println!("ch{ch}sl{sl}: layer_d={layer_d:.6} dev0={dev0:.0} od={od:.3e} dev*od={contrib:.6}");
                total[ch] += contrib;
            }
        }
        println!("sum(dev*od) = {total:?}  (golden+min = [{:.6} {:.6} {:.6}])",
            gold[0] + 0.03, gold[1] + 0.03, gold[2] + 0.03);
    }
}
