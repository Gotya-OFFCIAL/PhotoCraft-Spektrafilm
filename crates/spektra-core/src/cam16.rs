//! CAM16 / CAM16-UCS 色彩外观模型（colour-science 0.4.7 实现的位级移植）。
//!
//! 关键事实：colour 的 CAM16 复用 CIECAM02 的全部辅助函数（对手维度、eccentricity、
//! 各 correlate、非线性压缩），唯一区别是 CAT16 适配矩阵（MATRIX_16）。
//! viewing 条件固定：Average（F=1.0, c=0.69, N_c=1.0）、L_A=64、Y_b=20。
//! 输入/输出 XYZ 均为 Y=1 归一化尺度（colour 内部 ×100）。

use crate::types::Tables;

/// CAT16 适配矩阵（colour adaptation/datasets/cat.py）。
pub const MATRIX_16: [[f64; 3]; 3] = [
    [0.401288, 0.650173, -0.051461],
    [-0.250268, 1.204414, 0.045854],
    [-0.002079, 0.048952, 0.953127],
];

/// Average 环境因子（与 CIECAM02 相同）。
pub const SURROUND_F: f64 = 1.0;
pub const SURROUND_C: f64 = 0.69;
pub const SURROUND_N_C: f64 = 1.0;

/// CAM16-UCS 拟合系数（Luo et al. 2017）。K_L=1.00 已折叠进 C_1/C_2 的使用处。
const C_1: f64 = 0.007;
const C_2: f64 = 0.0228;

/// 上游固定 viewing 条件。
pub const L_A: f64 = 64.0;
pub const Y_B: f64 = 20.0;

/// 3×3 矩阵乘向量。
#[inline]
pub fn mat_vec(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// 3×3 求逆（伴随/行列式法；numpy np.linalg.inv 同阶精度）。
pub fn mat3_inv(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let (a, b, c, d, e, f, g, h, i) =
        (m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2], m[2][0], m[2][1], m[2][2]);
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    let inv_det = 1.0 / det;
    [
        [
            (e * i - f * h) * inv_det,
            (c * h - b * i) * inv_det,
            (b * f - c * e) * inv_det,
        ],
        [
            (f * g - d * i) * inv_det,
            (a * i - c * g) * inv_det,
            (c * d - a * f) * inv_det,
        ],
        [
            (d * h - e * g) * inv_det,
            (b * g - a * h) * inv_det,
            (a * e - b * d) * inv_det,
        ],
    ]
}

/// 前向非线性响应压缩（含 Luo2013 负值处理）。
#[inline]
fn nl_compress_forward(v: f64, f_l: f64) -> f64 {
    let t = (f_l * v.abs() / 100.0).powf(0.42);
    400.0 * v.signum() * t / (27.13 + t) + 0.1
}

/// 逆向非线性响应压缩。
#[inline]
fn nl_compress_inverse(v: f64, f_l: f64) -> f64 {
    let d = v - 0.1;
    v.signum() * 100.0 / f_l * ((27.13 * d.abs()) / (400.0 - d.abs())).powf(1.0 / 0.42)
}

/// CAM16 固定 viewing 条件下的环境参数。
pub struct Cam16Env {
    pub y_w: f64,
    pub d: f64,
    pub n: f64,
    pub f_l: f64,
    pub n_bb: f64,
    pub n_cb: f64,
    pub z: f64,
    pub d_rgb: [f64; 3],
    pub a_w: f64,
    /// Jp(白) — 输出亮度 roll-off 归一化用
    pub j_p_white: f64,
}

