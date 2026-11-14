//! profile JSON 加载与 oracle 静态表加载。
//!
//! 语义约定（见 types.rs）：
//! - JSON 数组中的 `null` 是上游 NaN 序列化产物，一律还原为 f64::NAN
//! - NaN 基底密度 = 不透光（扫描时透过率归零）
//! - oracle 二进制表为小端 f64、numpy C 行主序

use crate::types::{CdfModel, FilmProfile, ProfileData, ProfileInfo, RawLut, SpectraLut, Tables};
use serde_json::Value;
use std::path::Path;

/// 按stock名从资产目录加载（PhotoCraft fork：运行时解析，见 [`crate::assets`]）。
pub fn load_profile(stock: &str) -> Result<FilmProfile, ProfileError> {
    let path = format!("{}/{stock}.json", crate::assets::profiles_dir().display());
    load_profile_json(&path)
}

#[derive(Debug)]
pub enum ProfileError {
    Io(std::io::Error),
    Json(String),
    Shape { field: String, got: usize, expected: usize },
    Missing(String),
}

impl core::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProfileError::Io(e) => write!(f, "IO 错误: {e}"),
            ProfileError::Json(e) => write!(f, "JSON 错误: {e}"),
            ProfileError::Shape { field, got, expected } => {
                write!(f, "形状不符: {field} 得到 {got}，要求 {expected}")
            }
            ProfileError::Missing(field) => write!(f, "缺少字段: {field}"),
        }
    }
}

impl std::error::Error for ProfileError {}

impl From<std::io::Error> for ProfileError {
    fn from(e: std::io::Error) -> Self {
        ProfileError::Io(e)
    }
}

/// JSON 值 → f64：null → NaN。
fn v2f(v: &Value, field: &str) -> Result<f64, ProfileError> {
    match v {
        Value::Null => Ok(f64::NAN),
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| ProfileError::Json(format!("{field}: 非法数值 {n}"))),
        other => Err(ProfileError::Json(format!("{field}: 期望数值，得到 {other}"))),
    }
}

fn v2vec(v: &Value, field: &str) -> Result<Vec<f64>, ProfileError> {
    v.as_array()
        .ok_or_else(|| ProfileError::Json(format!("{field}: 期望数组")))
        .and_then(|a| a.iter().map(|x| v2f(x, field)).collect())
}

fn expect_array<'a>(v: &'a Value, field: &str) -> Result<&'a Vec<Value>, ProfileError> {
    v.as_array()
        .ok_or_else(|| ProfileError::Missing(field.to_string()))
}

fn check_len(actual: usize, expected: usize, field: &str) -> Result<(), ProfileError> {
    if actual == expected {
        Ok(())
    } else {
        Err(ProfileError::Shape { field: field.to_string(), got: actual, expected })
    }
}

fn parse_info(v: &Value) -> Result<ProfileInfo, ProfileError> {
    let get = |k: &str| -> Result<String, ProfileError> {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| ProfileError::Missing(format!("info.{k}")))
    };
    Ok(ProfileInfo {
        stock: get("stock")?,
        name: get("name")?,
        film_type: get("type")?,
        support: get("support")?,
        channel_model: get("channel_model")?,
        reference_illuminant: get("reference_illuminant")?,
        viewing_illuminant: get("viewing_illuminant")?,
    })
}

