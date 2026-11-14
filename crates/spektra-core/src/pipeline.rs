//! 负片直扫管线（M1 确定性逐像素路径）。
//!
//! 对应上游 `scan_film=True` + `debug.lut_mode=True` 的拓扑：
//! `rgb_in → filming.expose → filming.develop → scanning.scan_film → rgb_out`。
//! M1 关闭项（与 oracle 配置一致）：曝光补偿 0EV、halation boost 0、扩散/镜头模糊/滤镜关、
//! DIR 成色剂关、颗粒关、自动曝光关、扫描黑白校正关、gamut 压缩关。
//! 后续里程碑在此骨架上加回，每项都配独立对拍。

use crate::film;
use crate::types::{FilmProfile, RawLut, Tables};
use crate::upsampling;

/// f64 sRGB 分段解码（CLI 输入用；编码在 film.rs）。
pub fn decode_srgb_f64(c: f64) -> f64 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// filming.expose：线性 sRGB → log10 曝光。
/// 上游：`log_raw = log10(np.fmax(raw, 0.0) + 1e-10)`。
/// M1 中 raw 只由光谱上采样决定（补偿/boost/滤镜全 off）。
pub fn expose(rgb: [f64; 3], raw_lut: &RawLut, tables: &Tables) -> [f64; 3] {
    let raw = upsampling::rgb_to_raw(rgb, raw_lut, tables);
    let mut out = [0.0f64; 3];
    for i in 0..3 {
        out[i] = (raw[i].max(0.0) + 1e-10).log10();
    }
    out
}

/// 负片直扫管线：持有合成好的 raw LUT。
pub struct NegativeScanPipeline {
    pub profile: FilmProfile,
    pub tables: Tables,
    raw_lut: RawLut,
    /// DIR 成色剂（None = 关闭，M1 语义；Some = 开启，M2-C）
    pub couplers: Option<crate::couplers::DirCouplers>,
    /// 输出色域压缩（None = 关闭；Some = cam16ucs 默认，M2-D2）
    pub output_gamut: Option<std::sync::Arc<crate::gamut::OutputGamutCompressor>>,
    /// M3a：管线级预计算（零分配快速路径）
    pub pre: std::sync::Arc<film::Precomputed>,
    /// 颗粒随机场（None = 关闭；Some = oracle 导出场，M2-E）
    pub grain_fields: Option<std::sync::Arc<crate::grain::GrainFields>>,
    pub grain_cfg: crate::grain::GrainConfig,
    pub pixel_size_um: f64,
}

impl NegativeScanPipeline {
    /// 用光谱 LUT + 灵敏度 + erf4 窗合成 raw LUT（D55 参考光源）。
    /// 合成正确性由 upsampling 模块对 oracle raw_lut 的对拍测试保证。
    pub fn new(profile: FilmProfile, tables: Tables) -> Self {
        let sensitivity = profile.sensitivity_linear();
        let wl: Vec<f64> = profile.data.wavelengths.clone();
        let window: Vec<[f64; 3]> = match profile.data.window_params.len() {
            4 => {
                let mut wp = [0.0f64; 4];
                wp.copy_from_slice(&profile.data.window_params);
                upsampling::erf4_window(&wp, &wl).to_vec()
            }
            // 相纸/无窗口参数的 profile：单位窗（不施加带通）
            _ => vec![[1.0; 3]; wl.len()],
        };
        let illu_d55 = tables
            .illuminant(&profile.info.reference_illuminant)
            .cloned()
            .unwrap_or_else(|| tables.illuminant("D55").cloned().expect("D55 illuminant"));
        let raw_lut = upsampling::compose_raw_lut(&tables.spectra_lut, &sensitivity, &window, &illu_d55);
        let pre = std::sync::Arc::new(film::precompute(&profile, None, false));
        Self {
            profile,
            tables,
            raw_lut,
            couplers: None,
            output_gamut: None,
            pre,
            grain_fields: None,
            grain_cfg: crate::grain::GrainConfig::default_portra(),
            pixel_size_um: 0.0,
        }
    }

    /// 直接使用预合成 LUT（oracle raw_lut 校验用）。
    pub fn with_raw_lut(profile: FilmProfile, tables: Tables, raw_lut: RawLut) -> Self {
        let mut s = Self::new(profile, tables);
        s.raw_lut = raw_lut;
        s
    }

    /// 设置颗粒（M2-E）：随机场 + 像素尺寸（film_format_mm·1000/max(w,h)）。
    pub fn with_grain(mut self, fields: crate::grain::GrainFields, pixel_size_um: f64) -> Self {
        self.grain_fields = Some(std::sync::Arc::new(fields));
        self.pixel_size_um = pixel_size_um;
        self
    }

    /// 设置成色剂并同步重建预计算（M3a：curves0 依赖成色剂矩阵）。
    pub fn set_couplers(&mut self, c: Option<crate::couplers::DirCouplers>) {
        self.pre = std::sync::Arc::new(film::precompute(
            &self.profile,
            c.as_ref(),
            false,
        ));
        self.couplers = c;
    }