impl Cam16Env {
    /// 从 XYZ_w（Y=1 归一化）构建。
    pub fn new(xyz_w_unit_y: [f64; 3]) -> Self {
        let xyz_w = [xyz_w_unit_y[0] * 100.0, xyz_w_unit_y[1] * 100.0, xyz_w_unit_y[2] * 100.0];
        let y_w = xyz_w[1];
        let rgb_w = mat_vec(&MATRIX_16, xyz_w);

        let d = SURROUND_F * (1.0 - (1.0 / 3.6) * ((-L_A - 42.0) / 92.0).exp());
        let d = d.clamp(0.0, 1.0);
        let n = Y_B / y_w;
        let k = 1.0 / (5.0 * L_A + 1.0);
        let k4 = k * k * k * k;
        let f_l = 0.2 * k4 * (5.0 * L_A) + 0.1 * (1.0 - k4).powi(2) * (5.0 * L_A).powf(1.0 / 3.0);
        let n_bb = 0.725 * (1.0 / n).powf(0.2);
        let n_cb = n_bb;
        let z = 1.48 + n.sqrt();

        let d_rgb = [
            d * y_w / rgb_w[0] + 1.0 - d,
            d * y_w / rgb_w[1] + 1.0 - d,
            d * y_w / rgb_w[2] + 1.0 - d,
        ];
        let rgb_wc = [d_rgb[0] * rgb_w[0], d_rgb[1] * rgb_w[1], d_rgb[2] * rgb_w[2]];
        let rgb_aw = [
            nl_compress_forward(rgb_wc[0], f_l),
            nl_compress_forward(rgb_wc[1], f_l),
            nl_compress_forward(rgb_wc[2], f_l),
        ];
        let a_w = (2.0 * rgb_aw[0] + rgb_aw[1] + rgb_aw[2] / 20.0 - 0.305) * n_bb;

        // 白的 Jp（同条件正变换）
        let white = Self::forward_raw(&xyz_w_unit_y, y_w, d, n, f_l, n_bb, n_cb, z, &d_rgb, a_w);
        let j_p_white = white.0;

        Self { y_w, d, n, f_l, n_bb, n_cb, z, d_rgb, a_w, j_p_white }
    }

    /// 正变换内核（返回 Jp, ap, bp）。
    #[allow(clippy::too_many_arguments)]
    fn forward_raw(
        xyz_unit_y: &[f64; 3],
        y_w: f64,
        _d: f64,
        n: f64,
        f_l: f64,
        n_bb: f64,
        n_cb: f64,
        z: f64,
        d_rgb: &[f64; 3],
        a_w: f64,
    ) -> (f64, f64, f64) {
        let xyz = [xyz_unit_y[0] * 100.0, xyz_unit_y[1] * 100.0, xyz_unit_y[2] * 100.0];
        let rgb = mat_vec(&MATRIX_16, xyz);
        let rgb_c = [d_rgb[0] * rgb[0], d_rgb[1] * rgb[1], d_rgb[2] * rgb[2]];
        let rgb_a = [
            nl_compress_forward(rgb_c[0], f_l),
            nl_compress_forward(rgb_c[1], f_l),
            nl_compress_forward(rgb_c[2], f_l),
        ];

        let a = rgb_a[0] - 12.0 * rgb_a[1] / 11.0 + rgb_a[2] / 11.0;
        let b = (rgb_a[0] + rgb_a[1] - 2.0 * rgb_a[2]) / 9.0;
        let h = b.atan2(a).to_degrees();
        let e_t = ((2.0 + h.to_radians()).cos() + 3.8) / 4.0;
        let a_t = (2.0 * rgb_a[0] + rgb_a[1] + rgb_a[2] / 20.0 - 0.305) * n_bb;

        let j = 100.0 * (a_t / a_w).powf(SURROUND_C * z);
        let t = (50000.0 / 13.0) * SURROUND_N_C * n_cb * e_t * (a * a + b * b).sqrt()
            / (rgb_a[0] + rgb_a[1] + 21.0 * rgb_a[2] / 20.0);
        let c = t.powf(0.9) * (j / 100.0).sqrt() * (1.64 - 0.29f64.powf(n)).powf(0.73);
        let m = c * f_l.powf(0.25);

        let j_p = ((1.0 + 100.0 * C_1) * j) / (1.0 + C_1 * j);
        let m_p = (1.0 / C_2) * (C_2 * m).ln_1p();
        let ap = m_p * h.to_radians().cos();
        let bp = m_p * h.to_radians().sin();
        let _ = y_w;
        (j_p, ap, bp)
    }

    /// XYZ（Y=1 归一化）→ CAM16-UCS (Jp, a', b')。
    pub fn xyz_to_ucs(&self, xyz_unit_y: [f64; 3]) -> [f64; 3] {
        let (j_p, ap, bp) = Self::forward_raw(
            &xyz_unit_y, self.y_w, self.d, self.n, self.f_l, self.n_bb, self.n_cb, self.z, &self.d_rgb, self.a_w,
        );
        [j_p, ap, bp]
    }

