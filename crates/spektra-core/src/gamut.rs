//! 输入侧色域压缩（InputGamutCompressSpec，algorithm="xy"）。
//!
//! 移植自上游 utils/gamut_compression.py：以胶片参考光源为消色差轴，
//! 沿射线向 CIE 1931 光谱轨迹做 ACES-RGC 式 Reinhard knee 径向压缩，
//! 并在 raw LUT 合成时一次性烘焙（new_lut[xy] = old_lut[compress(xy)]）。
//! 输出侧（cam16ucs 等）在 M2-D2 实现。

use crate::types::RawLut;

/// 光谱轨迹闭合多边形（spectral_locus_xy）：CMF 380–700nm @5nm，首顶点重复。
/// `cmfs` 为 81×3（X,Y,Z 列），取前 65 行。
pub fn spectral_locus_xy(cmfs: &[[f64; 3]]) -> Vec<[f64; 2]> {
    let mut pts = Vec::with_capacity(66);
    for row in cmfs.iter().take(65) {
        let total = row[0] + row[1] + row[2];
        let safe = total.max(1e-12);
        pts.push([row[0] / safe, row[1] / safe]);
    }
    pts.push(pts[0]);
    pts
}

/// Reinhard knee（ACES RGC v1.3 同款公式）。
pub fn reinhard_knee(d: f64, threshold: f64, limit: f64, power: f64) -> f64 {
    if d <= threshold {
        return d;
    }
    let scale = limit - threshold;
    let x = (d - threshold) / scale;
    let y = x / (1.0 + x.powf(power)).powf(1.0 / power);
    threshold + scale * y
}

/// 射线-多边形首次相交距离（_ray_polygon_distance）。
fn ray_polygon_distance(origin: [f64; 2], direction: [f64; 2], polygon: &[[f64; 2]]) -> f64 {
    let mut t_min = f64::INFINITY;
    let n = polygon.len() - 1;
    for k in 0..n {
        let (ax, ay) = (polygon[k][0], polygon[k][1]);
        let (bx, by) = (polygon[k + 1][0], polygon[k + 1][1]);
        let (ex, ey) = (bx - ax, by - ay);
        let (dx, dy) = (direction[0], direction[1]);
        let denom = dx * ey - dy * ex;
        if denom.abs() <= 1e-12 {
            continue;
        }
        let (ox, oy) = (origin[0] - ax, origin[1] - ay);
        let t = (-ox * ey + oy * ex) / denom;
        let s = (-ox * dy + oy * dx) / denom;
        if t > 1e-9 && (0.0..=1.0).contains(&s) && t < t_min {
            t_min = t;
        }
    }
    t_min
}

/// compress_xy_radial：向光谱轨迹的径向压缩（单点版）。
pub fn compress_xy_radial(
    xy: [f64; 2],
    white_xy: [f64; 2],
    threshold: f64,
    limit: f64,
    power: f64,
    locus: &[[f64; 2]],
) -> [f64; 2] {
    let delta = [xy[0] - white_xy[0], xy[1] - white_xy[1]];
    let dist = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
    if dist < 1e-9 {
        return xy; // 白点处方向未定义，原样返回
    }
    let direction = [delta[0] / dist, delta[1] / dist];
    let boundary = ray_polygon_distance(white_xy, direction, locus);
    let d_norm = dist / boundary.max(1e-12);
    let d_compressed = reinhard_knee(d_norm, threshold, limit, power);
    [
        white_xy[0] + direction[0] * (d_compressed * boundary),
        white_xy[1] + direction[1] * (d_compressed * boundary),
    ]
}

