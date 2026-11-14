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
//! - `printChain`：是否走完整 print 链（负片→相纸→扫描），否则仅负片直扫
//!
//! v1 语义与 spektra-core M1/M2 对拍一致：颗粒/DIR 成色剂/色域压缩保持管线默认（关），
//! 保证输出可复现并与黄金向量一致。

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;
use spektra_core::pipeline::{decode_srgb_f64, NegativeScanPipeline, PrintPipeline};
use spektra_core::profile::{load_profile, load_tables};

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
}

fn parse_film_sim(p: &Value) -> Result<FilmSim> {
    let string_param = |key: &str, default: &str| -> String {
        match p.get(key) {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            _ => default.to_string(),
        }
    };
    let ev = p
        .get("ev")
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
        .clamp(-4.0, 4.0);
    let print_chain = p.get("printChain").and_then(Value::as_bool).unwrap_or(false);
    Ok(FilmSim {
        film: string_param("film", DEFAULT_FILM),
        paper: string_param("paper", DEFAULT_PAPER),
        ev,
        print_chain,
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

/// 加载（或从缓存取）(film, paper, printChain) 对应的管线。
/// 加载静态表 + 合成 raw LUT 约百毫秒级，按组合缓存避免每次滤镜都重建。
fn pipeline_for(cfg: &FilmSim) -> Result<Arc<Pipeline>> {
    let key = format!("{}\u{1}{}\u{1}{}", cfg.film, cfg.paper, cfg.print_chain);
    let mut guard = cache().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = guard.get(&key) {
        return Ok(Arc::clone(p));
    }
    let tables = load_tables(&spektra_core::oracle_dir().display().to_string())
        .map_err(|e| EngineError::Other(format!("SpektraFilm: {e}")))?;
    let p = if cfg.print_chain {
        let film = load_profile(&cfg.film)
            .map_err(|e| EngineError::Other(format!("SpektraFilm film `{}`: {e}", cfg.film)))?;
        let paper = load_profile(&cfg.paper)
            .map_err(|e| EngineError::Other(format!("SpektraFilm paper `{}`: {e}", cfg.paper)))?;
        Arc::new(Pipeline::Print(Arc::new(PrintPipeline::new(film, paper, tables))))
    } else {
        let film = load_profile(&cfg.film)
            .map_err(|e| EngineError::Other(format!("SpektraFilm film `{}`: {e}", cfg.film)))?;
        Arc::new(Pipeline::Scan(Arc::new(NegativeScanPipeline::new(film, tables))))
    };
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
