//! 共享数据契约 —— 所有模块以此为准，不得另立结构。
//!
//! 全部数值域 f64（与上游 Python/NumPy float64 对齐）。
//! JSON 中的 null（源自上游 NaN 序列化）一律还原为 f64::NAN，
//! 并沿用上游语义：NaN 密度 → 透过率为 0（不透光）。

/// 胶片/相纸基本信息（JSON `info` 段，Rust 端只保留用到的字段）。
#[derive(Clone, Debug)]
pub struct ProfileInfo {
    pub stock: String,
    pub name: String,
    /// "negative" | "positive"
    pub film_type: String,
    /// "film" | "paper"
    pub support: String,
    pub channel_model: String,
    /// 负片拍摄参考光源，如 "D55"
    pub reference_illuminant: String,
    /// 扫描 viewing 光源，如 "D50"
    pub viewing_illuminant: String,
}

/// 胶片/相纸光谱数据（JSON `data` 段）。
#[derive(Clone, Debug)]
pub struct ProfileData {
    /// 81 点波长（nm），380–780 @5nm
    pub wavelengths: Vec<f64>,
    /// 81×3 层光谱灵敏度（log10 域，可含 NaN）
    pub log_sensitivity: Vec<[f64; 3]>,
    /// hanatos2025 光谱带通窗口参数：4 值 = erf4 (c_uv, σ_uv, c_ir, σ_ir)
    pub window_params: Vec<f64>,
    /// hanatos2025 log 曝光校正面参数：3×15（poly4_warp_xy：14 多项式 + 1 α）
    pub surface_params: Vec<[f64; 15]>,
    /// 81×3 每通道染料光谱密度
    pub channel_density: Vec<[f64; 3]>,
    /// 81 基底密度（可含 NaN → 不透光语义）
    pub base_density: Vec<f64>,
    /// 81 中灰中性密度
    pub midscale_neutral_density: Vec<f64>,
    /// 256 点 log 曝光轴（linspace(-3,4,256)）
    pub log_exposure: Vec<f64>,
    /// 256×3 特性曲线（CMY 密度，可含 NaN）
    pub density_curves: Vec<[f64; 3]>,
    /// 256×3×3 分层特性曲线
    pub density_curves_layers: Vec<[[f64; 3]; 3]>,
    /// 参数化 CDF 显影模型（相纸 print develop 用）
    pub density_curves_model: CdfModel,
}

/// 相纸显影曲线的参数化 CDF 模型（JSON `density_curves_model`）。
/// 每通道 3 个子层：D(x) = Σ_i A[c][i]·Φ((x−μ[c][i])/σ[c][i])。
#[derive(Clone, Debug, Default)]
pub struct CdfModel {
    pub centers: [[f64; 3]; 3],
    pub amplitudes: [[f64; 3]; 3],
    pub sigmas: [[f64; 3]; 3],
}

/// 完整 profile。
#[derive(Clone, Debug)]
pub struct FilmProfile {
    pub info: ProfileInfo,
    pub data: ProfileData,
}

impl FilmProfile {
    /// 灵敏度线性域：10^log_sensitivity，NaN→0（上游 `np.nan_to_num(10**log_sensitivity)`）。
    pub fn sensitivity_linear(&self) -> Vec<[f64; 3]> {
        self.data
            .log_sensitivity
            .iter()
            .map(|row| {
                [
                    if row[0].is_nan() { 0.0 } else { 10.0f64.powf(row[0]) },
                    if row[1].is_nan() { 0.0 } else { 10.0f64.powf(row[1]) },
                    if row[2].is_nan() { 0.0 } else { 10.0f64.powf(row[2]) },
                ]
            })
            .collect()
    }
}

/// 从 oracle 导出的静态表（engine/oracle-data/）。
#[derive(Clone, Debug)]
pub struct Tables {
    /// CIE 1931 2° CMF，81×3
    pub cmfs: Vec<[f64; 3]>,
    /// 光源 SPD：["D55", "D50"] 各 81
    pub illuminants: Vec<(String, Vec<f64>)>,
    /// hanatos 光谱 LUT：N×N×81（192×192×81）
    pub spectra_lut: SpectraLut,
    /// 线性 sRGB → XYZ(D55, CAT16) 合成矩阵
    pub mat_rgb_to_xyz_d55: [[f64; 3]; 3],
    /// XYZ(viewing D50) → 线性 sRGB（CAT02）合成矩阵（含 RGB_to_RGB 往返，输出压缩关闭时用）
    pub mat_xyz_to_rgb_view: [[f64; 3]; 3],
    /// XYZ(viewing D50) → 线性 sRGB（CAT02，纯矩阵，无往返；输出压缩开启时用）
    pub mat_xyz_to_rgb_view_pure: [[f64; 3]; 3],
    /// sRGB XYZ → 线性 RGB（4 位小数官方矩阵）
    pub srgb_xyz_to_rgb: [[f64; 3]; 3],
    /// sRGB 线性 RGB → XYZ（4 位小数官方矩阵）
    pub srgb_rgb_to_xyz: [[f64; 3]; 3],
    /// sRGB RGB_to_RGB 往返矩阵（M_out·M_in，编码步隐式应用）
    pub srgb_rgb_roundtrip: [[f64; 3]; 3],
    /// D55 色度 xy
    pub illu_xy_d55: [f64; 2],
    /// viewing（D50）色度 xy
    pub illu_xy_d50: [f64; 2],
    /// sRGB 白点 xy（输出压缩用）
    pub srgb_white_xy: [f64; 2],
}

impl Tables {
    pub fn illuminant(&self, name: &str) -> Option<&Vec<f64>> {
        self.illuminants.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// N×N×81 行主序光谱 LUT（C 顺序：i 行、j 列、l 波长）。
#[derive(Clone, Debug)]
pub struct SpectraLut {
    pub n: usize,
    /// 长度 n*n*81
    pub data: Vec<f64>,
}

impl SpectraLut {
    #[inline]
    pub fn at(&self, i: usize, j: usize, l: usize) -> f64 {
        self.data[(i * self.n + j) * 81 + l]
    }
}

/// N×N×3 raw LUT（已合成灵敏度）。
#[derive(Clone, Debug)]
pub struct RawLut {
    pub n: usize,
    /// 长度 n*n*3
    pub data: Vec<[f64; 3]>,
}

impl RawLut {
    #[inline]
    pub fn at(&self, i: usize, j: usize) -> [f64; 3] {
        self.data[i * self.n + j]
    }
}