/// 双线性采样 + 最近边缘钳制（scipy map_coordinates order=1 mode=nearest）。
fn bilinear_nearest(lut: &RawLut, coord_i: f64, coord_j: f64) -> [f64; 3] {
    let n = lut.n;
    let upper = (n - 1) as f64;
    let ci = coord_i.clamp(0.0, upper);
    let cj = coord_j.clamp(0.0, upper);
    let i0 = (ci.floor() as usize).min(n - 1);
    let j0 = (cj.floor() as usize).min(n - 1);
    let i1 = (i0 + 1).min(n - 1);
    let j1 = (j0 + 1).min(n - 1);
    let ti = ci - i0 as f64;
    let tj = cj - j0 as f64;
    let v00 = lut.at(i0, j0);
    let v10 = lut.at(i1, j0);
    let v01 = lut.at(i0, j1);
    let v11 = lut.at(i1, j1);
    let mut out = [0.0f64; 3];
    for c in 0..3 {
        let a = v00[c] + ti * (v10[c] - v00[c]);
        let b = v01[c] + ti * (v11[c] - v01[c]);
        out[c] = a + tj * (b - a);
    }
    out
}

/// remap_tc_lut_for_compression：把输入压缩烘焙进 raw LUT。
/// new_lut[tc] = old_lut[tri2quad(compress(quad2tri(tc)))]，双线性采样。
pub fn remap_tc_lut_for_compression(
    tc_lut: &RawLut,
    reference_illuminant_xy: [f64; 2],
    threshold: f64,
    limit: f64,
    power: f64,
    locus: &[[f64; 2]],
) -> RawLut {
    let n = tc_lut.n;
    let mut data = vec![[0.0f64; 3]; n * n];
    for i in 0..n {
        for j in 0..n {
            let tc = [i as f64 / (n - 1) as f64, j as f64 / (n - 1) as f64];
            // Step 1: tc → xy（quad2tri）
            let xy = crate::upsampling::quad2tri(tc);
            // Step 2: 压缩
            let xy_c = compress_xy_radial(xy, reference_illuminant_xy, threshold, limit, power, locus);
            // Step 3: xy → tc（tri2quad）
            let tc_c = crate::upsampling::tri2quad(xy_c);
            // Step 4: 双线性采样原 LUT（索引空间）
            data[i * n + j] = bilinear_nearest(tc_lut, tc_c[0] * (n - 1) as f64, tc_c[1] * (n - 1) as f64);
        }
    }
    RawLut { n, data }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_aces_reference_values() {
        // ACES RGC v1.3 已知行为：threshold=0, limit=1, power=6 → 恒等附近平滑渐近 1
        assert!(reinhard_knee(0.5, 0.0, 1.0, 6.0) < 0.5);
        assert!((reinhard_knee(0.0, 0.0, 1.0, 6.0) - 0.0).abs() < 1e-12);
        // d→∞ 渐近 limit
        let big = reinhard_knee(100.0, 0.0, 1.0, 6.0);
        assert!(big > 0.99 && big < 1.0, "big={big}");
        // threshold 下恒等
        assert!((reinhard_knee(0.3, 0.5, 1.0, 6.0) - 0.3).abs() < 1e-12);
    }

    #[test]
    fn locus_polygon_closure() {
        let t = crate::profile::load_tables(&crate::oracle_dir().display().to_string()).unwrap();
        let locus = spectral_locus_xy(&t.cmfs);
        assert_eq!(locus.len(), 66);
        // 首尾闭合
        assert!((locus[0][0] - locus[65][0]).abs() < 1e-12);
        // 光谱轨迹的 x 范围约 0.13~0.17（紫端）到 0.73（红端）
        let x_max = locus.iter().take(65).map(|p| p[0]).fold(0.0, f64::max);
        assert!((0.7..0.75).contains(&x_max), "x_max={x_max}");
    }

    #[test]
    fn radial_compression_pulls_toward_white() {
        let t = crate::profile::load_tables(&crate::oracle_dir().display().to_string()).unwrap();
        let locus = spectral_locus_xy(&t.cmfs);
        let white = t.illu_xy_d55;
        // 一个饱和蓝点（光域内靠近轨迹）
        let blue = [0.16, 0.05];
        let out = compress_xy_radial(blue, white, 0.0, 1.0, 6.0, &locus);
        // 压缩后应更靠近白点
        let d0 = ((blue[0] - white[0]).powi(2) + (blue[1] - white[1]).powi(2)).sqrt();
        let d1 = ((out[0] - white[0]).powi(2) + (out[1] - white[1]).powi(2)).sqrt();
        assert!(d1 < d0, "d0={d0} d1={d1}");
        // 白点原样返回
        assert_eq!(compress_xy_radial(white, white, 0.0, 1.0, 6.0, &locus), white);
    }
}