fn parse_data(v: &Value) -> Result<ProfileData, ProfileError> {
    let vec1 = |k: &str, n: usize| -> Result<Vec<f64>, ProfileError> {
        let a = v2vec(v.get(k).ok_or_else(|| ProfileError::Missing(k.to_string()))?, k)?;
        check_len(a.len(), n, k)?;
        Ok(a)
    };
    let mat = |k: &str, rows: usize, cols: usize| -> Result<Vec<[f64; 3]>, ProfileError> {
        let a = expect_array(v.get(k).ok_or_else(|| ProfileError::Missing(k.to_string()))?, k)?;
        check_len(a.len(), rows, k)?;
        let mut out = Vec::with_capacity(rows);
        for (i, row) in a.iter().enumerate() {
            let r = v2vec(row, &format!("{k}[{i}]"))?;
            check_len(r.len(), cols, &format!("{k}[{i}]"))?;
            out.push([r[0], r[1], r[2]]);
        }
        Ok(out)
    };

    let wavelengths = vec1("wavelengths", 81)?;
    let log_sensitivity = mat("log_sensitivity", 81, 3)?;
    let channel_density = mat("channel_density", 81, 3)?;
    let base_density = vec1("base_density", 81)?;
    let midscale = vec1("midscale_neutral_density", 81)?;
    let log_exposure = vec1("log_exposure", 256)?;
    let density_curves = mat("density_curves", 256, 3)?;

    // window params: 长度不固定（erf4=4，logiflex8=8，相纸可为空）；管线侧按长度分派
    let window_params = v2vec(v.get("hanatos2025_adaptation_window_params").ok_or_else(|| ProfileError::Missing("window_params".to_string()))?, "window_params")?;

    // surface params: 可为空（相纸）；有行时每行 15
    let sp = expect_array(v.get("hanatos2025_adaptation_surface_params").ok_or_else(|| ProfileError::Missing("surface_params".to_string()))?, "surface_params")?;
    let mut surface_params: Vec<[f64; 15]> = Vec::with_capacity(sp.len());
    for (i, row) in sp.iter().enumerate() {
        let r = v2vec(row, &format!("surface_params[{i}]"))?;
        check_len(r.len(), 15, &format!("surface_params[{i}]"))?;
        let mut a = [0.0f64; 15];
        a.copy_from_slice(&r);
        surface_params.push(a);
    }

    // density_curves_layers: 256×3×3
    let layers_raw = expect_array(v.get("density_curves_layers").ok_or_else(|| ProfileError::Missing("density_curves_layers".to_string()))?, "density_curves_layers")?;
    if !layers_raw.is_empty() {
        check_len(layers_raw.len(), 256, "density_curves_layers")?;
    }
    let mut density_curves_layers = Vec::with_capacity(layers_raw.len());
    for (i, row) in layers_raw.iter().enumerate() {
        let chans = expect_array(row, &format!("layers[{i}]"))?;
        check_len(chans.len(), 3, &format!("layers[{i}]"))?;
        let mut px = [[0.0f64; 3]; 3];
        for (c, ch) in chans.iter().enumerate() {
            let r = v2vec(ch, &format!("layers[{i}][{c}]"))?;
            check_len(r.len(), 3, &format!("layers[{i}][{c}]"))?;
            px[c] = [r[0], r[1], r[2]];
        }
        density_curves_layers.push(px);
    }

    // density_curves_model: 3×3（centers/amplitudes/sigmas，null→NaN）
    let model_key = v
        .get("density_curves_model")
        .ok_or_else(|| ProfileError::Missing("density_curves_model".to_string()))?;
    let mat3x3 = |key: &str| -> Result<[[f64; 3]; 3], ProfileError> {
        let rows = expect_array(
            model_key.get(key).ok_or_else(|| ProfileError::Missing(format!("model.{key}")))?,
            &format!("model.{key}"),
        )?;
        check_len(rows.len(), 3, &format!("model.{key}"))?;
        let mut out = [[0.0f64; 3]; 3];
        for (c, row) in rows.iter().enumerate() {
            let r = v2vec(row, &format!("model.{key}[{c}]"))?;
            check_len(r.len(), 3, &format!("model.{key}[{c}]"))?;
            out[c] = [r[0], r[1], r[2]];
        }
        Ok(out)
    };
    let density_curves_model = CdfModel {
        centers: mat3x3("centers")?,
        amplitudes: mat3x3("amplitudes")?,
        sigmas: mat3x3("sigmas")?,
    };

    Ok(ProfileData {
        wavelengths,
        log_sensitivity,
        window_params,
        surface_params: surface_params.to_vec(),
        channel_density,
        base_density,
        midscale_neutral_density: midscale,
        log_exposure,
        density_curves,
        density_curves_layers,
        density_curves_model,
    })
}

