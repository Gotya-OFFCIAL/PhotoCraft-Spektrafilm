//! `filter.spektra.filmSim` — SpektraFilm 光谱胶片模拟滤镜（路线 D：源码级集成）。
//!
//! 母本 [spektrafilm](https://github.com/andreavolpato/spektrafilm)（GPL-3.0），经
//! spektrafilm-rust 的 `spektra-core`（GPL-3.0-or-later，已与 Python oracle 位级对拍）
//! 以源码形式 vendored 进本 fork。物理建模：负片曝光→冲洗→（可选）放大机→相纸→扫描，
//! 光线按 81 个波长追踪；不是 LUT。
//!
//! 许可证：本模块随 `spektra-core` 为 GPL-3.0-or-later，vendoring 后本 fork 的构建适用 GPL。
//!
//! 参数：
//! - `film`：胶片 stock 名（assets profiles 目录，如 `kodak_portra_400`）
//! - `paper`：相纸 stock 名（仅 `printChain` 开启时使用）
//! - `ev`：曝光补偿，-4..4，0 = 不补偿（线性输入按 2^ev 缩放）
//! - `printChain`：**默认 true** —— 相纸打印链（负片→放大机→相纸→扫描），即上游
//!   `io.scan_film=false` 的默认语义，产出的才是「胶片观感」；`false` = 负片直扫，
//!   输出负片橙罩原始扫描（上游 demo 里标注 "negative" 的对照图），仅供研究。
//!   （2026-11-15 修正：原默认 false 使滤镜默认输出橙罩，被误认为渲染 bug。）
//!
//! v1 语义与 spektra-core M1/M2 对拍一致：颗粒/DIR 成色剂/色域压缩保持管线默认（关），
//! 保证输出可复现并与黄金向量一致。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use spektra_core::pipeline::{decode_srgb_f64, NegativeScanPipeline, PrintPipeline};
use spektra_core::profile::load_profile;

use photocraft_color::ColorMode;
use photocraft_doc::LayerContent;
use photocraft_geom::Rect;
use photocraft_raster::Surface;

use crate::{EngineError, Result};

pub const FILM_SIM: &str = "filter.spektra.filmSim";

const DEFAULT_FILM: &str = "kodak_portra_400";
const DEFAULT_PAPER: &str = "kodak_portra_endura";

/// 每个 band 最多处理的像素数：限制线性输入/输出的 f64 工作内存（约 48 B/px）。
const BAND_PIXELS: usize = 1 << 20;

#[derive(Clone, Debug)]
struct FilmSim {
    film: String,
    paper: String,
    ev: f64,
    print_chain: bool,
    couplers: bool,
    couplers_amount: f64,
    input_gamut: bool,
    output_gamut: bool,
    print_ev: f64,
    cc_cyan: f64,
    cc_magenta: f64,
    cc_yellow: f64,
}

/// 查询 (相纸, 胶片) 在 TH-KG3 放大机光源下的数据库中性配光 CC 值 `[C, M, Y]`。
pub fn neutral_cc_for(paper: &str, film: &str) -> [f64; 3] {
    spektra_core::printing::resolve_print_filters(paper, "TH-KG3", film)
}

fn parse_film_sim(p: &Value) -> Result<FilmSim> {
    let string_param = |key: &str, default: &str| -> String {
        match p.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            _ => default.to_string(),
        }
    };
    let f64_param = |key: &str, default: f64, min: f64, max: f64| -> f64 {
        p.get(key)
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite())
            .unwrap_or(default)
            .clamp(min, max)
    };
    let ev = f64_param("ev", 0.0, -4.0, 4.0);
    // printChain 默认 true：相纸打印链才是「胶片观感」（上游 io.scan_film 默认 False 同理）。
    // false 是负片直扫——输出的是负片橙罩原始扫描（上游 demo 里标注 "negative" 的那张），
    // 只作研究用途，不是可用的滤镜观感。
    let print_chain = p.get("printChain").and_then(Value::as_bool).unwrap_or(true);
    let couplers = p.get("couplers").and_then(Value::as_bool).unwrap_or(false);
    let couplers_amount = f64_param("couplersAmount", 1.0, 0.0, 2.0);
    let input_gamut = p.get("inputGamut").and_then(Value::as_bool).unwrap_or(false);
    let output_gamut = p.get("outputGamut").and_then(Value::as_bool).unwrap_or(false);
    let print_ev = f64_param("printEv", 0.0, -2.0, 2.0);
    let cc_cyan = f64_param("ccCyan", 0.0, -50.0, 50.0);
    let cc_magenta = f64_param("ccMagenta", 0.0, -50.0, 50.0);
    let cc_yellow = f64_param("ccYellow", 0.0, -50.0, 50.0);
    Ok(FilmSim {
        film: string_param("film", DEFAULT_FILM),
        paper: string_param("paper", DEFAULT_PAPER),
        ev,
        print_chain,
        couplers,
        couplers_amount,
        input_gamut,
        output_gamut,
        print_ev,
        cc_cyan,
        cc_magenta,
        cc_yellow,
    })
}

