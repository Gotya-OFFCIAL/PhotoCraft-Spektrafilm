//! 相纸放大链（printing stage）：负片 CMY 密度 → 放大机滤色曝光 → 相纸显影 → 扫描输出。
//!
//! 移植自上游 runtime/stages/printing.py、services/filter_enlarger_source.py、
//! model/color_filters.py（custom 二向色滤镜，解析 erf 公式）、utils/morph_curves.py
//! （morph 关闭时的参数化 CDF 模型求值）。
//! M2-B 配置（与 oracle 一致）：lut_mode、glare 关、gamut 压缩关、黑白校正关。

use crate::film;
use crate::types::{CdfModel, FilmProfile, Tables};
use crate::upsampling;

/// 二向色滤镜透过率（C/M/Y 三列），custom 解析模型（create_combined_dichroic_filter）。
pub struct DichroicCustom {
    /// 81×3，列序 C、M、Y
    pub filters: [[f64; 3]; 81],
}

impl DichroicCustom {
    /// edges=[516,500,610,607], transitions=[12,8,8,8]（上游默认 custom 参数）
    pub fn new(wavelengths: &[f64]) -> Self {
        let edges = [516.0f64, 500.0, 610.0, 607.0];
        let tr = [12.0f64, 8.0, 8.0, 8.0];
        let mut filters = [[0.0f64; 3]; 81];
        for (i, &wl) in wavelengths.iter().enumerate() {
            // Y（index 2）
            filters[i][2] = libm::erf((wl - edges[0]) / tr[0]);
            // M（index 1）：550nm 分段
            filters[i][1] = if wl <= 550.0 {
                -libm::erf((wl - edges[1]) / tr[1])
            } else {
                libm::erf((wl - edges[2]) / tr[2])
            };
            // C（index 0）
            filters[i][0] = -libm::erf((wl - edges[3]) / tr[3]);
            for c in 0..3 {
                filters[i][c] = filters[i][c] / 2.0 + 0.5;
            }
        }
        Self { filters }
    }

    /// apply_cc：CC 值（C,M,Y 序，100 单位 = 1.0 密度）→ 滤色后光源。
    pub fn apply_cc(&self, illuminant: &[f64], cc: [f64; 3]) -> [f64; 81] {
        // filter_transmittance_values = 10^-(cc/100)
        let t = [10f64.powf(-cc[0] / 100.0), 10f64.powf(-cc[1] / 100.0), 10f64.powf(-cc[2] / 100.0)];
        let mut out = [0.0f64; 81];
        for l in 0..81 {
            let mut total = 1.0f64;
            for m in 0..3 {
                // dimmed = 1 - (1-filter)·(1-t)（Durst 滤光轮语义）
                total *= 1.0 - (1.0 - self.filters[l][m]) * (1.0 - t[m]);
            }
            out[l] = illuminant[l] * total;
        }
        out
    }
}

/// 放大机默认滤色（EnlargerParams 默认：c=0, m=65, y=55，无 shift）。
pub const DEFAULT_CC: [f64; 3] = [0.0, 65.0, 55.0];

fn cached_neutral_filters() -> Option<std::sync::Arc<serde_json::Value>> {
    static TABLE: std::sync::Mutex<Option<(std::path::PathBuf, std::sync::Arc<serde_json::Value>)>> =
        std::sync::Mutex::new(None);
    let path = crate::assets::oracle_dir().join("filters").join("neutral_print_filters.json");
    {
        let guard = TABLE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached_path, val)) = guard.as_ref() {
            if *cached_path == path {
                return Some(std::sync::Arc::clone(val));
            }
        }
    }
    let text = std::fs::read_to_string(&path).ok()?;
    let val: serde_json::Value = serde_json::from_str(&text).ok()?;
    let arc = std::sync::Arc::new(val);
    let mut guard = TABLE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((path, std::sync::Arc::clone(&arc)));
    Some(arc)
}

/// 解析放大机中性滤色（digest_params → apply_database_neutral_print_filters 语义）：
/// 按 (相纸, 光源, 胶片) 三元组查 neutral_print_filters.json，查到则替换默认值，否则用 DEFAULT_CC。
/// 首次成功读取后缓存于内存，避免 UI 每帧查询引发磁盘 I/O。
pub fn resolve_print_filters(paper_stock: &str, illuminant: &str, film_stock: &str) -> [f64; 3] {
    let Some(v) = cached_neutral_filters() else {
        return DEFAULT_CC;
    };
    let entry = &v[paper_stock][illuminant][film_stock];
    match entry.as_array() {
        Some(a) if a.len() == 3 => {
            let c = a[0].as_f64().unwrap_or(DEFAULT_CC[0]);
            let m = a[1].as_f64().unwrap_or(DEFAULT_CC[1]);
            let y = a[2].as_f64().unwrap_or(DEFAULT_CC[2]);
            [c, m, y]
        }
        _ => DEFAULT_CC,
    }
}

