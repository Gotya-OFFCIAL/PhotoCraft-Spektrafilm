//! DIR 成色剂（development inhibitor releasing couplers）逐像素密度校正。
//!
//! 移植自上游 model/couplers.py。lut_mode（空间扩散关）下为纯逐像素确定性逻辑：
//! 显影抑制剂矩阵作用于 silver 密度 → log 曝光校正 → 在「成色剂作用前」的
//! 密度曲线上重新插值。空间扩散（gaussian/exponential 滤镜）在 M2-C 不实现
//! （oracle 配置 diffusion_size_um=0 时被上游短路灯）。

/// DIR 成色剂参数（digest 后值：默认值经 _apply_film_specifics 按 sign 覆盖 gamma）。
#[derive(Clone, Debug)]
pub struct DirCouplers {
    pub amount: f64,
    pub inhibition_samelayer: f64,
    pub inhibition_interlayer: f64,
    /// 同层抑制 gamma（R,G,B）
    pub gamma_samelayer_rgb: [f64; 3],
    /// 跨层抑制 gamma：donor R → receiver G,B
    pub gamma_interlayer_r_to_gb: [f64; 2],
    /// donor G → receiver R,B
    pub gamma_interlayer_g_to_rb: [f64; 2],
    /// donor B → receiver R,G
    pub gamma_interlayer_b_to_rg: [f64; 2],
}

impl DirCouplers {
    /// 负片 gamma（_apply_film_specifics negative 分支，Portra/Vision3 等通用）。
    pub fn negative() -> Self {
        Self {
            amount: 1.0,
            inhibition_samelayer: 1.0,
            inhibition_interlayer: 1.0,
            gamma_samelayer_rgb: [0.336, 0.319, 0.273],
            gamma_interlayer_r_to_gb: [0.353, 0.302],
            gamma_interlayer_g_to_rb: [0.154, 0.353],
            gamma_interlayer_b_to_rg: [0.168, 0.226],
        }
    }

    /// 正片 gamma（_apply_film_specifics positive 分支，Provia/Velvia 等通用）。
    pub fn positive() -> Self {
        Self {
            amount: 1.0,
            inhibition_samelayer: 1.0,
            inhibition_interlayer: 1.0,
            gamma_samelayer_rgb: [0.12, 0.08, 0.06],
            gamma_interlayer_r_to_gb: [0.12, 0.06],
            gamma_interlayer_g_to_rb: [0.08, 0.06],
            gamma_interlayer_b_to_rg: [0.06, 0.06],
        }
    }

    /// 抑制剂矩阵（compute_dir_couplers_matrix）：行=供体层，列=接收层，×amount。
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        let mut m = [[0.0f64; 3]; 3];
        for c in 0..3 {
            m[c][c] = self.gamma_samelayer_rgb[c] * self.inhibition_samelayer;
        }
        // M_inter[donor][receiver]
        m[0][1] = self.gamma_interlayer_r_to_gb[0];
        m[0][2] = self.gamma_interlayer_r_to_gb[1];
        m[1][0] = self.gamma_interlayer_g_to_rb[0];
        m[1][2] = self.gamma_interlayer_g_to_rb[1];
        m[2][0] = self.gamma_interlayer_b_to_rg[0];
        m[2][1] = self.gamma_interlayer_b_to_rg[1];
        for r in 0..3 {
            for c in 0..3 {
                if r != c {
                    m[r][c] *= self.inhibition_interlayer;
                }
            }
        }
        for r in 0..3 {
            for c in 0..3 {
                m[r][c] *= self.amount;
            }
        }
        m
    }
}

/// 钳制线性插值（np.interp 语义；le0 需单调递增——对常规胶片数据成立）。
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

/// compute_density_curves_before_dir_couplers：求「成色剂作用前」的密度曲线。
///
/// amount[j,m] = Σ_k silver[j,k]·M[k,m]；le0[j,m] = log_exposure[j] − amount[j,m]；
/// corrected[:,c] = interp(log_exposure, le0[:,c], curves[:,c])。
pub fn density_curves_before_couplers(
    curves: &[[f64; 3]],
    log_exposure: &[f64],
    matrix: &[[f64; 3]; 3],
    positive: bool,
) -> Vec<[f64; 3]> {
    let n = curves.len();
    // le0[j][c]
    let mut le0 = vec![[0.0f64; 3]; n];
    for (j, row) in curves.iter().enumerate() {
        let silver = if positive { row.map(|v| v) } else { *row };
        for c in 0..3 {
            let mut amount = 0.0f64;
            for k in 0..3 {
                amount += silver[k] * matrix[k][c];
            }
            le0[j][c] = log_exposure[j] - amount;
        }
    }
    (0..n)
        .map(|j| {
            let mut out = [0.0f64; 3];
            for c in 0..3 {
                // 以 le0[:,c] 为 x 轴、curves[:,c] 为 y 值，在 log_exposure[j] 处插值
                let xs: Vec<f64> = le0.iter().map(|r| r[c]).collect();
                let ys: Vec<f64> = curves.iter().map(|r| r[c]).collect();
                out[c] = interp_clamped(log_exposure[j], &xs, &ys);
            }
            out
        })
        .collect()
}