// ---------------------------------------------------------------------------
// M2-D2：输出侧色域压缩（cam16ucs，上游默认算法）
// ---------------------------------------------------------------------------

use crate::cam16::Cam16Env;

/// 输出色域压缩器（每管线构建一次；C_max 表 + CAM16 环境 + 矩阵组）。
pub struct OutputGamutCompressor {
    pub env: Cam16Env,
    /// C_max 表网格：L 64 点（1..110）、h 720 点（−π..π）
    pub cmax_l: Vec<f64>,
    pub cmax_h: Vec<f64>,
    pub cmax_table: Vec<f64>, // 64×720 行主序
    pub m_in: [[f64; 3]; 3],
    pub m_out: [[f64; 3]; 3],
    pub m_rt: [[f64; 3]; 3],
    pub knee: (f64, f64, f64),
    pub lightness: (f64, f64, f64),
}

const CMAX_N_L: usize = 64;
const CMAX_N_H: usize = 720;
const CMAX_N_BISECT: usize = 18;

impl OutputGamutCompressor {
    /// 构建（默认 knee=(0,1,6)，lightness=(0.7,1.0,2.2)，输出空间 sRGB）。
    pub fn new_srgb(tables: &Tables_) -> Self {
        let env = crate::cam16::srgb_cam16_env(tables);
        let m_in = tables.srgb_rgb_to_xyz;
        let m_out = tables.srgb_xyz_to_rgb;
        let m_rt = tables.srgb_rgb_roundtrip;

        // C_max 二分表
        let l_grid: Vec<f64> = (0..CMAX_N_L)
            .map(|i| 1.0 + (110.0 - 1.0) * i as f64 / (CMAX_N_L - 1) as f64)
            .collect();
        let h_grid: Vec<f64> = (0..CMAX_N_H)
            .map(|i| -core::f64::consts::PI + core::f64::consts::PI * 2.0 * i as f64 / CMAX_N_H as f64)
            .collect();
        let mut table = vec![0.0f64; CMAX_N_L * CMAX_N_H];
        let mut lo = vec![0.0f64; CMAX_N_L * CMAX_N_H];
        let mut hi = vec![150.0f64; CMAX_N_L * CMAX_N_H];
        for _ in 0..CMAX_N_BISECT {
            for li in 0..CMAX_N_L {
                for hi_i in 0..CMAX_N_H {
                    let idx = li * CMAX_N_H + hi_i;
                    let mid = (lo[idx] + hi[idx]) * 0.5;
                    let a = mid * h_grid[hi_i].cos();
                    let b = mid * h_grid[hi_i].sin();
                    let xyz = env.ucs_to_xyz([l_grid[li], a, b]);
                    let rgb = mat_vec(&m_out, xyz);
                    let in_gamut = rgb.iter().all(|&v| (-1e-6..=1.0 + 1e-6).contains(&v));
                    if in_gamut {
                        lo[idx] = mid;
                    } else {
                        hi[idx] = mid;
                    }
                }
            }
        }
        table.copy_from_slice(&lo);
        let _ = table;

        Self {
            env,
            cmax_l: l_grid,
            cmax_h: h_grid,
            cmax_table: lo,
            m_in,
            m_out,
            m_rt,
            knee: (0.0, 1.0, 6.0),
            lightness: (0.7, 1.0, 2.2),
        }
    }