    /// 开启输入侧色域压缩（InputGamutCompressSpec 默认 knee=(0,1,6)），
    /// 重合成并烘焙 remap 到 raw LUT（M2-D1）。
    pub fn with_input_gamut(mut self, knee: (f64, f64, f64)) -> Self {
        let locus = crate::gamut::spectral_locus_xy(&self.tables.cmfs);
        let ref_xy = self.tables.illu_xy_d55;
        self.raw_lut = crate::gamut::remap_tc_lut_for_compression(
            &self.raw_lut, ref_xy, knee.0, knee.1, knee.2, &locus,
        );
        self
    }

    pub fn raw_lut(&self) -> &RawLut {
        &self.raw_lut
    }

    /// 单像素（px=0 便捷入口；颗粒消费 devs[0]）。
    pub fn process_pixel(&self, rgb_linear: [f64; 3]) -> [f64; 3] {
        self.process_pixel_at(0, rgb_linear)
    }

    /// 带像素索引的单像素（颗粒随机场按 px 取 devs）。
    pub fn process_pixel_at(&self, px: usize, rgb_linear: [f64; 3]) -> [f64; 3] {
        let log_raw = expose(rgb_linear, &self.raw_lut, &self.tables);
        let mut cmy = film::develop_pre(log_raw, &self.profile, &self.pre);
        if let Some(f) = &self.grain_fields {
            let mut out = [0.0f64; 3];
            for ch in 0..3 {
                let mut acc = 0.0f64;
                for sl in 0..3 {
                    let fld = &f.fields[ch * 3 + sl];
                    let dens = crate::grain::layer_density_pre(
                        cmy[ch], sl, ch, &self.pre.layer_xs, &self.pre.layer_ys,
                    ) + self.grain_cfg.density_min[ch];
                    let p = (dens / fld.density_max).clamp(1e-6, 1.0 - 1e-6);
                    let sat = 1.0 - p * fld.uniformity * (1.0 - 1e-6);
                    let od = fld.density_max / fld.n_particles;
                    let dev = fld.devs.get(px).copied().unwrap_or(0.0);
                    acc += dev * od * sat;
                }
                out[ch] = acc - self.grain_cfg.density_min[ch];
            }
            cmy = out;
        }
        film::scan_film_rgb_out(cmy, &self.profile, &self.tables, self.output_gamut.as_deref())
    }

    /// 批量：线性 sRGB 像素序列 → 负片直扫 → 编码 sRGB（rayon 并行，逐像素融合三阶段）。
    pub fn process_image(&self, linear: &[[f64; 3]]) -> Vec<[f64; 3]> {
        use rayon::prelude::*;
        linear
            .par_iter()
            .enumerate()
            .map(|(px, &rgb)| self.process_pixel_at(px, rgb))
            .collect()
    }
}

/// 完整暗房管线：负片 → 放大机 → 相纸 → 扫描（M2-B）。
pub struct PrintPipeline {
    pub film: FilmProfile,
    pub paper: FilmProfile,
    pub tables: Tables,
    raw_lut: RawLut,
    factor_midgray: f64,
    /// 数据库解析出的放大机中性滤色（print_stock × illuminant × film_stock）
    pub cc: [f64; 3],
    /// DIR 成色剂（作用在负片显影上；None = 关闭）
    pub couplers: Option<crate::couplers::DirCouplers>,
    /// 输出色域压缩（None = 关闭；Some = cam16ucs 默认，M2-D2）
    pub output_gamut: Option<std::sync::Arc<crate::gamut::OutputGamutCompressor>>,
    /// M3a：负片显影预计算（零分配快速路径）
    pub pre: std::sync::Arc<film::Precomputed>,
    /// M3b：print 显影 CDF 曲线缓存（256×3，构建一次）
    pub print_curves: Vec<[f64; 3]>,
    /// M3b：print expose 预计算（滤色光源 + 相纸灵敏度，构建一次）
    pub expose_pre: std::sync::Arc<crate::printing::PrintExposePre>,
}

impl PrintPipeline {
    pub fn new(film: FilmProfile, paper: FilmProfile, tables: Tables) -> Self {
        let sensitivity = film.sensitivity_linear();
        let wl: Vec<f64> = film.data.wavelengths.clone();
        let window: Vec<[f64; 3]> = match film.data.window_params.len() {
            4 => {
                let mut wp = [0.0f64; 4];
                wp.copy_from_slice(&film.data.window_params);
                crate::upsampling::erf4_window(&wp, &wl).to_vec()
            }
            _ => vec![[1.0; 3]; wl.len()],
        };
        let illu_d55 = tables
            .illuminant(&film.info.reference_illuminant)
            .cloned()
            .unwrap_or_else(|| tables.illuminant("D55").cloned().expect("D55"));
        let raw_lut =
            crate::upsampling::compose_raw_lut(&tables.spectra_lut, &sensitivity, &window, &illu_d55);
        let cc = crate::printing::resolve_print_filters(
            &paper.info.stock,
            "TH-KG3",
            &film.info.stock,
        );
        let factor_midgray =
            crate::printing::compute_print_exposure_factor(&film, &paper, &tables, &raw_lut, cc);
        let pre = std::sync::Arc::new(film::precompute(&film, None, false));
        let print_curves = crate::printing::evaluate_fitted_density(
            &paper.data.density_curves_model, &paper.data.log_exposure,
        );
        let expose_pre = std::sync::Arc::new(crate::printing::precompute_print_expose(&film, &tables, cc));
        Self {
            film,
            paper,
            tables,
            raw_lut,
            factor_midgray,
            cc,
            couplers: None,
            output_gamut: None,
            pre,
            print_curves,
            expose_pre,
        }
    }