/// compute_exposure_correction_dir_couplers 的逐像素路径（扩散=0，shift=0）。
pub fn exposure_correction(
    log_raw: [f64; 3],
    density_cmy: [f64; 3],
    matrix: &[[f64; 3]; 3],
    positive: bool,
    density_max: [f64; 3],
) -> [f64; 3] {
    let silver = if positive {
        [density_max[0] - density_cmy[0], density_max[1] - density_cmy[1], density_max[2] - density_cmy[2]]
    } else {
        density_cmy
    };
    let mut correction = [0.0f64; 3];
    for m in 0..3 {
        for k in 0..3 {
            correction[m] += silver[k] * matrix[k][m];
        }
    }
    [log_raw[0] - correction[0], log_raw[1] - correction[1], log_raw[2] - correction[2]]
}

/// apply_density_correction_dir_couplers 逐像素路径（扩散关、gamma=1）。
/// `normalized_curves`：nanmin 归一化后的密度曲线（上游 develop() 传入的正是它！）。
pub fn apply_density_correction(
    log_raw: [f64; 3],
    density_cmy: [f64; 3],
    normalized_curves: &[[f64; 3]],
    log_exposure: &[f64],
    couplers: &DirCouplers,
    positive: bool,
    curves0: &[[f64; 3]],
) -> [f64; 3] {
    let matrix = couplers.matrix();
    let mut density_max = [0.0f64; 3];
    for c in 0..3 {
        let mut mx = f64::NEG_INFINITY;
        for row in normalized_curves {
            if row[c].is_finite() && row[c] > mx {
                mx = row[c];
            }
        }
        density_max[c] = mx;
    }
    let log_raw0 = exposure_correction(log_raw, density_cmy, &matrix, positive, density_max);
    let mut out = [0.0f64; 3];
    for c in 0..3 {
        let ys: Vec<f64> = curves0.iter().map(|r| r[c]).collect();
        out[c] = interp_clamped(log_raw0[c], log_exposure, &ys);
    }
    out
}

/// 批量：预计算 curves0 后逐像素校正（管线用）。
pub fn apply_density_correction_image(
    log_raw: &[[f64; 3]],
    density_cmy: &[[f64; 3]],
    normalized_curves: &[[f64; 3]],
    log_exposure: &[f64],
    couplers: &DirCouplers,
    positive: bool,
) -> Vec<[f64; 3]> {
    let matrix = couplers.matrix();
    let curves0 = density_curves_before_couplers(normalized_curves, log_exposure, &matrix, positive);
    let mut density_max = [0.0f64; 3];
    for c in 0..3 {
        let mut mx = f64::NEG_INFINITY;
        for row in normalized_curves {
            if row[c].is_finite() && row[c] > mx {
                mx = row[c];
            }
        }
        density_max[c] = mx;
    }
    density_cmy
        .iter()
        .zip(log_raw.iter())
        .map(|(&cmy, &lr)| {
            let log_raw0 = exposure_correction(lr, cmy, &matrix, positive, density_max);
            let mut out = [0.0f64; 3];
            for c in 0..3 {
                let ys: Vec<f64> = curves0.iter().map(|r| r[c]).collect();
                out[c] = interp_clamped(log_raw0[c], log_exposure, &ys);
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_structure() {
        let c = DirCouplers::negative();
        let m = c.matrix();
        // 对角 = 同层 gamma × inhibition × amount
        assert!((m[0][0] - 0.336).abs() < 1e-12);
        assert!((m[1][1] - 0.319).abs() < 1e-12);
        // 跨层：donor R → receiver G = 0.353
        assert!((m[0][1] - 0.353).abs() < 1e-12);
        assert!((m[0][2] - 0.302).abs() < 1e-12);
        assert!((m[1][0] - 0.154).abs() < 1e-12);
        assert!((m[2][1] - 0.226).abs() < 1e-12);
        // amount 缩放
        let mut c2 = c.clone();
        c2.amount = 2.0;
        assert!((c2.matrix()[0][0] - 0.672).abs() < 1e-12);
    }
}
