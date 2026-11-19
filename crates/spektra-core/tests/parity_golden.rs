//! M1 全量黄金对拍：Rust 管线 vs 上游 Python oracle。
//!
//! 配置：负片直扫、确定性逐像素（lut_mode）、DIR/颗粒/自动曝光/gamut 压缩关、
//! 输入线性 sRGB、输出 sRGB 编码。容差 max_abs ≤ 1e-4（上游 numba fastmath 允许 ~1e-7 级差异）。

use spektra_core::film;
use spektra_core::pipeline::{expose, NegativeScanPipeline};
use spektra_core::profile::{load_profile, load_raw_lut, load_tables};
use spektra_core::oracle_dir;

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_vectors.json");

struct Golden {
    ramp: GoldenSet,
    patches: GoldenSet,
    patch_names: Vec<String>,
}

struct GoldenSet {
    input: Vec<[f64; 3]>,
    log_e_film: Vec<[f64; 3]>,
    cmy_film: Vec<[f64; 3]>,
    rgb_out: Vec<[f64; 3]>,
}

fn parse_px(v: &serde_json::Value) -> Vec<[f64; 3]> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|px| {
            let a = px.as_array().unwrap();
            [
                a[0].as_f64().unwrap_or(f64::NAN),
                a[1].as_f64().unwrap_or(f64::NAN),
                a[2].as_f64().unwrap_or(f64::NAN),
            ]
        })
        .collect()
}