/// 标准正态 CDF（scipy norm.cdf 对应实现）。
#[inline]
fn norm_cdf(z: f64) -> f64 {
    0.5 * (1.0 + libm::erf(z / core::f64::consts::SQRT_2))
}

/// 参数化 CDF 模型在 log_exposure 轴上求值（apply_print_curves_morph 的 active=False 路径）。
/// 相纸 type='negative' → signed_z = z。
pub fn evaluate_fitted_density(model: &CdfModel, log_exposure: &[f64]) -> Vec<[f64; 3]> {
    log_exposure
        .iter()
        .map(|&x| {
            let mut out = [0.0f64; 3];
            for c in 0..3 {
                let mut acc = 0.0f64;
                for i in 0..3 {
                    let z = (x - model.centers[c][i]) / model.sigmas[c][i];
                    acc += model.amplitudes[c][i] * norm_cdf(z);
                }
                out[c] = acc;
            }
            out
        })
        .collect()
}

/// 在（未归一化的）任意 256 点曲线上做钳制线性插值（interpolate_exposure_to_density，gamma=1）。
pub fn interp_on_curves(x: f64, log_exposure: &[f64], curves: &[[f64; 3]], c: usize) -> f64 {
    let k = log_exposure.len();
    if x <= log_exposure[0] {
        return curves[0][c];
    }
    if x >= log_exposure[k - 1] {
        return curves[k - 1][c];
    }
    let mut lo = 0usize;
    let mut hi = k;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if log_exposure[mid] <= x {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    let low = lo - 1;
    let t = (x - log_exposure[low]) / (log_exposure[low + 1] - log_exposure[low]);
    curves[low][c] + t * (curves[low + 1][c] - curves[low][c])
}

/// print 显影（printing.develop → develop_print_morph，morph 关闭）。
pub fn develop_print(log_raw: [f64; 3], paper: &FilmProfile) -> [f64; 3] {
    let morphed = evaluate_fitted_density(&paper.data.density_curves_model, &paper.data.log_exposure);
    let mut out = [0.0f64; 3];
    for c in 0..3 {
        out[c] = interp_on_curves(log_raw[c], &paper.data.log_exposure, &morphed, c);
    }
    out
}

/// 批量 print 显影（预求值 CDF 曲线一次）。
pub fn develop_print_batch(log_raw: &[[f64; 3]], paper: &FilmProfile) -> Vec<[f64; 3]> {
    let morphed = evaluate_fitted_density(&paper.data.density_curves_model, &paper.data.log_exposure);
    log_raw
        .iter()
        .map(|px| {
            let mut out = [0.0f64; 3];
            for c in 0..3 {
                out[c] = interp_on_curves(px[c], &paper.data.log_exposure, &morphed, c);
            }
            out
        })
        .collect()
}

/// 放大机滤色光源（TH-KG3 + 指定 CC 滤色）。
pub fn enlarger_illuminant_cc(tables: &Tables, cc: [f64; 3]) -> [f64; 81] {
    let light_source = tables
        .illuminant("TH-KG3")
        .cloned()
        .expect("TH-KG3 illuminant");
    let wavelengths: Vec<f64> = (0..81).map(|i| 380.0 + i as f64 * 5.0).collect();
    let dichroic = DichroicCustom::new(&wavelengths);
    dichroic.apply_cc(&light_source, cc)
}

/// 预计算放大机滤色光源 + 相纸灵敏度（管线级一次），逐像素零分配。
pub struct PrintExposePre {
    pub print_illu: [f64; 81],
    pub sens: Vec<[f64; 3]>,
}

pub fn precompute_print_expose(
    paper: &FilmProfile,
    tables: &Tables,
    cc: [f64; 3],
) -> PrintExposePre {
    let print_illu = enlarger_illuminant_cc(tables, cc);
    let sens = paper.sensitivity_linear();
    PrintExposePre { print_illu, sens }
}

/// 中灰曝光因子（printing._compute_exposure_factor_midgray，
/// lut_mode：compensation=False、normalize=True → 返回 factor_midgray）。
///
/// 链路：0.184 线性 sRGB → rgb_to_raw → log10 → develop_simple（未归一化曲线！）
/// → 负片光谱密度 → 放大机光源下相纸 raw → 三通道几何均值的倒数。
pub fn compute_print_exposure_factor(
    film_profile: &FilmProfile,
    paper: &FilmProfile,
    tables: &Tables,
    raw_lut: &crate::types::RawLut,
    cc: [f64; 3],
) -> f64 {
    // 1) 中灰 raw 曝光（注意：_simple_rgb_to_density_spectral 用 log10(raw + 1e-10)，无 fmax）
    let raw = upsampling::rgb_to_raw([0.184; 3], raw_lut, tables);
    let log_raw = [(raw[0] + 1e-10).log10(), (raw[1] + 1e-10).log10(), (raw[2] + 1e-10).log10()];
    // 2) 未归一化曲线显影
    let density_cmy = film::develop_simple(log_raw, film_profile);
    // 3) 负片光谱密度
    let ds = film::density_spectral(density_cmy, film_profile);
    // 4) 放大机光源 → 相纸 raw
    let print_illu = enlarger_illuminant_cc(tables, cc);
    let sens = paper.sensitivity_linear();
    let mut raw_midgray = [0.0f64; 3];
    for l in 0..81 {
        if ds[l].is_nan() {
            continue; // 不透光
        }
        let light = print_illu[l] * 10f64.powf(-ds[l]);
        for m in 0..3 {
            raw_midgray[m] += light * sens[l][m];
        }
    }
    // 5) fmax 1e-10 → 几何均值 → 倒数
    let mut log_sum = 0.0f64;
    for m in 0..3 {
        log_sum += raw_midgray[m].max(1e-10).ln();
    }
    1.0 / (log_sum / 3.0).exp()
}

/// print expose：负片 CMY 密度 → 相纸 log 曝光（printing._film_cmy_to_print_log_raw）。
pub fn film_cmy_to_print_log_raw(
    cmy: [f64; 3],
    film_profile: &FilmProfile,
    paper: &FilmProfile,
    tables: &Tables,
    factor_midgray: f64,
    cc: [f64; 3],
) -> [f64; 3] {
    let ds = film::density_spectral(cmy, film_profile);
    let print_illu = enlarger_illuminant_cc(tables, cc);
    let sens = paper.sensitivity_linear();
    let mut raw = [0.0f64; 3];
    for l in 0..81 {
        if ds[l].is_nan() {
            continue;
        }
        let light = print_illu[l] * 10f64.powf(-ds[l]);
        for m in 0..3 {
            raw[m] += light * sens[l][m];
        }
    }
    let mut out = [0.0f64; 3];
    for m in 0..3 {
        out[m] = ((raw[m] * factor_midgray).max(0.0f64) + 1e-10).log10();
    }
    out
}

/// print expose 完整步（printing.expose）：log_raw_print 有一次 10^ → ×曝光(1.0) → log10 往返，
/// 该往返把深黑的有效下限钳在 log10(1e-10) = −10（真实语义，必须复刻）。
pub fn print_expose(
    cmy: [f64; 3],
    film_profile: &FilmProfile,
    paper: &FilmProfile,
    tables: &Tables,
    factor_midgray: f64,
    cc: [f64; 3],
) -> [f64; 3] {
    let log_raw = film_cmy_to_print_log_raw(cmy, film_profile, paper, tables, factor_midgray, cc);
    let mut out = [0.0f64; 3];
    for m in 0..3 {
        out[m] = (10f64.powf(log_raw[m]).max(0.0) + 1e-10).log10();
    }
    out
}

/// print 扫描（scanning.py 非 scan_film 分支，glare 关、corrections 关、LUT 关）。
pub fn scan_print_rgb(cmy: [f64; 3], paper: &FilmProfile, tables: &Tables) -> [f64; 3] {
    scan_print_rgb_out(cmy, paper, tables, None)
}

/// print 扫描（带输出色域压缩）。
pub fn scan_print_rgb_out(
    cmy: [f64; 3],
    paper: &FilmProfile,
    tables: &Tables,
    output_gamut: Option<&crate::gamut::OutputGamutCompressor>,
) -> [f64; 3] {
    // 与 scan_film_rgb 相同公式，但数据源换成相纸
    let ds = film::density_spectral(cmy, paper);
    let ill = tables
        .illuminant(&paper.info.viewing_illuminant)
        .cloned()
        .unwrap_or_else(|| tables.illuminant("D50").cloned().expect("D50 illuminant"));
    let cmfs = &tables.cmfs;

    let mut norm = 0.0f64;
    for l in 0..81 {
        norm += ill[l] * cmfs[l][1];
    }

    let mut xyz = [0.0f64; 3];
    for l in 0..81 {
        if ds[l].is_nan() {
            continue;
        }
        let light = ill[l] * 10f64.powf(-ds[l]);
        xyz[0] += light * cmfs[l][0];
        xyz[1] += light * cmfs[l][1];
        xyz[2] += light * cmfs[l][2];
    }
    for v in &mut xyz {
        *v /= norm;
    }
    let mut xyz2 = [0.0f64; 3];
    for k in 0..3 {
        xyz2[k] = 10f64.powf((xyz[k].max(0.0) + 1e-10).log10());
    }
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
    [crate::film::encode_srgb_f64(rgb[0]),
     crate::film::encode_srgb_f64(rgb[1]),
     crate::film::encode_srgb_f64(rgb[2])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{load_profile, load_tables};
    use crate::oracle_dir;

    #[test]
    fn dichroic_filter_shapes() {
        let wavelengths: Vec<f64> = (0..81).map(|i| 380.0 + i as f64 * 5.0).collect();
        let d = DichroicCustom::new(&wavelengths);
        // C 滤镜在长波端应接近 0（青色滤红），Y 滤镜在短波端接近 0
        assert!(d.filters[80][0] < 0.01, "C@780nm={}", d.filters[80][0]);
        assert!(d.filters[0][2] < 0.01, "Y@380nm={}", d.filters[0][2]);
        // 全通（cc=0）时透过率≈滤镜本身乘积，直流范围 (0,1]
        let full = d.apply_cc(&vec![1.0; 81], [0.0, 0.0, 0.0]);
        assert!(full.iter().all(|v| *v > 0.0 && *v <= 1.0));
    }

    #[test]
    fn paper_profiles_load_with_model() {
        let paper = load_profile("kodak_portra_endura").unwrap();
        let m = &paper.data.density_curves_model;
        // amplitudes 非负、sigmas 正
        for c in 0..3 {
            for i in 0..3 {
                assert!(m.amplitudes[c][i] >= 0.0 || m.amplitudes[c][i].is_nan());
                assert!(m.sigmas[c][i] > 0.0, "sigma 必须为正");
            }
        }
    }

    #[test]
    fn cdf_curves_monotone_rising() {
        let paper = load_profile("kodak_portra_endura").unwrap();
        let curves = evaluate_fitted_density(&paper.data.density_curves_model, &paper.data.log_exposure);
        // 负片型相纸曲线应随 log 曝光单调上升
        for c in 0..3 {
            for i in 1..curves.len() {
                assert!(curves[i][c] >= curves[i - 1][c] - 1e-12, "c={c} i={i}");
            }
        }
    }

    #[test]
    fn resolve_print_filters_uses_bundled_and_caches() {
        let a = resolve_print_filters("kodak_portra_endura", "TH-KG3", "kodak_portra_400");
        let b = resolve_print_filters("kodak_portra_endura", "TH-KG3", "kodak_portra_400");
        assert_eq!(a, b);
        assert!(a[1].is_finite() && a[2].is_finite());
        let fallback = resolve_print_filters("nonexistent_paper", "TH-KG3", "nonexistent_film");
        assert_eq!(fallback, DEFAULT_CC);
    }
}

#[cfg(test)]
mod debug_print {
    use super::*;
    #[test]
    #[ignore = "手工 bisect 用"]
    fn debug_vs_oracle_intermediates() {
        let film = crate::profile::load_profile("kodak_portra_400").unwrap();
        let paper = crate::profile::load_profile("kodak_portra_endura").unwrap();
        let t = crate::profile::load_tables(&crate::oracle_dir().display().to_string()).unwrap();
        // debug bin: [print_illu(81), ds_mid(81)]
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/debug_print_intermediates_f64.bin")).unwrap();
        let vals: Vec<f64> = bytes.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect();
        let (illu_oracle, dsmid_oracle) = (&vals[0..81], &vals[81..162]);

        // 1) 我的滤色光源
        let mine = enlarger_illuminant_cc(&t, DEFAULT_CC);
        let mut max_illu = 0.0f64;
        for l in 0..81 { max_illu = max_illu.max((mine[l] - illu_oracle[l]).abs()); }
        println!("enlarger_illuminant max_diff = {max_illu:.3e}");

        // 2) 我的 midgray 密度谱（需要 raw_lut —— 用 oracle raw_lut 隔离）
        let raw_lut = crate::profile::load_raw_lut(&crate::oracle_dir().display().to_string()).unwrap();
        let raw = upsampling::rgb_to_raw([0.184; 3], &raw_lut, &t);
        let log_raw = [(raw[0] + 1e-10).log10(), (raw[1] + 1e-10).log10(), (raw[2] + 1e-10).log10()];
        let cmy = film::develop_simple(log_raw, &film);
        println!("midgray cmy = {cmy:?}");
        let ds = film::density_spectral(cmy, &film);
        let mut max_ds = 0.0f64;
        for l in 0..81 {
            let a = ds[l];
            let b = dsmid_oracle[l];
            if a.is_nan() && b.is_nan() { continue; }
            if a.is_nan() != b.is_nan() { println!("NaN mismatch at l={l}: mine={a} oracle={b}"); }
            max_ds = max_ds.max((a - b).abs());
        }
        println!("ds_midgray max_diff = {max_ds:.3e}");
    }
}