/// 从 JSON 文件加载 profile。
pub fn load_profile_json(path: &str) -> Result<FilmProfile, ProfileError> {
    let text = std::fs::read_to_string(Path::new(path))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| ProfileError::Json(e.to_string()))?;
    let info = parse_info(v.get("info").ok_or_else(|| ProfileError::Missing("info".to_string()))?)?;
    let data = parse_data(v.get("data").ok_or_else(|| ProfileError::Missing("data".to_string()))?)?;
    Ok(FilmProfile { info, data })
}

// ---------- oracle 二进制表 ----------

fn read_f64_bin(path: &Path, expected_elems: usize, name: &str) -> Result<Vec<f64>, ProfileError> {
    let bytes = std::fs::read(path)?;
    check_len(bytes.len(), expected_elems * 8, name)?;
    Ok(bytes
        .chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect())
}

fn parse_meta_n(dir: &str, key: &str) -> Result<usize, ProfileError> {
    let text = std::fs::read_to_string(Path::new(dir).join("tables_meta.json"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| ProfileError::Json(e.to_string()))?;
    let shape = v
        .get(key)
        .and_then(|k| k.get("shape"))
        .and_then(|s| s.as_array())
        .ok_or_else(|| ProfileError::Missing(format!("tables_meta.{key}.shape")))?;
    shape[0]
        .as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| ProfileError::Json(format!("tables_meta.{key}.shape[0] 非法")))
}

fn parse_mat3(v: &Value, key: &str) -> Result<[[f64; 3]; 3], ProfileError> {
    let rows = expect_array(v.get(key).ok_or_else(|| ProfileError::Missing(key.to_string()))?, key)?;
    check_len(rows.len(), 3, key)?;
    let mut m = [[0.0f64; 3]; 3];
    for (i, row) in rows.iter().enumerate() {
        let r = v2vec(row, &format!("{key}[{i}]"))?;
        check_len(r.len(), 3, &format!("{key}[{i}]"))?;
        m[i] = [r[0], r[1], r[2]];
    }
    Ok(m)
}

/// 加载 oracle 静态表（cmfs / illuminants / spectra_lut / matrices）。
pub fn load_tables(dir: &str) -> Result<Tables, ProfileError> {
    let dir_path = Path::new(dir);
    let cmfs_flat = read_f64_bin(&dir_path.join("cmfs_f64.bin"), 81 * 3, "cmfs_f64.bin")?;
    let cmfs: Vec<[f64; 3]> = cmfs_flat.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();

    let ill_flat = read_f64_bin(&dir_path.join("illuminants_f64.bin"), 3 * 81, "illuminants_f64.bin")?;
    let ill: Vec<Vec<f64>> = ill_flat.chunks_exact(81).map(|c| c.to_vec()).collect();
    // 导出顺序见 tools/oracle/export_tables.py：["D55", "D50", "TH-KG3"]
    let illuminants = vec![
        ("D55".to_string(), ill[0].clone()),
        ("D50".to_string(), ill[1].clone()),
        ("TH-KG3".to_string(), ill[2].clone()),
    ];

    let n = parse_meta_n(dir, "spectra_lut")?;
    let spectra_flat = read_f64_bin(&dir_path.join("spectra_lut_f64.bin"), n * n * 81, "spectra_lut_f64.bin")?;
    let spectra_lut = SpectraLut { n, data: spectra_flat };

    let text = std::fs::read_to_string(dir_path.join("matrices.json"))?;
    let m: Value = serde_json::from_str(&text).map_err(|e| ProfileError::Json(e.to_string()))?;
    let mat_rgb_to_xyz_d55 = parse_mat3(&m, "rgb_to_xyz_d55_cat16")?;
    let mat_xyz_to_rgb_view = parse_mat3(&m, "xyz_to_rgb_viewing_cat02")?;
    let mat_xyz_to_rgb_view_pure = parse_mat3(&m, "xyz_to_rgb_viewing_cat02_pure")?;
    let srgb_xyz_to_rgb = parse_mat3(&m, "srgb_xyz_to_rgb_pure")?;
    let srgb_rgb_to_xyz = parse_mat3(&m, "srgb_rgb_to_xyz_pure")?;
    let srgb_rgb_roundtrip = parse_mat3(&m, "srgb_rgb_roundtrip")?;
    let xy = |key: &str| -> Result<[f64; 2], ProfileError> {
        let a = v2vec(m.get(key).ok_or_else(|| ProfileError::Missing(key.to_string()))?, key)?;
        check_len(a.len(), 2, key)?;
        Ok([a[0], a[1]])
    };
    let illu_xy_d55 = xy("illu_xy_d55")?;
    let illu_xy_d50 = xy("illu_xy_d50_viewing")?;
    let srgb_white_xy = xy("srgb_white_xy")?;

    Ok(Tables {
        cmfs,
        illuminants,
        spectra_lut,
        mat_rgb_to_xyz_d55,
        mat_xyz_to_rgb_view,
        mat_xyz_to_rgb_view_pure,
        srgb_xyz_to_rgb,
        srgb_rgb_to_xyz,
        srgb_rgb_roundtrip,
        illu_xy_d55,
        illu_xy_d50,
        srgb_white_xy,
    })
}