enum Pipeline {
    Scan(Arc<NegativeScanPipeline>),
    Print(Arc<PrintPipeline>),
}

impl Pipeline {
    fn process(&self, linear: &[[f64; 3]]) -> Vec<[f64; 3]> {
        match self {
            Pipeline::Scan(p) => p.process_image(linear),
            Pipeline::Print(p) => p.process_image(linear),
        }
    }
}

fn cache() -> &'static Mutex<HashMap<String, Arc<Pipeline>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Pipeline>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// oracle 静态表全局单例（30MB 级）：所有 (film, paper, printChain) 组合共享一份，
/// 避免按组合重复加载导致内存随切换线性增长。首次成功加载后固化 assets 根；
/// 加载失败不缓存，下次调用重试。
fn tables() -> Result<Arc<spektra_core::Tables>> {
    static TABLES: OnceLock<Arc<spektra_core::Tables>> = OnceLock::new();
    static INIT: Mutex<()> = Mutex::new(());
    if let Some(t) = TABLES.get() {
        return Ok(Arc::clone(t));
    }
    let _guard = INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = TABLES.get() {
        return Ok(Arc::clone(t));
    }
    let t = Arc::new(
        spektra_core::profile::load_tables(&spektra_core::oracle_dir().display().to_string())
            .map_err(|e| EngineError::Other(format!("SpektraFilm: {e}")))?,
    );
    let _ = TABLES.set(Arc::clone(&t));
    Ok(t)
}

/// CAM16-UCS 输出色域压缩器全局单例（C_max 64×720 二分表构建一次后跨管线共享）。
fn shared_output_gamut() -> Result<Arc<spektra_core::gamut::OutputGamutCompressor>> {
    static OUT_GAMUT: OnceLock<Arc<spektra_core::gamut::OutputGamutCompressor>> = OnceLock::new();
    if let Some(g) = OUT_GAMUT.get() {
        return Ok(Arc::clone(g));
    }
    let t = tables()?;
    let g = Arc::new(spektra_core::gamut::OutputGamutCompressor::new_srgb(&t));
    let _ = OUT_GAMUT.set(Arc::clone(&g));
    Ok(g)
}