    pub fn raw_lut(&self) -> &RawLut {
        &self.raw_lut
    }

    pub fn factor_midgray(&self) -> f64 {
        self.factor_midgray
    }

    /// 开启输入侧色域压缩并重算 factor_midgray（中灰链走压缩后的 LUT，与上游一致）。
    pub fn with_input_gamut(mut self, knee: (f64, f64, f64)) -> Self {
        let locus = crate::gamut::spectral_locus_xy(&self.tables.cmfs);
        self.raw_lut = crate::gamut::remap_tc_lut_for_compression(
            &self.raw_lut, self.tables.illu_xy_d55, knee.0, knee.1, knee.2, &locus,
        );
        self.factor_midgray = crate::printing::compute_print_exposure_factor(
            &self.film, &self.paper, &self.tables, &self.raw_lut, self.cc,
        );
        self
    }

    /// 设置成色剂并同步重建预计算。
    pub fn set_couplers(&mut self, c: Option<crate::couplers::DirCouplers>) {
        self.pre = std::sync::Arc::new(film::precompute(&self.film, c.as_ref(), false));
        self.couplers = c;
    }

    /// 单像素（px=0 便捷入口）。
    pub fn process_pixel(&self, rgb_linear: [f64; 3]) -> [f64; 3] {
        self.process_pixel_at(0, rgb_linear)
    }

    /// 带像素索引的单像素。
    pub fn process_pixel_at(&self, px: usize, rgb_linear: [f64; 3]) -> [f64; 3] {
        let _ = px;
        let log_e_film = expose(rgb_linear, &self.raw_lut, &self.tables);
        let cmy_film = film::develop_pre(log_e_film, &self.film, &self.pre);
        let log_e_print = self.print_expose_pre(cmy_film);
        let cmy_print = [
            film::interp_clamped_scalar(log_e_print[0], &self.paper.data.log_exposure,
                &self.print_curves.iter().map(|r| r[0]).collect::<Vec<_>>()),
            film::interp_clamped_scalar(log_e_print[1], &self.paper.data.log_exposure,
                &self.print_curves.iter().map(|r| r[1]).collect::<Vec<_>>()),
            film::interp_clamped_scalar(log_e_print[2], &self.paper.data.log_exposure,
                &self.print_curves.iter().map(|r| r[2]).collect::<Vec<_>>()),
        ];
        crate::printing::scan_print_rgb_out(cmy_print, &self.paper, &self.tables, self.output_gamut.as_deref())
    }

    /// 逐像素 print expose（零分配；expose_pre 为管线级预计算）。
    fn print_expose_pre(&self, cmy: [f64; 3]) -> [f64; 3] {
        let ds = film::density_spectral(cmy, &self.film);
        let illu = &self.expose_pre.print_illu;
        let sens = &self.expose_pre.sens;
        let mut raw = [0.0f64; 3];
        for l in 0..81 {
            if ds[l].is_nan() { continue; }
            let light = illu[l] * 10f64.powf(-ds[l]);
            for m in 0..3 {
                raw[m] += light * sens[l][m];
            }
        }
        let mut out = [0.0f64; 3];
        for m in 0..3 {
            out[m] = ((raw[m] * self.factor_midgray).max(0.0) + 1e-10).log10();
            out[m] = (10f64.powf(out[m]).max(0.0) + 1e-10).log10();
        }
        out
    }

    /// 批量（M2-B 已验证分阶段函数 + rayon 并行）。
    pub fn process_image(&self, linear: &[[f64; 3]]) -> Vec<[f64; 3]> {
        use rayon::prelude::*;
        let log_e: Vec<[f64; 3]> = linear
            .par_iter()
            .map(|&rgb| expose(rgb, &self.raw_lut, &self.tables))
            .collect();
        let cmy_film: Vec<[f64; 3]> = log_e
            .par_iter()
            .map(|&lr| film::develop_pre(lr, &self.film, &self.pre))
            .collect();
        let log_e_print: Vec<[f64; 3]> = cmy_film
            .par_iter()
            .map(|&cmy| {
                crate::printing::print_expose(
                    cmy, &self.film, &self.paper, &self.tables,
                    self.factor_midgray, self.cc,
                )
            })
            .collect();
        let cmy_print = crate::printing::develop_print_batch(&log_e_print, &self.paper);
        cmy_print
            .par_iter()
            .map(|&cmy| crate::printing::scan_print_rgb_out(cmy, &self.paper, &self.tables, self.output_gamut.as_deref()))
            .collect()
    }
}