/// 加载 oracle 预合成的 raw LUT（对拍合成正确性 / 隔离管线误差用）。
pub fn load_raw_lut(dir: &str) -> Result<RawLut, ProfileError> {
    let n = parse_meta_n(dir, "raw_lut")?;
    let dir_path = Path::new(dir);
    let flat = read_f64_bin(&dir_path.join("raw_lut_f64.bin"), n * n * 3, "raw_lut_f64.bin")?;
    let data: Vec<[f64; 3]> = flat.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    Ok(RawLut { n, data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_portra_400() {
        let p = load_profile("kodak_portra_400").expect("portra");
        assert_eq!(p.data.wavelengths.len(), 81);
        assert!((p.data.wavelengths[0] - 380.0).abs() < 1e-9);
        assert!((p.data.log_exposure[0] - (-3.0)).abs() < 1e-9);
        assert!((p.data.log_exposure[255] - 4.0).abs() < 1e-9);
        let nan_count = p.data.base_density.iter().filter(|v| v.is_nan()).count();
        assert!(nan_count > 0, "base_density 应含 NaN（null 还原）");
        let cd = &p.data.channel_density[40];
        assert!((cd[0] - 0.1165).abs() < 1e-3 && (cd[1] - 0.6161).abs() < 1e-3 && (cd[2] - 0.0209).abs() < 1e-3);
        assert_eq!(p.info.reference_illuminant, "D55");
        assert_eq!(p.info.film_type, "negative");
    }

    #[test]
    fn load_endura_paper() {
        let p = load_profile("kodak_portra_endura").expect("endura");
        assert_eq!(p.info.support, "paper");
        assert_eq!(p.data.wavelengths.len(), 81);
    }

    #[test]
    fn load_oracle_tables() {
        let t = load_tables(&crate::oracle_dir().display().to_string()).expect("tables");
        assert_eq!(t.spectra_lut.n, 192);
        assert_eq!(t.spectra_lut.data.len(), 192 * 192 * 81);
        assert!(t.cmfs.iter().all(|r| r.iter().all(|v| v.is_finite())));
        let det = t.mat_rgb_to_xyz_d55[0][0] * (t.mat_rgb_to_xyz_d55[1][1] * t.mat_rgb_to_xyz_d55[2][2] - t.mat_rgb_to_xyz_d55[1][2] * t.mat_rgb_to_xyz_d55[2][1]);
        assert!(det.abs() > 1e-3);
    }

    #[test]
    fn load_oracle_raw_lut() {
        let lut = load_raw_lut(&crate::oracle_dir().display().to_string()).expect("raw_lut");
        assert_eq!(lut.n, 192);
        assert!(lut.at(0, 0).iter().all(|v| v.is_finite()));
    }
}