/// 加载（或从缓存取）配置对应的管线。
/// 静态表全局共享后，每组合只构建管线自身（raw LUT 合成约百毫秒级），按参数指纹缓存。
fn pipeline_for(cfg: &FilmSim) -> Result<Arc<Pipeline>> {
    let q = |v: f64| (v * 100.0).round() as i64;
    let key = format!(
        "{}\u{1}{}\u{1}{}\u{1}{}:{}\u{1}{}:{}\u{1}{}:{}:{}:{}",
        cfg.film,
        cfg.paper,
        cfg.print_chain,
        cfg.couplers,
        q(cfg.couplers_amount),
        cfg.input_gamut,
        cfg.output_gamut,
        q(cfg.print_ev),
        q(cfg.cc_cyan),
        q(cfg.cc_magenta),
        q(cfg.cc_yellow)
    );
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = guard.get(&key) {
        return Ok(Arc::clone(p));
    }
    let tables = tables()?;
    let out_gamut = if cfg.output_gamut {
        Some(shared_output_gamut()?)
    } else {
        None
    };
    let make_couplers = |film: &spektra_core::FilmProfile| -> Option<spektra_core::couplers::DirCouplers> {
        if !cfg.couplers || cfg.couplers_amount <= 0.0 || film.info.film_type == "positive" {
            // spektra-core precompute() 当前仅实现负片方向（silver = 曲线本身），正片反射分支尚未移植。
            return None;
        }
        let mut c = spektra_core::couplers::DirCouplers::negative();
        c.amount = cfg.couplers_amount;
        Some(c)
    };
    let film = load_profile(&cfg.film)
        .map_err(|e| EngineError::Other(format!("SpektraFilm film `{}`: {e}", cfg.film)))?;
    let couplers = make_couplers(&film);
    // 彩色反转正片（Velvia / Provia / Ektachrome / Kodachrome）显影后已是正像，
    // 送入负片相纸 PrintPipeline 会被二次反相；按 SpektraLab 规则自动跳过相纸改走透射正片直扫。
    let use_print = cfg.print_chain && film.info.film_type != "positive";
    let p = if use_print {
        let paper = load_profile(&cfg.paper)
            .map_err(|e| EngineError::Other(format!("SpektraFilm paper `{}`: {e}", cfg.paper)))?;
        let mut pipe = PrintPipeline::new(film, paper, tables);
        if cfg.input_gamut {
            pipe = pipe.with_input_gamut((0.0, 1.0, 6.0));
        }
        if cfg.cc_cyan != 0.0 || cfg.cc_magenta != 0.0 || cfg.cc_yellow != 0.0 || cfg.print_ev != 0.0 {
            pipe = pipe.with_enlarger([cfg.cc_cyan, cfg.cc_magenta, cfg.cc_yellow], cfg.print_ev);
        }
        if couplers.is_some() {
            pipe.set_couplers(couplers);
        }
        pipe.output_gamut = out_gamut;
        Arc::new(Pipeline::Print(Arc::new(pipe)))
    } else {
        let mut pipe = NegativeScanPipeline::new(film, tables);
        if cfg.input_gamut {
            pipe = pipe.with_input_gamut((0.0, 1.0, 6.0));
        }
        if couplers.is_some() {
            pipe.set_couplers(couplers);
        }
        pipe.output_gamut = out_gamut;
        Arc::new(Pipeline::Scan(Arc::new(pipe)))
    };
    if guard.len() >= 32 {
        guard.clear();
    }
    guard.insert(key, Arc::clone(&p));
    Ok(p)
}