    /// _c_max_lookup：C_max(L, h) 双线性查找（h 取模环绕）。
    fn c_max_lookup(&self, l: f64, h: f64) -> f64 {
        let l = l.clamp(self.cmax_l[0], self.cmax_l[self.cmax_l.len() - 1]);
        let n_h = self.cmax_h.len();
        let h_step = self.cmax_h[1] - self.cmax_h[0];
        let h_idx = (h - self.cmax_h[0]) / h_step;
        let h_lo = (h_idx.floor() as i64).rem_euclid(n_h as i64) as usize;
        let h_hi = (h_lo + 1) % n_h;
        let h_frac = h_idx - h_idx.floor();

        let l_idx = (l - self.cmax_l[0]) / (self.cmax_l[self.cmax_l.len() - 1] - self.cmax_l[0])
            * (self.cmax_l.len() - 1) as f64;
        let l_lo = ((l_idx.floor() as i64).clamp(0, self.cmax_l.len() as i64 - 2)) as usize;
        let l_hi = l_lo + 1;
        let l_frac = l_idx - l_lo as f64;

        let at = |li: usize, hi: usize| self.cmax_table[li * n_h + hi];
        let v00 = at(l_lo, h_lo);
        let v01 = at(l_lo, h_hi);
        let v10 = at(l_hi, h_lo);
        let v11 = at(l_hi, h_hi);
        v00 * (1.0 - l_frac) * (1.0 - h_frac)
            + v01 * (1.0 - l_frac) * h_frac
            + v10 * l_frac * (1.0 - h_frac)
            + v11 * l_frac * h_frac
    }

    /// compress_rgb_cam16ucs_chroma（单像素，linear sRGB 输入/输出）。
    pub fn compress_rgb(&self, rgb: [f64; 3]) -> [f64; 3] {
        let xyz = mat_vec(&self.m_in, rgb);
        let jab = self.env.xyz_to_ucs(xyz);
        // 亮度 roll-off（归一化到白的 Jp；黑锚定）
        let (lt, ll, lp) = self.lightness;
        let j_p_w = self.env.j_p_white;
        let j_p = reinhard_knee(jab[0] / j_p_w, lt, ll, lp) * j_p_w;
        let c_p = (jab[1] * jab[1] + jab[2] * jab[2]).sqrt();
        let h_p = jab[2].atan2(jab[1]);
        let c_p_max = self.c_max_lookup(j_p, h_p).max(1e-9);
        let (kt, kl, kp) = self.knee;
        let c_new = reinhard_knee(c_p / c_p_max, kt, kl, kp) * c_p_max;
        let jab_new = [j_p, c_new * h_p.cos(), c_new * h_p.sin()];
        let xyz_new = self.env.ucs_to_xyz(jab_new);
        mat_vec(&self.m_out, xyz_new)
    }

}

// Tables 别名（避免循环引用书写）
use crate::cam16::mat_vec;
use crate::types::Tables as Tables_;

impl OutputGamutCompressor {
    /// 纯矩阵线性 RGB → 压缩 → 往返（供扫描函数调用）。
    pub fn apply_to_linear_pure(&self, m_view_pure: &[[f64; 3]; 3], xyz2: [f64; 3]) -> [f64; 3] {
        let rgb_lin = mat_vec(m_view_pure, xyz2);
        self.apply_to_linear(rgb_lin)
    }

    /// compress_rgb_cam16ucs_chroma（单像素，linear sRGB 输入/输出）。
    pub fn apply_to_linear(&self, rgb_lin: [f64; 3]) -> [f64; 3] {
        let xyz = mat_vec(&self.m_in, rgb_lin);
        let jab = self.env.xyz_to_ucs(xyz);
        // 亮度 roll-off（归一化到白的 Jp；黑锚定，负值走 knee 恒等段）
        let (lt, ll, lp) = self.lightness;
        let j_p_w = self.env.j_p_white;
        let j_p = reinhard_knee(jab[0] / j_p_w, lt, ll, lp) * j_p_w;
        let c_p = (jab[1] * jab[1] + jab[2] * jab[2]).sqrt();
        let h_p = jab[2].atan2(jab[1]);
        let c_p_max = self.c_max_lookup(j_p, h_p).max(1e-9);
        let (kt, kl, kp) = self.knee;
        let c_new = reinhard_knee(c_p / c_p_max, kt, kl, kp) * c_p_max;
        let jab_new = [j_p, c_new * h_p.cos(), c_new * h_p.sin()];
        let xyz_new = self.env.ucs_to_xyz(jab_new);
        mat_vec(&self.m_out, xyz_new)
    }
}