    /// CAM16-UCS (Jp, a', b') → XYZ（Y=1 归一化）。
    pub fn ucs_to_xyz(&self, jab: [f64; 3]) -> [f64; 3] {
        let (j_p, ap, bp) = (jab[0], jab[1], jab[2]);
        // UCS → JMh
        let j = j_p / (1.0 + 100.0 * C_1 - C_1 * j_p);
        let m_p = (ap * ap + bp * bp).sqrt();
        let h = bp.atan2(ap).to_degrees();
        let m = ((m_p * C_2).exp_m1()) / C_2;

        // JMh 中间量
        let c = m / self.f_l.powf(0.25);
        let t = (c
            / ((j.max(f64::EPSILON) / 100.0).sqrt()
                * (1.64 - 0.29f64.powf(self.n)).powf(0.73)))
        .powf(1.0 / 0.9);
        let e_t = ((2.0 + h.to_radians()).cos() + 3.8) / 4.0;
        let a_t = self.a_w * (j / 100.0).powf(1.0 / (SURROUND_C * self.z));

        // P_1..P_3
        let p_1 = (50000.0 / 13.0) * SURROUND_N_C * self.n_cb * e_t / t;
        let p_2 = a_t / self.n_bb + 0.305;
        let p_3 = 21.0 / 20.0;

        // 逆对手维度（分支选择，未选分支的 inf/NaN 丢弃）
        let hr = h.to_radians();
        let (sin_hr, cos_hr) = hr.sin_cos();
        let c_s = if sin_hr == 0.0 { 0.0 } else { cos_hr / sin_hr };
        let s_c = if cos_hr == 0.0 { 0.0 } else { sin_hr / cos_hr };
        let p_4 = if sin_hr == 0.0 { f64::INFINITY } else { p_1 / sin_hr };
        let p_5 = if cos_hr == 0.0 { f64::INFINITY } else { p_1 / cos_hr };

        let n_p = p_2 * (2.0 + p_3) * (460.0 / 1403.0);
        let (mut a, mut b) = if sin_hr.abs() >= cos_hr.abs() {
            let b = n_p
                / (p_4 + (2.0 + p_3) * (220.0 / 1403.0) * c_s
                    - (27.0 / 1403.0)
                    + p_3 * (6300.0 / 1403.0));
            (b * c_s, b)
        } else {
            let a = n_p
                / (p_5 + (2.0 + p_3) * (220.0 / 1403.0)
                    - ((27.0 / 1403.0) - p_3 * (6300.0 / 1403.0)) * s_c);
            (a, a * s_c)
        };
        if t == 0.0 {
            a = 0.0;
            b = 0.0;
        }

        // 非线性压缩矩阵
        let rgb_a = [
            (460.0 * p_2 + 451.0 * a + 288.0 * b) / 1403.0,
            (460.0 * p_2 - 891.0 * a - 261.0 * b) / 1403.0,
            (460.0 * p_2 - 220.0 * a - 6300.0 * b) / 1403.0,
        ];
        let rgb_p = [
            nl_compress_inverse(rgb_a[0], self.f_l),
            nl_compress_inverse(rgb_a[1], self.f_l),
            nl_compress_inverse(rgb_a[2], self.f_l),
        ];
        let rgb = [rgb_p[0] / self.d_rgb[0], rgb_p[1] / self.d_rgb[1], rgb_p[2] / self.d_rgb[2]];
        let m_inv = mat3_inv(&MATRIX_16);
        let xyz = mat_vec(&m_inv, rgb);
        [xyz[0] / 100.0, xyz[1] / 100.0, xyz[2] / 100.0]
    }
}