/// 对 `surf` 的 `bounds` 区域应用胶片模拟，选区外像素保持原样。
/// 返回新 surface（与 `plugin_cmds`/`distort_cmds` 的分发契约一致，`None` = 不改动）。
pub(crate) fn apply_to_surface(
    params: &Value,
    surf: &Surface,
    bounds: Rect,
    selection: Option<&Surface>,
    canvas: Rect,
    ctx: &crate::jobs::JobCtx,
) -> Result<Option<Surface>> {
    let cfg = parse_film_sim(params)?;
    let format = surf.format();
    if format.mode != ColorMode::Rgb {
        return Err(EngineError::Other(format!(
            "SpektraFilm film simulation needs an RGB layer (this layer is {:?})",
            format.mode
        )));
    }
    let n = format.channels();
    let area = surf
        .content_bounds()
        .intersect(&bounds)
        .intersect(&canvas)
        .intersect(&selection.map_or(canvas, |s| s.content_bounds()));
    if area.is_empty() {
        return Ok(None);
    }
    let pipeline = pipeline_for(&cfg)?;
    let ev_scale = 2f64.powf(cfg.ev);
    let sel_n = selection.map(Surface::channels);
    let mut out = surf.clone();
    let w = area.width() as usize;
    let band_h = (BAND_PIXELS / w).max(1);
    let mut y0 = area.y0;
    while y0 < area.y1 {
        ctx.check()?;
        let y1 = (y0 + band_h as i32).min(area.y1);
        let band = Rect::new(area.x0, y0, area.x1, y1);
        // 选区按 band 读取（与像素分带同节奏，避免大选区整块驻留内存；按自身通道数交错）。
        let sel_band = selection.map(|s| s.read_region(band));
        let band_data = surf.read_region(band);
        // sRGB 解码 → 线性，按 2^ev 做曝光补偿。
        let mut linear: Vec<[f64; 3]> = Vec::with_capacity((y1 - y0) as usize * w);
        for chunk in band_data.chunks_exact(n) {
            linear.push([
                decode_srgb_f64(chunk[0] as f64) * ev_scale,
                decode_srgb_f64(chunk[1] as f64) * ev_scale,
                decode_srgb_f64(chunk[2] as f64) * ev_scale,
            ]);
        }
        let filtered = pipeline.process(&linear);
        // 管线输出与输入像素一一对应（编码 sRGB）；不等长说明上游错误，拒绝静默写回。
        if filtered.len() != linear.len() {
            return Err(EngineError::Other(format!(
                "SpektraFilm: pipeline returned {} values for {} pixels",
                filtered.len(),
                linear.len()
            )));
        }
        // 选区覆盖率（channel 0）做混合：out = orig·(1-k) + filtered·k；alpha/额外通道原样保留。
        let mut result: Vec<f32> = Vec::with_capacity(band_data.len());
        for (px, chunk) in band_data.chunks_exact(n).enumerate() {
            let k = match (sel_band.as_deref(), sel_n) {
                (Some(s), Some(m)) if m > 0 => s[px * m].clamp(0.0, 1.0) as f64,
                _ => 1.0,
            };
            for c in 0..3 {
                let o = chunk[c] as f64;
                result.push(((o * (1.0 - k) + filtered[px][c] * k) as f32).clamp(0.0, 1.0));
            }
            for c in 3..n {
                result.push(chunk[c]);
            }
        }
        out.write_region(band, &result);
        y0 = y1;
    }
    Ok(Some(out))
}

/// `run_filter` 的接入点：常规滤镜命令路径（含选区、锁定、智能对象、历史记录）。
pub(crate) fn run(s: &mut crate::Session, p: &Value) -> Result<Value> {
    let layer = crate::commands::layer_param(s, p)?;
    let label = "Film Simulation".to_string();
    let params = p.clone();
    // The journal descriptor captures its own copy: the edit closure owns `params`.
    let journal_params = params.clone();
    let layer_id = layer.0;
    crate::jobs::edit_job(
        s,
        &label,
        move |doc, _, ctx| {
            ctx.check()?;
            let selection = doc.selection.clone();
            let doc_bounds = doc.bounds();
            // 无选区时作用于整个画布。
            let bounds = selection
                .as_ref()
                .map(Surface::content_bounds)
                .filter(|b| !b.is_empty())
                .unwrap_or(doc_bounds);
            let locks = doc.effective_locks(layer);
            let l = doc.layer_mut(layer).ok_or(EngineError::NoLayer(layer))?;
            if locks.pixels || locks.all {
                return Err(EngineError::Other(format!(
                    "Could not complete your request because the layer \"{}\" is locked",
                    l.name
                )));
            }
            let surf = match &mut l.content {
                LayerContent::Raster(surf) => surf,
                LayerContent::Smart(_) => {
                    // 非破坏性：记录为智能滤镜，之后从参数重渲染。
                    let sf = photocraft_doc::SmartFilter {
                        command: FILM_SIM.to_string(),
                        params,
                        blend: photocraft_color::BlendMode::Normal,
                        opacity: 1.0,
                        visible: true,
                    };
                    return crate::smart_cmds::add_smart_filter_in(doc, layer, sf, selection.as_ref(), ctx)
                        .map(|()| Value::Null);
                }
                _ => return Err(EngineError::Other("not a pixel layer".into())),
            };
            let before = locks.transparency.then(|| surf.clone());
            *surf = apply_to_surface(&params, surf, bounds, selection.as_ref(), doc_bounds, ctx)?
                .unwrap_or_else(|| surf.clone());
            if let Some(before) = &before {
                crate::filters::keep_alpha(before, surf, &|| ctx.cancelled())?;
            }
            Ok(Value::Null)
        },
        move |_| serde_json::json!({ "layer": layer_id, "filter": FILM_SIM, "params": &journal_params }),
    )
}