fn load_golden() -> Golden {
    let raw = std::fs::read_to_string(GOLDEN).expect("golden_vectors.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("golden json parse");
    parse_golden(&v)
}

/// 解析任意 stock 的黄金向量 JSON（meta.film 记录生成时的胶片）。
fn parse_golden(v: &serde_json::Value) -> Golden {
    let ramp = &v["ramp"];
    let patches = &v["patches"];
    Golden {
        ramp: GoldenSet {
            input: parse_px(&ramp["input_linear_srgb"]),
            log_e_film: parse_px(&ramp["log_e_film"]),
            cmy_film: parse_px(&ramp["cmy_film"]),
            rgb_out: parse_px(&ramp["rgb_out"]),
        },
        patches: GoldenSet {
            input: parse_px(&patches["input_linear_srgb"]),
            log_e_film: parse_px(&patches["log_e_film"]),
            cmy_film: parse_px(&patches["cmy_film"]),
            rgb_out: parse_px(&patches["rgb_out"]),
        },
        patch_names: patches["names"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().to_string())
            .collect(),
    }
}

fn build_pipeline() -> NegativeScanPipeline {
    let prof = load_profile("kodak_portra_400").expect("profile");
    let tables = load_tables(&oracle_dir().display().to_string()).expect("tables");
    NegativeScanPipeline::new(prof, tables)
}

/// 用 oracle 预合成 LUT 再跑一遍（隔离"合成差异"与"管线差异"）。
fn build_pipeline_with_oracle_lut() -> NegativeScanPipeline {
    let prof = load_profile("kodak_portra_400").expect("profile");
    let tables = load_tables(&oracle_dir().display().to_string()).expect("tables");
    let raw_lut = load_raw_lut(&oracle_dir().display().to_string()).expect("raw_lut");
    NegativeScanPipeline::with_raw_lut(prof, tables, raw_lut)
}

struct Stats {
    max_abs: f64,
    rms: f64,
}

fn compare(a: &[[f64; 3]], b: &[[f64; 3]]) -> Stats {
    assert_eq!(a.len(), b.len(), "长度不一致");
    let mut max_abs = 0.0f64;
    let mut sq = 0.0f64;
    let mut n = 0usize;
    for (pa, pb) in a.iter().zip(b.iter()) {
        for k in 0..3 {
            let d = (pa[k] - pb[k]).abs();
            if d.is_finite() {
                max_abs = max_abs.max(d);
                sq += d * d;
                n += 1;
            }
        }
    }
    Stats { max_abs, rms: (sq / n as f64).sqrt() }
}

fn run_stage(golden: &GoldenSet, pipe: &NegativeScanPipeline, label: &str) {
    // 1) expose 分级：线性输入 → log_e_film
    let log_e: Vec<[f64; 3]> = golden
        .input
        .iter()
        .map(|&rgb| expose(rgb, pipe.raw_lut(), &pipe.tables))
        .collect();
    let s1 = compare(&log_e, &golden.log_e_film);
    println!("[{label}] expose        max_abs={:.3e}  rms={:.3e}", s1.max_abs, s1.rms);

    // 2) develop 分级：oracle log_e → cmy（隔离显影）
    let cmy = film::develop_image(&golden.log_e_film, &pipe.profile);
    let s2 = compare(&cmy, &golden.cmy_film);
    println!("[{label}] develop       max_abs={:.3e}  rms={:.3e}", s2.max_abs, s2.rms);

    // 3) scan 分级：oracle cmy → rgb（隔离扫描）
    let rgb = film::scan_film_image(&golden.cmy_film, &pipe.profile, &pipe.tables);
    let s3 = compare(&rgb, &golden.rgb_out);
    println!("[{label}] scan          max_abs={:.3e}  rms={:.3e}", s3.max_abs, s3.rms);

    // 4) 端到端：线性输入 → rgb_out
    let e2e = pipe.process_image(&golden.input);
    let s4 = compare(&e2e, &golden.rgb_out);
    println!("[{label}] end-to-end    max_abs={:.3e}  rms={:.3e}", s4.max_abs, s4.rms);

    assert!(s1.max_abs <= 1e-4, "{label} expose 误差超标: {}", s1.max_abs);
    assert!(s2.max_abs <= 1e-4, "{label} develop 误差超标: {}", s2.max_abs);
    assert!(s3.max_abs <= 1e-4, "{label} scan 误差超标: {}", s3.max_abs);
    assert!(s4.max_abs <= 1e-4, "{label} end-to-end 误差超标: {}", s4.max_abs);
}

#[test]
fn parity_against_upstream_oracle() {
    let golden = load_golden();
    println!("== 负片直扫黄金对拍（自行合成 raw LUT） ==");
    let pipe = build_pipeline();
    run_stage(&golden.ramp, &pipe, "ramp  ");
    run_stage(&golden.patches, &pipe, "patch ");

    println!("== 负片直扫黄金对拍（oracle 预合成 LUT，隔离合成误差） ==");
    let pipe2 = build_pipeline_with_oracle_lut();
    run_stage(&golden.ramp, &pipe2, "ramp/L");
    run_stage(&golden.patches, &pipe2, "patch/L");
}

/// 正片（slide film）直扫对拍：上游 scan 链对 positive 无独立分支（差异只在
/// couplers/grain/黑白校正，v1 全关），语义由 profile 数据承载。
/// Velvia 黄金向量由 tools/oracle/golden_vectors.py 以 ORACLE_FILM_STOCK 导出；
/// 管线走 new()（自行合成 raw LUT），与 filmSim 引擎同路径。
#[test]
fn parity_positive_velvia_scan_film() {
    const GOLDEN_VELVIA: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_vectors_velvia.json");
    let raw = std::fs::read_to_string(GOLDEN_VELVIA).expect("golden_vectors_velvia.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("golden json parse");
    assert_eq!(
        v["meta"]["film"].as_str().unwrap(),
        "fujifilm_velvia_100",
        "黄金向量生成 stock 不匹配"
    );
    let golden = parse_golden(&v);
    println!("== 正片（Velvia 100）直扫黄金对拍（自行合成 raw LUT） ==");
    let prof = load_profile("fujifilm_velvia_100").expect("velvia profile");
    let tables = load_tables(&oracle_dir().display().to_string()).expect("tables");
    let pipe = NegativeScanPipeline::new(prof, tables);
    run_stage(&golden.ramp, &pipe, "ramp  ");
    run_stage(&golden.patches, &pipe, "patch ");
}

#[test]
fn patch_names_match_golden() {
    let golden = load_golden();
    assert_eq!(golden.patch_names.len(), 18);
    assert!(golden.patch_names.contains(&"skin".to_string()));
    assert_eq!(golden.ramp.input.len(), 61);
}

// ===================== M2-B：print 链对拍 =====================

const GOLDEN_PRINT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_print.json");

struct GoldenPrintSet {
    input: Vec<[f64; 3]>,
    cmy_film_from_m1: Option<Vec<[f64; 3]>>, // 用 M1 黄金的 cmy_film（同一输入集）
    log_e_print: Vec<[f64; 3]>,
    cmy_print: Vec<[f64; 3]>,
    rgb_out: Vec<[f64; 3]>,
}

fn load_golden_print() -> (GoldenPrintSet, GoldenPrintSet, Vec<String>) {
    let raw = std::fs::read_to_string(GOLDEN_PRINT).expect("golden_print.json");
    let v: serde_json::Value = serde_json::from_str(&raw).expect("parse");
    let m1 = std::fs::read_to_string(GOLDEN).expect("golden_vectors.json");
    let m1v: serde_json::Value = serde_json::from_str(&m1).expect("parse");
    let get = |node: &serde_json::Value, m1node: &serde_json::Value| GoldenPrintSet {
        input: parse_px(&node["input_linear_srgb"]),
        cmy_film_from_m1: Some(parse_px(&m1node["cmy_film"])),
        log_e_print: parse_px(&node["log_e_print"]),
        cmy_print: parse_px(&node["cmy_print"]),
        rgb_out: parse_px(&node["rgb_out"]),
    };
    let names = v["patches"]["names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    (get(&v["ramp"], &m1v["ramp"]), get(&v["patches"], &m1v["patches"]), names)
}

fn build_print_pipeline() -> spektra_core::pipeline::PrintPipeline {
    let film = load_profile("kodak_portra_400").expect("film");
    let paper = load_profile("kodak_portra_endura").expect("paper");
    let tables = load_tables(&oracle_dir().display().to_string()).expect("tables");
    spektra_core::pipeline::PrintPipeline::new(film, paper, tables)
}

fn run_print_stage(g: &GoldenPrintSet, pipe: &spektra_core::pipeline::PrintPipeline, label: &str) {
    // 1) print expose：oracle cmy_film → log_e_print
    let log_e_print: Vec<[f64; 3]> = g
        .cmy_film_from_m1
        .as_ref()
        .unwrap()
        .iter()
        .map(|&cmy| {
            spektra_core::printing::print_expose(cmy, &pipe.film, &pipe.paper, &pipe.tables, pipe.factor_midgray(), pipe.cc)
        })
        .collect();
    let s1 = compare(&log_e_print, &g.log_e_print);
    println!("[{label}] print_expose   max_abs={:.3e}  rms={:.3e}", s1.max_abs, s1.rms);

    // 2) print develop：oracle log_e_print → cmy_print
    let cmy_print = spektra_core::printing::develop_print_batch(&g.log_e_print, &pipe.paper);
    let s2 = compare(&cmy_print, &g.cmy_print);
    println!("[{label}] print_develop  max_abs={:.3e}  rms={:.3e}", s2.max_abs, s2.rms);

    // 3) scan print：oracle cmy_print → rgb_out
    let rgb: Vec<[f64; 3]> = g
        .cmy_print
        .iter()
        .map(|&cmy| spektra_core::printing::scan_print_rgb(cmy, &pipe.paper, &pipe.tables))
        .collect();
    let s3 = compare(&rgb, &g.rgb_out);
    println!("[{label}] scan_print     max_abs={:.3e}  rms={:.3e}", s3.max_abs, s3.rms);

    // 4) 端到端：线性输入 → 完整暗房链 → rgb_out
    let e2e = pipe.process_image(&g.input);
    let s4 = compare(&e2e, &g.rgb_out);
    println!("[{label}] print e2e      max_abs={:.3e}  rms={:.3e}", s4.max_abs, s4.rms);

    assert!(s1.max_abs <= 1e-4, "{label} print_expose 超标: {}", s1.max_abs);
    assert!(s2.max_abs <= 1e-4, "{label} print_develop 超标: {}", s2.max_abs);
    assert!(s3.max_abs <= 1e-4, "{label} scan_print 超标: {}", s3.max_abs);
    assert!(s4.max_abs <= 1e-4, "{label} print e2e 超标: {}", s4.max_abs);
}

#[test]
fn parity_print_chain() {
    let (ramp, patches, _names) = load_golden_print();
    println!("== print 链黄金对拍（负片→放大机→相纸→扫描） ==");
    let pipe = build_print_pipeline();
    println!("factor_midgray = {:.17e}  cc = {:?}", pipe.factor_midgray(), pipe.cc);
    run_print_stage(&ramp, &pipe, "ramp  ");
    run_print_stage(&patches, &pipe, "patch ");
}

// ===================== M2-C：DIR 成色剂对拍 =====================

const GOLDEN_COULPLERS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_couplers.json");
const GOLDEN_PRINT_COULPLERS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_print_couplers.json");

fn parse_node(v: &serde_json::Value) -> (Vec<[f64; 3]>, Vec<[f64; 3]>, Vec<[f64; 3]>, Vec<[f64; 3]>) {
    (
        parse_px(&v["input_linear_srgb"]),
        parse_px(&v["log_e_film"]),
        parse_px(&v["cmy_film"]),
        parse_px(&v["rgb_out"]),
    )
}

/// scan_film 链 + couplers ON
#[test]
fn parity_couplers_scan_film() {
    let raw = std::fs::read_to_string(GOLDEN_COULPLERS).expect("golden_couplers.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let (ramp_in, ramp_le, ramp_cmy, ramp_rgb) = parse_node(&v["ramp"]);
    let (patch_in, patch_le, patch_cmy, patch_rgb) = parse_node(&v["patches"]);

    let mut pipe = build_pipeline();
    pipe.set_couplers(Some(spektra_core::couplers::DirCouplers::negative()));

    for (label, input, le_g, cmy_g, rgb_g) in
        [("ramp", ramp_in, ramp_le, ramp_cmy, ramp_rgb), ("patch", patch_in, patch_le, patch_cmy, patch_rgb)]
    {
        // 分级：oracle log_e → 显影(含成色剂) → cmy
        let cmy = film::develop_with_couplers_image(&le_g, &pipe.profile, pipe.couplers.as_ref().unwrap(), false);
        let s2 = compare(&cmy, &cmy_g);
        println!("[{label}] couplers develop  max_abs={:.3e}  rms={:.3e}", s2.max_abs, s2.rms);
        // 分级：oracle cmy → scan → rgb
        let rgb = film::scan_film_image(&cmy_g, &pipe.profile, &pipe.tables);
        let s3 = compare(&rgb, &rgb_g);
        println!("[{label}] scan              max_abs={:.3e}  rms={:.3e}", s3.max_abs, s3.rms);
        // 端到端
        let e2e = pipe.process_image(&input);
        let s4 = compare(&e2e, &rgb_g);
        println!("[{label}] couplers e2e      max_abs={:.3e}  rms={:.3e}", s4.max_abs, s4.rms);
        assert!(s2.max_abs <= 1e-4, "{label} couplers develop 超标: {}", s2.max_abs);
        assert!(s4.max_abs <= 1e-4, "{label} couplers e2e 超标: {}", s4.max_abs);
    }
}

/// print 链 + couplers ON
#[test]
fn parity_couplers_print() {
    let raw = std::fs::read_to_string(GOLDEN_PRINT_COULPLERS).expect("golden_print_couplers.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let m1c = std::fs::read_to_string(GOLDEN_COULPLERS).unwrap();
    let m1cv: serde_json::Value = serde_json::from_str(&m1c).unwrap();

    let mut pipe = build_print_pipeline();
    pipe.set_couplers(Some(spektra_core::couplers::DirCouplers::negative()));

    for (label, pn, m1n) in [("ramp", &v["ramp"], &m1cv["ramp"]), ("patch", &v["patches"], &m1cv["patches"])] {
        let input = parse_px(&pn["input_linear_srgb"]);
        let le_print = parse_px(&pn["log_e_print"]);
        let cmy_print_g = parse_px(&pn["cmy_print"]);
        let rgb_g = parse_px(&pn["rgb_out"]);
        let cmy_film = parse_px(&m1n["cmy_film"]); // 同为 couplers-ON 的负片显影结果

        // 分级：oracle cmy_film → print expose → log_e_print
        let le: Vec<[f64; 3]> = cmy_film
            .iter()
            .map(|&cmy| spektra_core::printing::print_expose(cmy, &pipe.film, &pipe.paper, &pipe.tables, pipe.factor_midgray(), pipe.cc))
            .collect();
        let s1 = compare(&le, &le_print);
        println!("[{label}] c-print_expose    max_abs={:.3e}  rms={:.3e}", s1.max_abs, s1.rms);
        // 分级：log_e_print → print develop
        let cmy_print = spektra_core::printing::develop_print_batch(&le_print, &pipe.paper);
        let s2 = compare(&cmy_print, &cmy_print_g);
        println!("[{label}] c-print_develop   max_abs={:.3e}  rms={:.3e}", s2.max_abs, s2.rms);
        // 端到端
        let e2e = pipe.process_image(&input);
        let s4 = compare(&e2e, &rgb_g);
        println!("[{label}] c-print e2e       max_abs={:.3e}  rms={:.3e}", s4.max_abs, s4.rms);
        assert!(s1.max_abs <= 1e-4, "{label} c-print_expose 超标: {}", s1.max_abs);
        assert!(s4.max_abs <= 1e-4, "{label} c-print e2e 超标: {}", s4.max_abs);
    }
}

// ===================== M2-D1：输入侧色域压缩对拍 =====================

const GOLDEN_GAMUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_gamut_in.json");
const GOLDEN_PRINT_GAMUT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_print_gamut_in.json");

/// scan_film 链 + 输入色域压缩 ON
#[test]
fn parity_gamut_in_scan_film() {
    let raw = std::fs::read_to_string(GOLDEN_GAMUT).expect("golden_gamut_in.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let mut pipe = build_pipeline().with_input_gamut((0.0, 1.0, 6.0));
    pipe.couplers = None; // 隔离变量（oracle 亦关）

    for (label, node) in [("ramp", &v["ramp"]), ("patch", &v["patches"])] {
        let input = parse_px(&node["input_linear_srgb"]);
        let le_g = parse_px(&node["log_e_film"]);
        let rgb_g = parse_px(&node["rgb_out"]);

        let log_e: Vec<[f64; 3]> = input
            .iter()
            .map(|&rgb| expose(rgb, pipe.raw_lut(), &pipe.tables))
            .collect();
        let s1 = compare(&log_e, &le_g);
        println!("[{label}] gamut expose   max_abs={:.3e}  rms={:.3e}", s1.max_abs, s1.rms);
        let e2e = pipe.process_image(&input);
        let s2 = compare(&e2e, &rgb_g);
        println!("[{label}] gamut e2e      max_abs={:.3e}  rms={:.3e}", s2.max_abs, s2.rms);
        assert!(s1.max_abs <= 1e-4, "{label} gamut expose 超标: {}", s1.max_abs);
        assert!(s2.max_abs <= 1e-4, "{label} gamut e2e 超标: {}", s2.max_abs);
    }
}

/// print 链 + 输入色域压缩 ON
#[test]
fn parity_gamut_in_print() {
    let raw = std::fs::read_to_string(GOLDEN_PRINT_GAMUT).expect("golden_print_gamut_in.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();

    let pipe = build_print_pipeline().with_input_gamut((0.0, 1.0, 6.0));
    println!("gamut factor_midgray = {:.17e}", pipe.factor_midgray());

    for (label, node) in [("ramp", &v["ramp"]), ("patch", &v["patches"])] {
        let input = parse_px(&node["input_linear_srgb"]);
        let rgb_g = parse_px(&node["rgb_out"]);
        let e2e = pipe.process_image(&input);
        let s = compare(&e2e, &rgb_g);
        println!("[{label}] gamut print e2e  max_abs={:.3e}  rms={:.3e}", s.max_abs, s.rms);
        assert!(s.max_abs <= 1e-4, "{label} gamut print e2e 超标: {}", s.max_abs);
    }
}

// ===================== M2-D2：输出侧 cam16ucs 色域压缩对拍 =====================

const GOLDEN_GAMUT_OUT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_gamut_out.json");
const GOLDEN_PRINT_GAMUT_OUT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_print_gamut_out.json");

/// scan_film 链 + 输出 cam16ucs 压缩（上游默认：knee 0/1/6 + 亮度 roll-off 0.7/1/2.2）
#[test]
fn parity_gamut_out_scan_film() {
    let raw = std::fs::read_to_string(GOLDEN_GAMUT_OUT).expect("golden_gamut_out.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let tables = load_tables(&oracle_dir().display().to_string()).expect("tables");
    let compressor = std::sync::Arc::new(spektra_core::gamut::OutputGamutCompressor::new_srgb(&tables));

    let mut pipe = build_pipeline();
    pipe.output_gamut = Some(compressor);

    for (label, node) in [("ramp", &v["ramp"]), ("patch", &v["patches"])] {
        let input = parse_px(&node["input_linear_srgb"]);
        let rgb_g = parse_px(&node["rgb_out"]);
        let e2e = pipe.process_image(&input);
        let s = compare(&e2e, &rgb_g);
        println!("[{label}] gamut-out scan e2e  max_abs={:.3e}  rms={:.3e}", s.max_abs, s.rms);
        assert!(s.max_abs <= 1e-4, "{label} gamut-out e2e 超标: {}", s.max_abs);
    }
}

/// print 链 + 输出 cam16ucs 压缩
#[test]
fn parity_gamut_out_print() {
    let raw = std::fs::read_to_string(GOLDEN_PRINT_GAMUT_OUT).expect("golden_print_gamut_out.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let tables = load_tables(&oracle_dir().display().to_string()).expect("tables");
    let compressor = std::sync::Arc::new(spektra_core::gamut::OutputGamutCompressor::new_srgb(&tables));

    let mut pipe = build_print_pipeline();
    pipe.output_gamut = Some(compressor);

    for (label, node) in [("ramp", &v["ramp"]), ("patch", &v["patches"])] {
        let input = parse_px(&node["input_linear_srgb"]);
        let rgb_g = parse_px(&node["rgb_out"]);
        let e2e = pipe.process_image(&input);
        let s = compare(&e2e, &rgb_g);
        println!("[{label}] gamut-out print e2e max_abs={:.3e}  rms={:.3e}", s.max_abs, s.rms);
        assert!(s.max_abs <= 1e-4, "{label} gamut-out print e2e 超标: {}", s.max_abs);
    }
}

// ===================== M2-E：颗粒对拍 =====================

const GOLDEN_GRAIN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/spektra/oracle/golden_grain.json");

/// 颗粒开启（子层 3×3 路径，随机场由 oracle 导出；RNG 本身不在对拍范围）
#[test]
fn parity_grain() {
    let raw = std::fs::read_to_string(GOLDEN_GRAIN).expect("golden_grain.json");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut pipe = build_pipeline(); // couplers off / gamut off —— 与 oracle 配置一致
    let _ = &mut pipe;

    for (label, node) in [("ramp", &v["ramp"]), ("patch", &v["patches"])] {
        let le_g = parse_px(&node["log_e_film"]);
        let cmy_g = parse_px(&node["cmy_film"]);
        let pixel_size = node["pixel_size_um"].as_f64().unwrap();
        let fields = spektra_core::grain::GrainFields::from_json(&v["fields"][label])
            .expect("随机场字段缺失");
        let cfg = spektra_core::grain::GrainConfig::default_portra();

        // 分级：oracle log_e → develop（已验证）→ grain → cmy_film
        let cmy_pre = film::develop_image(&le_g, &pipe.profile);
        let out = spektra_core::grain::apply_grain_layers(
            &cmy_pre,
            &pipe.profile,
            pixel_size,
            &cfg,
            &fields,
        );
        let s = compare(&out, &cmy_g);
        println!("[{label}] grain e2e  max_abs={:.3e}  rms={:.3e}", s.max_abs, s.rms);
        assert!(s.max_abs <= 1e-4, "{label} grain 超标: {}", s.max_abs);
    }
}