/// 从 Tables 构建 sRGB 输出压缩环境（white = sRGB whitepoint at Y=1）。
pub fn srgb_cam16_env(tables: &Tables) -> Cam16Env {
    let xy = tables.srgb_white_xy;
    let safe_y = xy[1].max(1e-12);
    let xyz_w = [xy[0] / safe_y, 1.0, (1.0 - xy[0] - xy[1]) / safe_y];
    Cam16Env::new(xyz_w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::load_tables;
    use crate::oracle_dir;
    use serde_json::Value;

    #[test]
    fn cam16_forward_inverse_matches_colour() {
        let t = load_tables(&oracle_dir().display().to_string()).unwrap();
        let env = srgb_cam16_env(&t);
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/matrices.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        let refs = &v["cam16_reference"];
        for (name, r) in refs.as_object().unwrap() {
            let xyz = [
                r["xyz"][0].as_f64().unwrap(),
                r["xyz"][1].as_f64().unwrap(),
                r["xyz"][2].as_f64().unwrap(),
            ];
            let jab_g = [
                r["jab"][0].as_f64().unwrap(),
                r["jab"][1].as_f64().unwrap(),
                r["jab"][2].as_f64().unwrap(),
            ];
            let rt_g = [
                r["roundtrip_xyz"][0].as_f64().unwrap(),
                r["roundtrip_xyz"][1].as_f64().unwrap(),
                r["roundtrip_xyz"][2].as_f64().unwrap(),
            ];
            let jab = env.xyz_to_ucs(xyz);
            let d1 = (0..3).map(|k| (jab[k] - jab_g[k]).abs()).fold(0.0, f64::max);
            let back = env.ucs_to_xyz(jab_g);
            let d2 = (0..3).map(|k| (back[k] - rt_g[k]).abs()).fold(0.0, f64::max);
            println!("{name}: forward max_diff={d1:.3e}  inverse max_diff={d2:.3e}");
            assert!(d1 <= 1e-9, "{name} 正变换超标: {d1:.3e}");
            assert!(d2 <= 1e-9, "{name} 逆变换超标: {d2:.3e}");
        }
    }
}

#[cfg(test)]
mod debug_cam16 {
    use super::*;
    #[test]
    #[ignore = "手工 bisect 用"]
    fn debug_blue_forward() {
        let t = crate::profile::load_tables(&crate::oracle_dir().display().to_string()).unwrap();
        let env = srgb_cam16_env(&t);
        // 打印环境参数
        println!("D={:.15} n={:.15} F_L={:.15} N_bb={:.15} z={:.15} A_w={:.15} Jp_w={:.15}",
            env.d, env.n, env.f_l, env.n_bb, env.z, env.a_w, env.j_p_white);
        // 逐步正变换 blue = [0.05, 0.1, 0.5]
        let xyz = [0.05 * 100.0, 0.1 * 100.0, 0.5 * 100.0];
        let rgb = mat_vec(&MATRIX_16, xyz);
        println!("RGB={:?} D_RGB={:?}", rgb, env.d_rgb);
        let rgb_c = [env.d_rgb[0] * rgb[0], env.d_rgb[1] * rgb[1], env.d_rgb[2] * rgb[2]];
        let rgb_a = [
            nl_compress_forward(rgb_c[0], env.f_l),
            nl_compress_forward(rgb_c[1], env.f_l),
            nl_compress_forward(rgb_c[2], env.f_l),
        ];
        println!("RGB_a={rgb_a:?}");
        let a = rgb_a[0] - 12.0 * rgb_a[1] / 11.0 + rgb_a[2] / 11.0;
        let b = (rgb_a[0] + rgb_a[1] - 2.0 * rgb_a[2]) / 9.0;
        let h = b.atan2(a).to_degrees();
        println!("a={a:.15} b={b:.15} h={h:.15}");
        let e_t = ((2.0 + h.to_radians()).cos() + 3.8) / 4.0;
        let a_t = (2.0 * rgb_a[0] + rgb_a[1] + rgb_a[2] / 20.0 - 0.305) * env.n_bb;
        let j = 100.0 * (a_t / env.a_w).powf(SURROUND_C * env.z);
        let tt = (50000.0 / 13.0) * SURROUND_N_C * env.n_cb * e_t * (a * a + b * b).sqrt()
            / (rgb_a[0] + rgb_a[1] + 21.0 * rgb_a[2] / 20.0);
        let c = tt.powf(0.9) * (j / 100.0).sqrt() * (1.64 - 0.29f64.powf(env.n)).powf(0.73);
        let m = c * env.f_l.powf(0.25);
        println!("e_t={e_t:.15} A={a_t:.15} J={j:.15} t={tt:.15} C={c:.15} M={m:.15}");
        let jab = env.xyz_to_ucs([0.05, 0.1, 0.5]);
        println!("jab={jab:?}");
    }
}
