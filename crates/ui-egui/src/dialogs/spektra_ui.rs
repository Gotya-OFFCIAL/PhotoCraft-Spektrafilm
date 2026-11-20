//! SpektraFilm Darkroom Studio UI (`filter.spektra.filmSim`), modelled on SpektraLab's
//! 3-column darkroom interface (`LeftRail`: Film & Print stock lists with covers and CINE badges;
//! `Centre`: interactive Fit/Zoom viewfinder with Process/Split/Original comparison and corner
//! RGB histogram; `RightRail`: Latitude plot, Camera Exposure EV, DIR Couplers, Durst Dichroic
//! Enlarger C/M/Y Color Head filtration, and Spectral/CAM16-UCS Gamut Compression).

use craft::settings::Remembered;
use craft::ui::choice::Checkbox;
use craft::ui::contract::{Bind, Props as _, SaveKey, Show as _, ShowContainer as _};
use craft::ui::dialog::{Dialog, DialogId, DialogResult, DialogStack, StackDialog, dialog_id};
use craft::ui::patterns::form_dialogs::{Param, ParamKind};
use craft::ui::runtime::env::Label;
use craft::ui::sections::{DialogRole, FooterButton};
use egui::{Align2, Color32, CornerRadius, Id, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, Vec2, pos2, vec2};
use photocraft_codecs::{ChannelLayout, SampleType};
use photocraft_doc::Document;
use serde_json::{Map, Value, json};

use crate::PhotocraftApp;
use crate::filter_dialog::FilterPreviewKey;
use crate::theme;

pub const COMMAND: &str = "filter.spektra.filmSim";
pub const DEFAULT_SIZE: Vec2 = vec2(1120.0, 700.0);
pub const MIN_SIZE: Vec2 = vec2(860.0, 540.0);
const REMEMBER_SCOPE: &str = "photocraft.filter.spektra.filmSim";
const WINDOW_SAVE_SCOPE: &str = "photocraft.filter.spektra.filmSim.window";

// Private UI state fields stored in dialog `fields` (stripped from engine params by `params_of`
// because they start with `__`).
const VIEW_MODE_FIELD: &str = "__spkViewMode"; // "process" | "split" | "original"
const SPLIT_POS_FIELD: &str = "__spkSplitPos"; // 0.05..=0.95
const FIT_FIELD: &str = "__spkFit"; // bool (default true)
const ZOOM_FIELD: &str = "__pvZoom";
const CENTER_X_FIELD: &str = "__pvCx";
const CENTER_Y_FIELD: &str = "__pvCy";
const FILTER_CAT_FIELD: &str = "__spkCat"; // "all" | "negative" | "positive" | "cine"
const SEARCH_FIELD: &str = "__spkSearch";
const AUTO_PAPER_FIELD: &str = "__spkAutoPaper"; // bool (default true)
const PARAM_TAB_FIELD: &str = "__spkParamTab"; // "all" | "preDev" | "postDev"

// ---------------------------------------------------------------------------
// SpektraLab Visual Tokens (`SpektraLab/desktop/src/styles/theme.css`)
// ---------------------------------------------------------------------------

struct SpkColors;

impl SpkColors {
    const GROUND: Color32 = Color32::from_rgb(72, 72, 70); // Darkroom surround (#484846 / #5f5f5f)
    const CARD: Color32 = Color32::from_rgb(45, 45, 44); // #2d2d2c
    const WELL: Color32 = Color32::from_rgb(26, 26, 25); // #1a1a19
    const PLOT: Color32 = Color32::from_rgb(30, 31, 30); // #1e1f1e
    const PILL: Color32 = Color32::from_rgb(76, 76, 74); // #4c4c4a
    const RULE: Color32 = Color32::from_rgb(58, 59, 57); // #3a3b39
    const TEXT: Color32 = Color32::from_rgb(249, 247, 243); // #f9f7f3
    const SECONDARY: Color32 = Color32::from_rgb(218, 216, 212); // #dad8d4
    const DIM: Color32 = Color32::from_rgb(142, 141, 138); // #8e8d8a
    const ACCENT: Color32 = Color32::from_rgb(225, 169, 95); // #e1a95f (SpektraLab amber gold)
    const SELECTION: Color32 = Color32::WHITE; // #ffffff
    const ON_SELECTION: Color32 = Color32::from_rgb(15, 14, 14); // #0f0e0e
    const KNOB: Color32 = Color32::from_rgb(251, 248, 243); // #fbf8f3
    const LAT_INSIDE: Color32 = Color32::from_rgb(156, 156, 154); // #9c9c9a
    const LAT_BEYOND: Color32 = Color32::from_rgb(63, 63, 61); // #3f3f3d
    const HIST_R: Color32 = Color32::from_rgb(232, 82, 78); // #e8524e
    const HIST_G: Color32 = Color32::from_rgb(98, 196, 98); // #62c462
    const HIST_B: Color32 = Color32::from_rgb(93, 125, 232); // #5d7de8
    const HIST_Y: Color32 = Color32::from_rgb(189, 189, 189); // #bdbdbd
}

// ---------------------------------------------------------------------------
// Stock Catalogue (`SpektraLab/StockCatalog.json` + 11 embedded covers)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Polarity {
    Positive,
    Negative,
}

pub struct FilmStockInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub short_name: &'static str,
    pub cn_tag: &'static str,
    pub polarity: Polarity,
    pub is_cine: bool,
    pub target_paper: &'static str,
    pub badge_colors: [Color32; 2],
    pub badge_text: &'static str,
    pub cover_jpg: Option<&'static [u8]>,
}

pub struct PaperStockInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub short_name: &'static str,
    pub cn_tag: &'static str,
    pub badge_colors: [Color32; 2],
}

const COVER_PROVIA_100F: &[u8] = include_bytes!("../../../../assets/spektra/covers/fujifilm_provia_100f.jpg");
const COVER_VELVIA_100: &[u8] = include_bytes!("../../../../assets/spektra/covers/fujifilm_velvia_100.jpg");
const COVER_XTRA_400: &[u8] = include_bytes!("../../../../assets/spektra/covers/fujifilm_xtra_400.jpg");
const COVER_EKTACHROME_100: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_ektachrome_100.jpg");
const COVER_GOLD_200: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_gold_200.jpg");
const COVER_PORTRA_160: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_portra_160.jpg");
const COVER_PORTRA_400: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_portra_400.jpg");
const COVER_PORTRA_800: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_portra_800.jpg");
const COVER_PORTRA_800_P1: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_portra_800_push1.jpg");
const COVER_PORTRA_800_P2: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_portra_800_push2.jpg");
const COVER_ULTRAMAX_400: &[u8] = include_bytes!("../../../../assets/spektra/covers/kodak_ultramax_400.jpg");

pub const FILM_STOCKS: &[FilmStockInfo] = &[
    // Positive (4)
    FilmStockInfo {
        id: "fujifilm_provia_100f",
        name: "Fujifilm Provia 100F",
        short_name: "Provia 100F",
        cn_tag: "专业标准反转片 · 色彩中性细腻",
        polarity: Polarity::Positive,
        is_cine: false,
        target_paper: "fujifilm_crystal_archive_typeii",
        badge_colors: [Color32::from_rgb(38, 92, 168), Color32::from_rgb(46, 178, 92)],
        badge_text: "RDP",
        cover_jpg: Some(COVER_PROVIA_100F),
    },
    FilmStockInfo {
        id: "fujifilm_velvia_100",
        name: "Fujifilm Velvia 100",
        short_name: "Velvia 100",
        cn_tag: "高饱和风光反转片 · 浓烈反差",
        polarity: Polarity::Positive,
        is_cine: false,
        target_paper: "fujifilm_crystal_archive_typeii",
        badge_colors: [Color32::from_rgb(34, 48, 78), Color32::from_rgb(56, 188, 84)],
        badge_text: "RVP",
        cover_jpg: Some(COVER_VELVIA_100),
    },
    FilmStockInfo {
        id: "kodak_ektachrome_100",
        name: "Kodak Ektachrome 100",
        short_name: "Ektachrome 100",
        cn_tag: "E100 彩色反转片 · 清冽通透",
        polarity: Polarity::Positive,
        is_cine: false,
        target_paper: "kodak_endura_premier",
        badge_colors: [Color32::from_rgb(28, 72, 148), Color32::from_rgb(232, 182, 44)],
        badge_text: "E100",
        cover_jpg: Some(COVER_EKTACHROME_100),
    },
    FilmStockInfo {
        id: "kodak_kodachrome_64",
        name: "Kodak Kodachrome 64",
        short_name: "Kodachrome 64",
        cn_tag: "K-14 传奇外耦反转片 · 经典纪实",
        polarity: Polarity::Positive,
        is_cine: false,
        target_paper: "kodak_endura_premier",
        badge_colors: [Color32::from_rgb(218, 162, 38), Color32::from_rgb(186, 42, 38)],
        badge_text: "K64",
        cover_jpg: None,
    },
    // Negative — Still (11)
    FilmStockInfo {
        id: "fujifilm_c200",
        name: "Fujifilm C200",
        short_name: "Fujifilm C200",
        cn_tag: "日系日常彩色负片 · 清新翠绿",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "fujifilm_crystal_archive_typeii",
        badge_colors: [Color32::from_rgb(32, 148, 78), Color32::from_rgb(198, 42, 52)],
        badge_text: "C200",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "fujifilm_pro_400h",
        name: "Fujifilm Pro 400H",
        short_name: "Pro 400H",
        cn_tag: "第四感光层专业负片 · 柔和青透肤色",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "fujifilm_crystal_archive_typeii",
        badge_colors: [Color32::from_rgb(42, 162, 148), Color32::from_rgb(32, 128, 76)],
        badge_text: "400H",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "fujifilm_xtra_400",
        name: "Fujifilm X-Tra 400",
        short_name: "X-Tra 400",
        cn_tag: "Superia 400 街拍负片 · 冷暖层次丰富",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "fujifilm_crystal_archive_typeii",
        badge_colors: [Color32::from_rgb(36, 152, 82), Color32::from_rgb(44, 88, 158)],
        badge_text: "X400",
        cover_jpg: Some(COVER_XTRA_400),
    },
    FilmStockInfo {
        id: "kodak_ektar_100",
        name: "Kodak Ektar 100",
        short_name: "Ektar 100",
        cn_tag: "超细颗粒高饱和负片 · 风光纪实",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(192, 44, 42), Color32::from_rgb(228, 168, 42)],
        badge_text: "EK100",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_gold_200",
        name: "Kodak Gold 200",
        short_name: "Gold 200",
        cn_tag: "经典金胶卷 · 温暖怀旧金色调",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(236, 186, 68), Color32::from_rgb(154, 72, 148)],
        badge_text: "G200",
        cover_jpg: Some(COVER_GOLD_200),
    },
    FilmStockInfo {
        id: "kodak_portra_160",
        name: "Kodak Portra 160",
        short_name: "Portra 160",
        cn_tag: "细腻人像专业负片 · 温润肤色",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(228, 174, 82), Color32::from_rgb(142, 88, 128)],
        badge_text: "P160",
        cover_jpg: Some(COVER_PORTRA_160),
    },
    FilmStockInfo {
        id: "kodak_portra_400",
        name: "Kodak Portra 400",
        short_name: "Portra 400",
        cn_tag: "专业人像标杆 · 极佳宽容度与自然肤色",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(232, 182, 76), Color32::from_rgb(168, 76, 122)],
        badge_text: "P400",
        cover_jpg: Some(COVER_PORTRA_400),
    },
    FilmStockInfo {
        id: "kodak_portra_800",
        name: "Kodak Portra 800",
        short_name: "Portra 800",
        cn_tag: "高感人像专业负片 · 弱光柔和色彩",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(224, 168, 64), Color32::from_rgb(152, 68, 118)],
        badge_text: "P800",
        cover_jpg: Some(COVER_PORTRA_800),
    },
    FilmStockInfo {
        id: "kodak_portra_800_push1",
        name: "Kodak Portra 800 (Push 1)",
        short_name: "Portra 800 +1",
        cn_tag: "Portra 800 迫冲一档 · 反差与氛围感强化",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(218, 152, 54), Color32::from_rgb(142, 58, 108)],
        badge_text: "P8+1",
        cover_jpg: Some(COVER_PORTRA_800_P1),
    },
    FilmStockInfo {
        id: "kodak_portra_800_push2",
        name: "Kodak Portra 800 (Push 2)",
        short_name: "Portra 800 +2",
        cn_tag: "Portra 800 迫冲两档 · 强烈胶片反差",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(208, 138, 44), Color32::from_rgb(128, 48, 98)],
        badge_text: "P8+2",
        cover_jpg: Some(COVER_PORTRA_800_P2),
    },
    FilmStockInfo {
        id: "kodak_ultramax_400",
        name: "Kodak Ultramax 400",
        short_name: "Ultramax 400",
        cn_tag: "全能高饱和街拍负片 · 明快色彩",
        polarity: Polarity::Negative,
        is_cine: false,
        target_paper: "kodak_portra_endura",
        badge_colors: [Color32::from_rgb(232, 184, 58), Color32::from_rgb(44, 92, 168)],
        badge_text: "U400",
        cover_jpg: Some(COVER_ULTRAMAX_400),
    },
    // Negative — Cine (7)
    FilmStockInfo {
        id: "kodak_verita_200d",
        name: "Kodak Verita 200D",
        short_name: "Verita 200D",
        cn_tag: "200D 日光型柔和电影负片",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(184, 132, 68), Color32::from_rgb(56, 56, 54)],
        badge_text: "200D",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_vision3_50d",
        name: "Kodak Vision3 50D",
        short_name: "Vision3 50D",
        cn_tag: "5203 日光型超细颗粒电影负片 · 极高宽容度",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(218, 162, 78), Color32::from_rgb(48, 52, 58)],
        badge_text: "50D",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_vision3_200t",
        name: "Kodak Vision3 200T",
        short_name: "Vision3 200T",
        cn_tag: "5213 钨丝灯电影负片 · 细腻冷暖层次",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(78, 138, 198), Color32::from_rgb(48, 52, 58)],
        badge_text: "200T",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_vision3_250d",
        name: "Kodak Vision3 250D",
        short_name: "Vision3 250D",
        cn_tag: "5207 日光型主力院线电影负片",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(225, 169, 95), Color32::from_rgb(48, 52, 58)],
        badge_text: "250D",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_vision3_500t",
        name: "Kodak Vision3 500T",
        short_name: "Vision3 500T",
        cn_tag: "5219 夜景与暗光院线电影负片标杆",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(68, 128, 192), Color32::from_rgb(38, 42, 48)],
        badge_text: "500T",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_2383",
        name: "Kodak Vision 2383",
        short_name: "Vision 2383",
        cn_tag: "2383 经典院线电影印片风格",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(212, 152, 74), Color32::from_rgb(38, 38, 36)],
        badge_text: "2383",
        cover_jpg: None,
    },
    FilmStockInfo {
        id: "kodak_2393",
        name: "Kodak Vision Premier 2393",
        short_name: "Premier 2393",
        cn_tag: "2393 高反差深黑院线印片风格",
        polarity: Polarity::Negative,
        is_cine: true,
        target_paper: "kodak_supra_endura",
        badge_colors: [Color32::from_rgb(228, 164, 82), Color32::from_rgb(32, 32, 30)],
        badge_text: "2393",
        cover_jpg: None,
    },
];

pub const PAPER_STOCKS: &[PaperStockInfo] = &[
    PaperStockInfo {
        id: "kodak_portra_endura",
        name: "Kodak Professional Portra Endura",
        short_name: "Portra Endura",
        cn_tag: "人像相纸 · 柔和过渡与温润肤色还原",
        badge_colors: [Color32::from_rgb(225, 169, 95), Color32::from_rgb(142, 78, 112)],
    },
    PaperStockInfo {
        id: "kodak_supra_endura",
        name: "Kodak Professional Supra Endura",
        short_name: "Supra Endura",
        cn_tag: "商业标准相纸 · 均衡对比度与自然饱和",
        badge_colors: [Color32::from_rgb(218, 158, 72), Color32::from_rgb(72, 108, 156)],
    },
    PaperStockInfo {
        id: "kodak_ultra_endura",
        name: "Kodak Professional Ultra Endura",
        short_name: "Ultra Endura",
        cn_tag: "高反差相纸 · 浓郁色彩与深邃黑位",
        badge_colors: [Color32::from_rgb(212, 132, 58), Color32::from_rgb(168, 52, 52)],
    },
    PaperStockInfo {
        id: "kodak_endura_premier",
        name: "Kodak Professional Endura Premier",
        short_name: "Endura Premier",
        cn_tag: "旗舰银盐相纸 · 扩展色域与通透高光",
        badge_colors: [Color32::from_rgb(232, 182, 84), Color32::from_rgb(88, 64, 128)],
    },
    PaperStockInfo {
        id: "kodak_ektacolor_edge",
        name: "Kodak Ektacolor Edge",
        short_name: "Ektacolor Edge",
        cn_tag: "经典冲印相纸 · 明快反差与干净白场",
        badge_colors: [Color32::from_rgb(228, 172, 64), Color32::from_rgb(48, 118, 158)],
    },
    PaperStockInfo {
        id: "fujifilm_crystal_archive_typeii",
        name: "Fujifilm Crystal Archive Type II",
        short_name: "Crystal Archive II",
        cn_tag: "富士晶彩二代相纸 · 清透绿调与纯净高光",
        badge_colors: [Color32::from_rgb(42, 162, 92), Color32::from_rgb(38, 104, 168)],
    },
];

pub fn find_film(id: &str) -> &'static FilmStockInfo {
    FILM_STOCKS.iter().find(|s| s.id == id).unwrap_or(&FILM_STOCKS[10]) // defaults to kodak_portra_400
}

pub fn find_paper(id: &str) -> &'static PaperStockInfo {
    PAPER_STOCKS.iter().find(|s| s.id == id).unwrap_or(&PAPER_STOCKS[0])
}

/// Whether `fields` belongs to the `filter.spektra.filmSim` dialog.
pub fn owns(fields: &Map<String, Value>) -> bool {
    fields.get("__command").and_then(Value::as_str) == Some(COMMAND)
}

// ---------------------------------------------------------------------------
// Cover Texture Cache & Histogram / Latitude Computation
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct CoverCache {
    textures: std::collections::HashMap<&'static str, TextureHandle>,
}

fn cover_texture(ctx: &egui::Context, stock: &FilmStockInfo) -> Option<TextureHandle> {
    let bytes = stock.cover_jpg?;
    let id = Id::unique("spektra-film-covers");
    let mut cache: CoverCache = ctx.data_mut(|d| std::mem::take(d.get_temp_mut_or_default::<CoverCache>(id)));
    if let Some(tex) = cache.textures.get(stock.id).cloned() {
        ctx.data_mut(|d| d.insert_temp(id, cache));
        return Some(tex);
    }
    let decoded = photocraft_codecs::decode(bytes).ok().map(|img| img.convert(ChannelLayout::Rgba, SampleType::U8));
    let tex = match decoded {
        Some(img) if img.data().len() == img.width() as usize * img.height() as usize * 4 => {
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.data());
            let opts = TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..TextureOptions::LINEAR };
            Some(ctx.load_texture(format!("spk-cover-{}", stock.id), ci, opts))
        }
        _ => None,
    };
    if let Some(t) = &tex {
        cache.textures.insert(stock.id, t.clone());
    }
    ctx.data_mut(|d| d.insert_temp(id, cache));
    tex
}

#[derive(Clone)]
struct ScopeStats {
    hist_r: [f32; 64],
    hist_g: [f32; 64],
    hist_b: [f32; 64],
    hist_y: [f32; 64],
    lat_bins: [f32; 40],
    below_pct: f32,
    within_pct: f32,
    above_pct: f32,
}

impl Default for ScopeStats {
    fn default() -> Self {
        Self {
            hist_r: [0.0; 64],
            hist_g: [0.0; 64],
            hist_b: [0.0; 64],
            hist_y: [0.0; 64],
            lat_bins: [0.0; 40],
            below_pct: 0.0,
            within_pct: 100.0,
            above_pct: 0.0,
        }
    }
}

fn compute_scope_stats(doc: &Document) -> ScopeStats {
    let w = doc.size.width.clamp(1, 96);
    let h = doc.size.height.clamp(1, 96);
    let small = photocraft_compose::render_reduced(doc, w, h);
    let mut r_bins = [0u32; 64];
    let mut g_bins = [0u32; 64];
    let mut b_bins = [0u32; 64];
    let mut y_bins = [0u32; 64];
    let mut lat = [0u32; 40];
    let mut below = 0u32;
    let mut within = 0u32;
    let mut above = 0u32;
    let mut total = 0u32;

    for px in &small.px {
        let r = px[0].clamp(0.0, 1.0);
        let g = px[1].clamp(0.0, 1.0);
        let b = px[2].clamp(0.0, 1.0);
        let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
        let ri = ((r * 63.0).round() as usize).min(63);
        let gi = ((g * 63.0).round() as usize).min(63);
        let bi = ((b * 63.0).round() as usize).min(63);
        let yi = ((luma * 63.0).round() as usize).min(63);
        r_bins[ri] += 1;
        g_bins[gi] += 1;
        b_bins[bi] += 1;
        y_bins[yi] += 1;

        // Map luma to stops around mid-gray (-5..+5 EV mapped into 40 bins)
        let lin = if luma <= 0.04045 { luma / 12.92 } else { ((luma + 0.055) / 1.055).powf(2.4) };
        let ev = ((lin.max(1e-4)) / 0.184).log2().clamp(-5.0, 5.0);
        let li = (((ev + 5.0) / 10.0 * 39.0).round() as usize).min(39);
        lat[li] += 1;
        total += 1;
        if ev < -3.2 {
            below += 1;
        } else if ev > 3.2 {
            above += 1;
        } else {
            within += 1;
        }
    }

    let max_h = r_bins
        .iter()
        .chain(&g_bins)
        .chain(&b_bins)
        .copied()
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let max_lat = lat.iter().copied().max().unwrap_or(1).max(1) as f32;
    let mut out = ScopeStats::default();
    for i in 0..64 {
        out.hist_r[i] = (r_bins[i] as f32 / max_h).powf(0.65);
        out.hist_g[i] = (g_bins[i] as f32 / max_h).powf(0.65);
        out.hist_b[i] = (b_bins[i] as f32 / max_h).powf(0.65);
        out.hist_y[i] = (y_bins[i] as f32 / max_h).powf(0.65);
    }
    for i in 0..40 {
        out.lat_bins[i] = lat[i] as f32 / max_lat;
    }
    let tot = total.max(1) as f32;
    out.below_pct = below as f32 * 100.0 / tot;
    out.within_pct = within as f32 * 100.0 / tot;
    out.above_pct = above as f32 * 100.0 / tot;
    out
}

#[derive(Clone)]
struct CachedTex {
    tex: TextureHandle,
    size: [u32; 2],
    stats: ScopeStats,
}

#[derive(Default, Clone)]
struct FedPreview {
    orig: Option<CachedTex>,
    filt: Option<CachedTex>,
    base_k: u32,
}

#[derive(Clone, PartialEq)]
enum TexKey {
    Filtered(FilterPreviewKey),
    Original(photocraft_doc::DocId, u64, u32),
}

/// Answer produced when the user confirms [`SpektraDialog`].
#[derive(Clone, Debug)]
pub struct SpektraAnswer {
    id: DialogId,
    fields: Map<String, Value>,
    captured: Value,
}

/// Hosted 3-column darkroom dialog for `filter.spektra.filmSim`.
pub struct SpektraDialog {
    id: DialogId,
    fields: Map<String, Value>,
    captured: Value,
    fed: FedPreview,
    view_sig: Option<(photocraft_engine::channel_cmds::ChannelView, bool, Option<photocraft_doc::LayerId>)>,
    orig_key: Option<TexKey>,
    filt_key: Option<TexKey>,
}

impl SpektraDialog {
    fn new(fields: Map<String, Value>, captured: Value) -> Self {
        Self {
            id: dialog_id(COMMAND),
            fields,
            captured,
            fed: FedPreview { base_k: 1, ..Default::default() },
            view_sig: None,
            orig_key: None,
            filt_key: None,
        }
    }

    /// Expose fields for unit tests and inspection.
    pub fn fields(&self) -> &Map<String, Value> {
        &self.fields
    }

    /// Mutable access to fields for unit tests.
    pub fn fields_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.fields
    }
}

impl StackDialog for SpektraDialog {
    type Result = SpektraAnswer;

    fn id(&self) -> DialogId {
        self.id
    }

    fn key(&self) -> Option<&str> {
        Some(COMMAND)
    }

    fn show(&mut self, ui: &mut Ui, _stack: &DialogStack) -> Option<DialogResult<SpektraAnswer>> {
        let SpektraDialog { id, fields, captured, fed, .. } = self;
        let mut preview = fields.get("__preview").and_then(Value::as_bool).unwrap_or(true);
        let shown = Dialog::new(Label::from(tl!("Film Simulation").to_string()))
            .overlay(|overlay| overlay.resizable(true).size(DEFAULT_SIZE).min_size(MIN_SIZE))
            .remember(SaveKey::named(WINDOW_SAVE_SCOPE))
            .close_on_enter(true)
            .buttons([
                FooterButton::new(Label::from(tl!("Cancel").to_string()), DialogRole::Cancel),
                FooterButton::new(Label::from(tl!("OK").to_string()), DialogRole::Accept),
            ])
            .footer(|ui, _| {
                Checkbox::new(Bind::bound(&mut preview)).label(Label::from(tl!("Preview").to_string())).show(ui);
            })
            .show(ui, |ui, _| {
                render_body(fed, ui, fields);
            });
        fields.insert("__preview".into(), json!(preview));
        shown.result(|| SpektraAnswer { id: *id, fields: fields.clone(), captured: captured.clone() })
    }

    fn confirm(&mut self, _ctx: &egui::Context) -> Option<SpektraAnswer> {
        Some(SpektraAnswer { id: self.id, fields: self.fields.clone(), captured: self.captured.clone() })
    }

    fn json_fields(&self) -> Option<Value> {
        Some(crate::filter_dialog::params_of(&self.fields))
    }

    fn set_json_field(&mut self, ctx: &egui::Context, field: &str, value: Value) -> Result<(), String> {
        if field == "preview" || field == "__preview" {
            let on = value.as_bool().ok_or_else(|| format!("`{field}` must be a boolean"))?;
            self.fields.insert("__preview".into(), json!(on));
            ctx.request_repaint();
            return Ok(());
        }
        let Some(spec) = photocraft_engine::commands::find(COMMAND) else {
            return Err(format!("{COMMAND} is unavailable"));
        };
        let params = Param::parse_list(spec.params);
        let Some(param) = params.iter().find(|p| p.key == field) else {
            return Err(format!("the Film Simulation dialog has no field `{field}`"));
        };
        let normalized = validate_param_json(param, &value).ok_or_else(|| format!("`{field}` is not valid: {value}"))?;
        self.fields.insert(field.to_owned(), normalized);
        ctx.request_repaint();
        Ok(())
    }
}

fn defaults_from_params(params: &[Param]) -> Map<String, Value> {
    let mut fields = Map::new();
    for p in params {
        let val = match &p.kind {
            ParamKind::Bool { default, .. } => json!(default),
            ParamKind::Int { default, .. } => json!(default),
            ParamKind::Range { default, .. } | ParamKind::Number { default, .. } | ParamKind::Angle { default, .. } => json!(default),
            ParamKind::Choice { options, default, .. } | ParamKind::Radio { options, default, .. } => {
                let initial = default.as_deref().or_else(|| options.first().map(|(id, _)| id.as_str())).unwrap_or_default();
                json!(initial)
            }
            ParamKind::Text { default, .. } => json!(default),
            _ => continue,
        };
        fields.insert(p.key.clone(), val);
    }
    fields.entry("printChain".to_owned()).or_insert_with(|| json!(true));
    fields
}

fn validate_param_json(param: &Param, value: &Value) -> Option<Value> {
    match &param.kind {
        ParamKind::Bool { .. } => value.as_bool().map(|v| json!(v)),
        ParamKind::Int { min, max, .. } => {
            let n = value.as_i64()?;
            if min.is_some_and(|lo| n < lo) || max.is_some_and(|hi| n > hi) {
                return None;
            }
            Some(json!(n))
        }
        ParamKind::Range { min, max, .. } => {
            let n = value.as_f64().filter(|x| x.is_finite())?;
            if n < *min || n > *max {
                return None;
            }
            Some(json!(n))
        }
        ParamKind::Number { min, max, .. } => {
            let n = value.as_f64().filter(|x| x.is_finite())?;
            if min.is_some_and(|lo| n < lo) || max.is_some_and(|hi| n > hi) {
                return None;
            }
            Some(json!(n))
        }
        ParamKind::Choice { options, .. } | ParamKind::Radio { options, .. } => {
            let s = value.as_str()?;
            options.iter().any(|(id, _)| id == s).then(|| json!(s))
        }
        ParamKind::Text { .. } => value.as_str().map(|s| json!(s)),
        _ => None,
    }
}

/// Opens the SpektraFilm Darkroom Studio dialog (`filter.spektra.filmSim`).
/// Remembers film/look parameters while keeping `printChain` pinned to `true` (`paper print chain`).
pub fn open(app: &mut PhotocraftApp) -> Result<DialogId, String> {
    let spec = photocraft_engine::commands::find(COMMAND).ok_or_else(|| format!("{COMMAND} has no dialog"))?;
    let params = Param::parse_list(spec.params);
    let mut fields = defaults_from_params(&params);
    if let Some(saved) = Remembered::<toml::Table>::new(REMEMBER_SCOPE).recall(app.framework.host()) {
        for (key, value) in saved {
            // `printChain` is a pipeline-mode switch, never restored as false on reopen.
            if key == "printChain" {
                continue;
            }
            let Some(param) = params.iter().find(|p| p.key == key) else { continue };
            if let Ok(json_val) = serde_json::to_value(value)
                && let Some(valid) = validate_param_json(param, &json_val)
            {
                fields.insert(key, valid);
            }
        }
    }
    fields.insert("__command".into(), json!(COMMAND));
    fields.insert("__preview".into(), json!(true));
    app.filter_preview = None;
    Ok(app.framework.dialogs.open(SpektraDialog::new(fields, Value::Null)))
}

/// Opens the SpektraFilm studio to edit an existing Smart Filter (`recorded` overlaid over schema defaults).
pub fn open_smart_filter(app: &mut PhotocraftApp, recorded: &Value, target: Value) -> Result<DialogId, String> {
    let spec = photocraft_engine::commands::find(COMMAND).ok_or_else(|| format!("{COMMAND} has no dialog"))?;
    let params = Param::parse_list(spec.params);
    let mut fields = defaults_from_params(&params);
    if let Some(obj) = recorded.as_object() {
        for (key, value) in obj {
            if let Some(param) = params.iter().find(|p| p.key == *key) {
                if let Some(valid) = validate_param_json(param, value) {
                    fields.insert(key.clone(), valid);
                }
            } else if !key.starts_with("__") {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    fields.insert("__command".into(), json!(COMMAND));
    fields.insert("__preview".into(), json!(true));
    app.filter_preview = None;
    Ok(app.framework.dialogs.open(SpektraDialog::new(fields, target)))
}

/// Live-preview request for `canvas::ensure_filter_preview` while `SpektraDialog` is open with preview on.
pub(crate) fn preview_request(app: &PhotocraftApp) -> Option<(DialogId, String, Value)> {
    let dialog = app.framework.dialogs.get::<SpektraDialog>()?;
    if !dialog.fields.get("__preview").and_then(Value::as_bool).unwrap_or(true) {
        return None;
    }
    let params = crate::filter_dialog::params_of(&dialog.fields);
    if crate::smart_filter_dialog::is_target(&dialog.captured) {
        let (cmd, params) = crate::smart_filter_dialog::params_command(app, &dialog.captured, COMMAND, params).ok()?;
        Some((dialog.id, cmd, params))
    } else {
        Some((dialog.id, COMMAND.to_owned(), app.with_mask_target(COMMAND, params)))
    }
}

/// Whether `SpektraDialog` currently keeps a live filter preview alive.
pub(crate) fn is_previewing(app: &PhotocraftApp) -> bool {
    preview_request(app).is_some()
}

/// Uploads/refreshes proxy & filtered preview textures + scopes for an open `SpektraDialog`.
pub(crate) fn feed(app: &mut PhotocraftApp, ctx: &egui::Context) {
    let Some((id, preview_on, captured)) = app
        .framework
        .dialogs
        .get::<SpektraDialog>()
        .map(|d| (d.id, d.fields.get("__preview").and_then(Value::as_bool).unwrap_or(true), d.captured.clone()))
    else {
        return;
    };
    if preview_on && let Some(idx) = app.session.active_index() {
        crate::canvas::filter_preview::ensure_filter_preview(app, idx, ctx);
    }
    let Some(st) = app.session.active() else { return };
    let (doc_id, revision) = (st.doc.id, st.revision);
    let view_sig = (st.channel_view.clone(), app.session.prefs().interface.show_channels_in_color, st.active_layer);
    let previewed = crate::smart_ui::preview_command(COMMAND, COMMAND, &captured);
    let filtered = app.filter_preview.as_ref().and_then(|p| {
        (preview_on && p.key.dialog == id && p.key.doc == doc_id && p.key.command == previewed)
            .then(|| p.result.clone().map(|r| (p.key.clone(), r)))
            .flatten()
    });
    let base_k = filtered
        .as_ref()
        .map(|(k, _)| k.k)
        .unwrap_or_else(|| {
            let params = preview_request(app).map(|(_, _, p)| p).unwrap_or_else(|| json!({}));
            crate::canvas::filter_preview_factor(app, &st.doc, COMMAND, &params)
        });
    let orig_key = TexKey::Original(doc_id, revision, base_k);
    let filt_key = filtered.as_ref().map(|(k, _)| TexKey::Filtered(k.clone()));

    let (need_orig, need_filt) = {
        let Some(d) = app.framework.dialogs.get::<SpektraDialog>() else { return };
        let view_changed = d.view_sig.as_ref() != Some(&view_sig);
        (view_changed || d.orig_key.as_ref() != Some(&orig_key), view_changed || d.filt_key != filt_key)
    };
    let next_orig = if need_orig {
        let proxy = crate::proxy::proxy_document(&st.doc, base_k);
        let stats = compute_scope_stats(&proxy);
        let tex = crate::filter_preview_ui::upload(ctx, app, "spektra-preview-orig", &proxy);
        Some(CachedTex { tex, size: [proxy.size.width, proxy.size.height], stats })
    } else {
        None
    };
    let next_filt = if need_filt {
        filtered.as_ref().map(|(_, filtered_doc)| {
            let stats = compute_scope_stats(filtered_doc);
            let tex = crate::filter_preview_ui::upload(ctx, app, "spektra-preview-filt", filtered_doc);
            CachedTex { tex, size: [filtered_doc.size.width, filtered_doc.size.height], stats }
        })
    } else {
        None
    };

    if let Some(d) = app.framework.dialogs.get_mut::<SpektraDialog>() {
        if d.view_sig.as_ref() != Some(&view_sig) {
            d.orig_key = None;
            d.filt_key = None;
            d.fed.orig = None;
            d.fed.filt = None;
            d.view_sig = Some(view_sig);
        }
        d.fed.base_k = base_k;
        if let Some(tex) = next_orig {
            d.orig_key = Some(orig_key);
            d.fed.orig = Some(tex);
        }
        if need_filt {
            d.filt_key = filt_key;
            d.fed.filt = next_filt;
        }
    }
}

/// Processes confirmed or cancelled [`SpektraDialog`] results (`TAKERS` entry).
pub(crate) fn take(app: &mut PhotocraftApp, _ctx: &egui::Context) {
    let Some(result) = app.framework.dialogs.take::<SpektraDialog>() else { return };
    match result {
        DialogResult::Ok(ans) => {
            let params = crate::filter_dialog::params_of(&ans.fields);
            if crate::actions::record_again::owns_dialog(app, ans.id) {
                if let Err(e) = crate::actions::record_again::confirm(app, ans.id, COMMAND, params) {
                    crate::notices::error(app, e);
                }
                return;
            }
            let token = crate::actions::token_of(app, ans.id);
            let abandon = |app: &mut PhotocraftApp, token: Option<u64>| {
                if let Some(t) = token {
                    crate::actions::abandoned(app, t);
                }
            };
            if let Err(error) = crate::actions::authorize_panel_preview(app, ans.id, COMMAND, &params) {
                app.filter_preview = None;
                abandon(app, token);
                crate::notices::error(app, error);
                return;
            }
            let (cmd, final_params) = match crate::smart_ui::confirm_command(app, &ans.captured, COMMAND.to_owned(), params) {
                Ok(edit) => edit,
                Err(error) => {
                    app.filter_preview = None;
                    abandon(app, token);
                    crate::notices::error(app, error);
                    return;
                }
            };
            let outcome = crate::actions::run_step_dialog(app, ans.id, &cmd, final_params);
            let running = app
                .session
                .active()
                .is_some_and(|st| app.filter_preview.as_ref().is_some_and(|p| p.key.doc == st.doc.id) && app.session.job_on(st.doc.id).is_some());
            match app.filter_preview.as_mut() {
                Some(p) if outcome.is_ok() && running => p.committing = true,
                _ => app.filter_preview = None,
            }
            if outcome.is_ok() {
                if !crate::smart_filter_dialog::is_target(&ans.captured) {
                    let mut table = toml::Table::new();
                    for (k, v) in &ans.fields {
                        if k.starts_with("__") || k == "printChain" {
                            continue;
                        }
                        if let Ok(tv) = toml::Value::try_from(v.clone()) {
                            table.insert(k.clone(), tv);
                        }
                    }
                    if !table.is_empty() {
                        Remembered::<toml::Table>::new(REMEMBER_SCOPE).store(app.framework.host(), &table);
                    }
                }
                if let Some(t) = token
                    && let Err(e) = crate::actions::resume(app, t)
                {
                    crate::notices::error(app, e);
                }
            } else if let Err(error) = outcome {
                abandon(app, token);
                crate::notices::error(app, error);
            }
        }
        DialogResult::Cancel | _ => {
            if let Some(t) = crate::actions::token_of(app, dialog_id(COMMAND)) {
                crate::actions::abandoned(app, t);
            }
            app.filter_preview = None;
        }
    }
}

fn draw_checker(painter: &egui::Painter, rect: Rect, tile: f32) {
    let dark = Color32::from_gray(42);
    let light = Color32::from_gray(52);
    painter.rect_filled(rect, CornerRadius::ZERO, dark);
    let cols = (rect.width() / tile).ceil() as usize;
    let rows = (rect.height() / tile).ceil() as usize;
    for r in 0..rows {
        for c in 0..cols {
            if (r + c) % 2 == 1 {
                let min = pos2(rect.left() + c as f32 * tile, rect.top() + r as f32 * tile);
                let max = pos2((min.x + tile).min(rect.right()), (min.y + tile).min(rect.bottom()));
                painter.rect_filled(Rect::from_min_max(min, max), CornerRadius::ZERO, light);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main 3-Column Darkroom Studio Body
// ---------------------------------------------------------------------------

fn render_body(fed: &FedPreview, ui: &mut Ui, f: &mut Map<String, Value>) {
    // Paint darkroom card background behind the whole 3-column studio area.
    let avail = ui.available_size();
    let total_h = avail.y.max(460.0);
    let total_w = avail.x.max(780.0);

    let (outer_rect, _) = ui.allocate_exact_size(vec2(total_w, total_h), Sense::hover());
    ui.painter().rect_filled(outer_rect, CornerRadius::same(6), SpkColors::CARD);
    ui.painter().rect_stroke(outer_rect, CornerRadius::same(6), Stroke::new(1.0, SpkColors::WELL), StrokeKind::Inside);

    // Top bar (38px) + 3-column body below
    let topbar_h = 38.0;
    let topbar_rect = Rect::from_min_size(outer_rect.min, vec2(outer_rect.width(), topbar_h));
    let body_rect = Rect::from_min_max(pos2(outer_rect.left(), topbar_rect.bottom() + 1.0), outer_rect.max);

    // Draw top bar
    let mut top_ui = ui.new_child(egui::UiBuilder::new().max_rect(topbar_rect));
    draw_top_bar(&mut top_ui, f);
    ui.painter().line_segment(
        [pos2(outer_rect.left(), topbar_rect.bottom()), pos2(outer_rect.right(), topbar_rect.bottom())],
        Stroke::new(1.0, SpkColors::RULE),
    );

    // 3 Columns: Left Rail (264px) | Center Viewfinder (flex) | Right Rail (292px)
    let left_w = 264.0;
    let right_w = 292.0;
    let left_rect = Rect::from_min_max(body_rect.min, pos2((body_rect.left() + left_w).min(body_rect.right()), body_rect.bottom()));
    let right_rect = Rect::from_min_max(pos2((body_rect.right() - right_w).max(left_rect.right()), body_rect.top()), body_rect.max);
    let center_rect = Rect::from_min_max(pos2(left_rect.right() + 1.0, body_rect.top()), pos2(right_rect.left() - 1.0, body_rect.bottom()));

    // Vertical dividers
    ui.painter().line_segment([pos2(left_rect.right(), body_rect.top()), pos2(left_rect.right(), body_rect.bottom())], Stroke::new(1.0, SpkColors::WELL));
    ui.painter().line_segment([pos2(right_rect.left(), body_rect.top()), pos2(right_rect.left(), body_rect.bottom())], Stroke::new(1.0, SpkColors::WELL));

    let mut left_ui = ui.new_child(egui::UiBuilder::new().max_rect(left_rect));
    draw_left_rail(&mut left_ui, f);

    // Prepare preview textures & scope stats
    let stats = draw_center_canvas(fed, ui, center_rect, f);

    let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right_rect));
    draw_right_rail(&mut right_ui, f, &stats);
}

// ---------------------------------------------------------------------------
// Top Bar (`TopBar.tsx`: Presets · Process/Split/Original Pills · Zoom)
// ---------------------------------------------------------------------------

fn draw_top_bar(ui: &mut Ui, f: &mut Map<String, Value>) {
    let rect = ui.max_rect().shrink2(vec2(12.0, 4.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut child;
    ui.spacing_mut().item_spacing.x = 6.0;

    // Quick Recipe Presets
    ui.label(RichText::new("预设配方").font(theme::semibold(11.0)).color(SpkColors::DIM));
    let presets: &[(&str, &str, &str, bool, bool, f64)] = &[
        ("Portra 人像", "kodak_portra_400", "kodak_portra_endura", true, false, 0.0),
        ("Velvia 风光", "fujifilm_velvia_100", "fujifilm_crystal_archive_typeii", true, false, 0.0),
        ("富士 400H 日系", "fujifilm_pro_400h", "fujifilm_crystal_archive_typeii", true, false, 0.0),
        ("Vision3 电影", "kodak_vision3_500t", "kodak_supra_endura", true, true, 0.0),
        ("Gold 暖阳", "kodak_gold_200", "kodak_portra_endura", true, false, 6.0),
    ];
    let cur_film = f.get("film").and_then(Value::as_str).unwrap_or("kodak_portra_400").to_string();
    let cur_paper = f.get("paper").and_then(Value::as_str).unwrap_or("kodak_portra_endura").to_string();
    let cur_print = f.get("printChain").and_then(Value::as_bool).unwrap_or(true);

    for &(label, film, paper, print_chain, couplers, cc_y) in presets {
        let is_pos = find_film(film).polarity == Polarity::Positive;
        let active = cur_film == film && (is_pos || (cur_paper == paper && cur_print == print_chain));
        if chip_button(ui, label, active).clicked() {
            f.insert("film".into(), json!(film));
            if !is_pos {
                f.insert("paper".into(), json!(paper));
                f.insert("printChain".into(), json!(print_chain));
            }
            f.insert("couplers".into(), json!(couplers));
            f.insert("ccYellow".into(), json!(cc_y));
            f.insert("ccCyan".into(), json!(0.0));
            f.insert("ccMagenta".into(), json!(0.0));
        }
    }

    ui.add_space(10.0);

    // Center View Mode Action Pills (`Process` | `Split` | `Original`)
    let mode = f.get(VIEW_MODE_FIELD).and_then(Value::as_str).unwrap_or("process").to_string();
    for (id, label, tip) in [
        ("process", "显影效果 Process", "查看当前胶片与暗房印相完整效果（按住画布可临时对比原图）"),
        ("split", "分屏对比 Split", "在画布上左右拖动分界线实时对比原图与冲印效果"),
        ("original", "原图 Original", "查看未经胶片模拟的原始图层图像"),
    ] {
        if action_pill(ui, label, mode == id).on_hover_text(tip).clicked() {
            f.insert(VIEW_MODE_FIELD.into(), json!(id));
        }
    }

    // Right-aligned Zoom & Fit Controls
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let fit = f.get(FIT_FIELD).and_then(Value::as_bool).unwrap_or(true);
        let mut zoom = f.get(ZOOM_FIELD).and_then(Value::as_f64).map(|v| v as f32).filter(|z| z.is_finite() && *z > 0.0).unwrap_or(1.0);
        if small_pill_button(ui, "+", false).on_hover_text("放大预览").clicked() {
            zoom = (zoom * 1.4142).clamp(0.125, 8.0);
            f.insert(ZOOM_FIELD.into(), json!(zoom));
            f.insert(FIT_FIELD.into(), json!(false));
        }
        let zoom_text = if fit { "Fit".to_string() } else { format!("{:.0}%", zoom * 100.0) };
        if small_pill_button(ui, &zoom_text, fit).on_hover_text("点击切换适合窗口 (Fit) / 100%").clicked() {
            if fit {
                f.insert(FIT_FIELD.into(), json!(false));
                f.insert(ZOOM_FIELD.into(), json!(1.0));
            } else {
                f.insert(FIT_FIELD.into(), json!(true));
            }
        }
        if small_pill_button(ui, "−", false).on_hover_text("缩小预览").clicked() {
            zoom = (zoom / 1.4142).clamp(0.125, 8.0);
            f.insert(ZOOM_FIELD.into(), json!(zoom));
            f.insert(FIT_FIELD.into(), json!(false));
        }
    });
}

// ---------------------------------------------------------------------------
// Left Rail (`LeftRail.tsx` & `StockList.tsx` — Film & Print Lists)
// ---------------------------------------------------------------------------

fn draw_left_rail(ui: &mut Ui, f: &mut Map<String, Value>) {
    let rect = ui.max_rect();
    // Rail header
    let header_rect = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
    {
        let mut h_ui = ui.new_child(egui::UiBuilder::new().max_rect(header_rect.shrink2(vec2(12.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        h_ui.label(RichText::new("胶片与印相 · Film & Print").font(theme::semibold(12.5)).color(SpkColors::TEXT));
        h_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut auto_paper = f.get(AUTO_PAPER_FIELD).and_then(Value::as_bool).unwrap_or(true);
            if chip_button(ui, if auto_paper { "自动配纸 ✓" } else { "自动配纸" }, auto_paper)
                .on_hover_text("选择胶片时自动切换到该胶片官方推荐的暗房相纸（来自 SpektraLab StockCatalog）")
                .clicked()
            {
                auto_paper = !auto_paper;
                f.insert(AUTO_PAPER_FIELD.into(), json!(auto_paper));
            }
        });
    }
    ui.painter().line_segment([pos2(rect.left(), header_rect.bottom()), pos2(rect.right(), header_rect.bottom())], Stroke::new(1.0, SpkColors::RULE));

    // Category filter bar + search
    let filter_rect = Rect::from_min_size(pos2(rect.left(), header_rect.bottom() + 1.0), vec2(rect.width(), 56.0));
    {
        let mut f_ui = ui.new_child(egui::UiBuilder::new().max_rect(filter_rect.shrink2(vec2(10.0, 4.0))));
        let cur_cat = f.get(FILTER_CAT_FIELD).and_then(Value::as_str).unwrap_or("all").to_string();
        f_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (cat, label) in [("all", "全部(22)"), ("negative", "负片(11)"), ("positive", "正片(4)"), ("cine", "电影(7)")] {
                if chip_button(ui, label, cur_cat == cat).clicked() {
                    f.insert(FILTER_CAT_FIELD.into(), json!(cat));
                }
            }
        });
        f_ui.add_space(4.0);
        let mut search = f.get(SEARCH_FIELD).and_then(Value::as_str).unwrap_or_default().to_string();
        let was_empty = search.is_empty();
        f_ui.horizontal(|ui| {
            let edit = egui::TextEdit::singleline(&mut search)
                .hint_text("搜索胶片型号 (如 Portra, Velvia, 500T)…")
                .desired_width(ui.available_width() - if was_empty { 0.0 } else { 24.0 })
                .font(egui::FontId::proportional(11.5));
            ui.add(edit);
            if !search.is_empty() && ui.small_button("×").clicked() {
                search.clear();
            }
        });
        if search.is_empty() {
            f.remove(SEARCH_FIELD);
        } else {
            f.insert(SEARCH_FIELD.into(), json!(search));
        }
    }
    ui.painter().line_segment([pos2(rect.left(), filter_rect.bottom()), pos2(rect.right(), filter_rect.bottom())], Stroke::new(1.0, SpkColors::RULE));

    // Scrollable Film & Print Stock Lists
    let scroll_rect = Rect::from_min_max(pos2(rect.left(), filter_rect.bottom() + 1.0), rect.max);
    let mut s_ui = ui.new_child(egui::UiBuilder::new().max_rect(scroll_rect));
    egui::ScrollArea::vertical()
        .id_salt("spektra-left-rail-scroll")
        .auto_shrink([false, false])
        .show(&mut s_ui, |ui| {
            ui.add_space(6.0);
            let cur_cat = f.get(FILTER_CAT_FIELD).and_then(Value::as_str).unwrap_or("all").to_string();
            let query = f.get(SEARCH_FIELD).and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
            let auto_paper = f.get(AUTO_PAPER_FIELD).and_then(Value::as_bool).unwrap_or(true);
            let mut selected_film = f.get("film").and_then(Value::as_str).unwrap_or("kodak_portra_400").to_string();

            // Section: Film
            section_header(ui, "▾  胶片 · Film", Some(&format!("{}", FILM_STOCKS.len())));

            let matches_filter = |s: &FilmStockInfo| -> bool {
                let cat_ok = match cur_cat.as_str() {
                    "positive" => s.polarity == Polarity::Positive,
                    "negative" => s.polarity == Polarity::Negative && !s.is_cine,
                    "cine" => s.is_cine,
                    _ => true,
                };
                let q_ok = query.is_empty()
                    || s.name.to_ascii_lowercase().contains(&query)
                    || s.id.to_ascii_lowercase().contains(&query)
                    || s.cn_tag.to_lowercase().contains(&query);
                cat_ok && q_ok
            };

            // Group 1: Positive
            let positives: Vec<&FilmStockInfo> = FILM_STOCKS.iter().filter(|s| s.polarity == Polarity::Positive && matches_filter(s)).collect();
            if !positives.is_empty() {
                stock_group_header(ui, "Positive · 彩色反转正片");
                for stock in positives {
                    if draw_film_row(ui, stock, selected_film == stock.id) {
                        selected_film = stock.id.to_string();
                        f.insert("film".into(), json!(stock.id));
                    }
                }
            }

            // Group 2: Negative (Still + Cine)
            let negatives: Vec<&FilmStockInfo> = FILM_STOCKS.iter().filter(|s| s.polarity == Polarity::Negative && matches_filter(s)).collect();
            if !negatives.is_empty() {
                stock_group_header(ui, "Negative · 彩色负片与电影胶片");
                for stock in negatives {
                    if draw_film_row(ui, stock, selected_film == stock.id) {
                        selected_film = stock.id.to_string();
                        f.insert("film".into(), json!(stock.id));
                        if auto_paper {
                            f.insert("paper".into(), json!(stock.target_paper));
                            f.insert("printChain".into(), json!(true));
                        }
                    }
                }
            }

            ui.add_space(10.0);
            rail_hairline(ui);
            ui.add_space(6.0);

            // Section: Print / Paper
            let is_positive = find_film(&selected_film).polarity == Polarity::Positive;
            let print_chain = f.get("printChain").and_then(Value::as_bool).unwrap_or(true);
            let effective_print = print_chain && !is_positive;
            let selected_paper = f.get("paper").and_then(Value::as_str).unwrap_or("kodak_portra_endura").to_string();
            section_header(
                ui,
                "▾  印相介质 · Print & Scan",
                Some(if is_positive {
                    "正片透射直扫"
                } else if print_chain {
                    "相纸输出"
                } else {
                    "负片直扫"
                }),
            );

            stock_group_header(ui, "Still Paper · 暗房银盐相纸（胶片观感）");
            ui.add_enabled_ui(!is_positive, |ui| {
                for paper in PAPER_STOCKS {
                    let is_sel = effective_print && selected_paper == paper.id;
                    if draw_paper_row(ui, paper, is_sel) && !is_positive {
                        f.insert("paper".into(), json!(paper.id));
                        f.insert("printChain".into(), json!(true));
                    }
                }
            });

            stock_group_header(
                ui,
                if is_positive {
                    "Positive Slide · 正片透射直扫（自动启用）"
                } else {
                    "Direct Scan · 负片原始直扫（无相纸）"
                },
            );
            if draw_direct_scan_row(ui, !effective_print, is_positive) && !is_positive {
                f.insert("printChain".into(), json!(false));
            }
            ui.add_space(12.0);
        });
}

fn draw_film_row(ui: &mut Ui, stock: &FilmStockInfo, selected: bool) -> bool {
    let avail_w = ui.available_width() - 16.0;
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(avail_w, 24.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, stock.name));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("{} — {}", stock.name, stock.cn_tag));
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::same(7), SpkColors::SELECTION);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(7), Color32::from_white_alpha(15));
        }

        let text_color = if selected { SpkColors::ON_SELECTION } else { SpkColors::TEXT };
        // Right side: 18x18 cover icon, and optional CINE pill before it
        let cover_rect = Rect::from_min_size(pos2(rect.right() - 22.0, rect.center().y - 9.0), vec2(18.0, 18.0));
        draw_stock_cover(ui, cover_rect, stock);

        let mut right_limit = cover_rect.left() - 6.0;
        if stock.is_cine {
            let cine_rect = Rect::from_min_size(pos2(right_limit - 30.0, rect.center().y - 6.5), vec2(28.0, 13.0));
            let cine_color = if selected { Color32::from_rgb(176, 108, 24) } else { SpkColors::ACCENT };
            ui.painter().rect_stroke(cine_rect, CornerRadius::same(4), Stroke::new(1.0, cine_color), StrokeKind::Inside);
            ui.painter().text(cine_rect.center(), Align2::CENTER_CENTER, "CINE", theme::semibold(8.5), cine_color);
            right_limit = cine_rect.left() - 5.0;
        }

        // Stock name clipped to `right_limit`
        let text_rect = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(right_limit, rect.bottom()));
        let painter = ui.painter().with_clip_rect(text_rect);
        painter.text(
            pos2(text_rect.left(), text_rect.center().y),
            Align2::LEFT_CENTER,
            stock.name,
            theme::semibold(11.5),
            text_color,
        );

        if resp.clicked() {
            clicked = true;
        }
    });
    clicked
}

fn draw_stock_cover(ui: &mut Ui, rect: Rect, stock: &FilmStockInfo) {
    if let Some(tex) = cover_texture(ui.ctx(), stock) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(tex.id(), rect, uv, Color32::WHITE);
        ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(0.5, Color32::from_black_alpha(90)), StrokeKind::Inside);
    } else {
        // Stylized two-tone film box badge for stocks without an embedded JPG
        let top_h = rect.height() * 0.68;
        let top_r = Rect::from_min_size(rect.min, vec2(rect.width(), top_h));
        let bot_r = Rect::from_min_max(pos2(rect.left(), rect.top() + top_h), rect.max);
        ui.painter().rect_filled(top_r, CornerRadius { nw: 2, ne: 2, sw: 0, se: 0 }, stock.badge_colors[0]);
        ui.painter().rect_filled(bot_r, CornerRadius { nw: 0, ne: 0, sw: 2, se: 2 }, stock.badge_colors[1]);
        ui.painter().text(top_r.center(), Align2::CENTER_CENTER, stock.badge_text, theme::semibold(6.5), Color32::WHITE);
    }
}

fn draw_paper_row(ui: &mut Ui, paper: &PaperStockInfo, selected: bool) -> bool {
    let avail_w = ui.available_width() - 16.0;
    let enabled = ui.is_enabled();
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(avail_w, 23.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, selected, paper.name));
        let resp = if enabled {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            resp
        }
        .on_hover_text(format!("{} — {}", paper.name, paper.cn_tag))
        .on_disabled_hover_text("彩色反转正片本身已是正像，无需经暗房相纸二次反相；切换负片/电影胶片后可选相纸");
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::same(7), SpkColors::SELECTION);
        } else if enabled && resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(7), Color32::from_white_alpha(15));
        }
        let text_color = if !enabled {
            SpkColors::DIM
        } else if selected {
            SpkColors::ON_SELECTION
        } else {
            SpkColors::TEXT
        };
        // Paper swatch pill on right
        let swatch = Rect::from_min_size(pos2(rect.right() - 20.0, rect.center().y - 6.0), vec2(14.0, 12.0));
        ui.painter().rect_filled(swatch, CornerRadius::same(2), paper.badge_colors[0]);
        ui.painter().rect_stroke(swatch, CornerRadius::same(2), Stroke::new(1.0, paper.badge_colors[1]), StrokeKind::Inside);

        let text_rect = Rect::from_min_max(pos2(rect.left() + 8.0, rect.top()), pos2(swatch.left() - 6.0, rect.bottom()));
        ui.painter().with_clip_rect(text_rect).text(
            pos2(text_rect.left(), text_rect.center().y),
            Align2::LEFT_CENTER,
            paper.name,
            theme::semibold(11.0),
            text_color,
        );
        if resp.clicked() {
            clicked = true;
        }
    });
    clicked
}

fn draw_direct_scan_row(ui: &mut Ui, selected: bool, is_positive: bool) -> bool {
    let avail_w = ui.available_width() - 16.0;
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let (label, tip) = if is_positive {
            (
                "正片透射直扫 · Slide Positive Scan",
                "正片/反转片（Velvia / Provia / Ektachrome / Kodachrome）显影后已是正像，自动采用正片透射直扫",
            )
        } else {
            (
                "不使用相纸配置 (负片直扫 · Direct Scan)",
                "跳过放大机与相纸显影，直接输出胶片密度扫描（负片将保留原始橙色色罩，仅供研究或外置反转）",
            )
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(avail_w, 23.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, label));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
        if selected {
            ui.painter().rect_filled(rect, CornerRadius::same(7), SpkColors::SELECTION);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(7), Color32::from_white_alpha(15));
        }
        let text_color = if selected { SpkColors::ON_SELECTION } else { SpkColors::SECONDARY };
        ui.painter().text(
            pos2(rect.left() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::semibold(11.0),
            text_color,
        );
        if resp.clicked() {
            clicked = true;
        }
    });
    clicked
}

// ---------------------------------------------------------------------------
// Center Darkroom Canvas (Viewfinder · Split Compare · Corner RGB Histogram)
// ---------------------------------------------------------------------------

fn draw_center_canvas(fed: &FedPreview, ui: &mut Ui, center_rect: Rect, f: &mut Map<String, Value>) -> ScopeStats {
    let footer_h = 28.0;
    let canvas_rect = Rect::from_min_max(center_rect.min, pos2(center_rect.right(), (center_rect.bottom() - footer_h).max(center_rect.top() + 80.0)));
    let footer_rect = Rect::from_min_max(pos2(center_rect.left(), canvas_rect.bottom()), center_rect.max);

    ui.painter().rect_filled(canvas_rect, CornerRadius::ZERO, SpkColors::GROUND);
    ui.painter().rect_filled(footer_rect, CornerRadius::ZERO, SpkColors::WELL);
    ui.painter().line_segment([pos2(footer_rect.left(), footer_rect.top()), pos2(footer_rect.right(), footer_rect.top())], Stroke::new(1.0, SpkColors::RULE));

    let cur_film_id = f.get("film").and_then(Value::as_str).unwrap_or("kodak_portra_400");
    let cur_paper_id = f.get("paper").and_then(Value::as_str).unwrap_or("kodak_portra_endura");
    let print_chain = f.get("printChain").and_then(Value::as_bool).unwrap_or(true);
    let film_info = find_film(cur_film_id);
    let paper_info = find_paper(cur_paper_id);
    let is_positive = film_info.polarity == Polarity::Positive;
    let effective_print = print_chain && !is_positive;

    // Draw pipeline status footer
    let pipe_desc = if is_positive {
        format!(
            "正片管线: D55 入射 (380–780nm @ 5nm)  →  {} ({})  →  D50 正片透射直扫 sRGB",
            film_info.name, film_info.cn_tag
        )
    } else if effective_print {
        format!(
            "光谱管线: D55 入射 (380–780nm @ 5nm)  →  {} ({})  →  TH-KG3 放大机 + {}  →  D50 扫描 sRGB",
            film_info.name, film_info.cn_tag, paper_info.short_name
        )
    } else {
        format!(
            "直扫管线: D55 入射 (380–780nm @ 5nm)  →  {} ({})  →  D50 负片直扫 (无相纸)",
            film_info.name, film_info.cn_tag
        )
    };
    ui.painter().with_clip_rect(footer_rect.shrink2(vec2(10.0, 0.0))).text(
        pos2(footer_rect.left() + 12.0, footer_rect.center().y),
        Align2::LEFT_CENTER,
        pipe_desc,
        theme::medium(11.0),
        SpkColors::SECONDARY,
    );

    let base_k = fed.base_k.max(1);
    let orig = fed.orig.clone();
    let filt = fed.filt.clone();

    let active_stats = filt.as_ref().or(orig.as_ref()).map(|c| c.stats.clone()).unwrap_or_default();
    let Some([img_w, img_h]) = filt.as_ref().map(|c| c.size).or_else(|| orig.as_ref().map(|c| c.size)) else {
        return active_stats;
    };
    if img_w == 0 || img_h == 0 {
        return active_stats;
    }

    let doc_w = (img_w as f32 * base_k as f32).max(1.0);
    let doc_h = (img_h as f32 * base_k as f32).max(1.0);

    let fit = f.get(FIT_FIELD).and_then(Value::as_bool).unwrap_or(true);
    let pad = 20.0;
    let inner_avail = (canvas_rect.size() - vec2(pad * 2.0, pad * 2.0)).max(vec2(64.0, 64.0));
    let fit_zoom = (inner_avail.x / doc_w).min(inner_avail.y / doc_h).clamp(0.05, 8.0);

    let mut zoom = if fit {
        fit_zoom
    } else {
        f.get(ZOOM_FIELD).and_then(Value::as_f64).map(|v| v as f32).filter(|z| z.is_finite() && *z > 0.0).unwrap_or(fit_zoom).clamp(0.125, 8.0)
    };
    let mut cx = f.get(CENTER_X_FIELD).and_then(Value::as_f64).map(|v| v as f32).filter(|v| v.is_finite()).unwrap_or(doc_w * 0.5);
    let mut cy = f.get(CENTER_Y_FIELD).and_then(Value::as_f64).map(|v| v as f32).filter(|v| v.is_finite()).unwrap_or(doc_h * 0.5);
    if fit {
        cx = doc_w * 0.5;
        cy = doc_h * 0.5;
    }

    let mut sub_ui = ui.new_child(egui::UiBuilder::new().max_rect(canvas_rect));
    let (_, response) = sub_ui.allocate_exact_size(canvas_rect.size(), Sense::click_and_drag());

    // Compute destination rect & UV
    let sw = canvas_rect.width() / zoom;
    let sh = canvas_rect.height() / zoom;
    let x0 = if sw >= doc_w { (doc_w - sw) * 0.5 } else { (cx - sw * 0.5).clamp(0.0, doc_w - sw) };
    let y0 = if sh >= doc_h { (doc_h - sh) * 0.5 } else { (cy - sh * 0.5).clamp(0.0, doc_h - sh) };
    let (vx0, vy0) = (x0.max(0.0), y0.max(0.0));
    let (vx1, vy1) = ((x0 + sw).min(doc_w).max(vx0), (y0 + sh).min(doc_h).max(vy0));
    let dest = Rect::from_min_max(
        canvas_rect.min + vec2((vx0 - x0) * zoom, (vy0 - y0) * zoom),
        canvas_rect.min + vec2((vx1 - x0) * zoom, (vy1 - y0) * zoom),
    );
    let uv = Rect::from_min_max(pos2(vx0 / doc_w, vy0 / doc_h), pos2(vx1 / doc_w, vy1 / doc_h));

    let painter = ui.painter_at(canvas_rect);
    // Subtle dark drop frame around the image
    painter.rect_filled(dest.expand(2.0), CornerRadius::same(2), SpkColors::WELL);
    draw_checker(&painter, dest, 8.0);

    let mode = f.get(VIEW_MODE_FIELD).and_then(Value::as_str).unwrap_or("process").to_string();
    let mut split_pos = f.get(SPLIT_POS_FIELD).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(0.5).clamp(0.05, 0.95);

    if mode == "split" {
        let split_x = dest.left() + dest.width() * split_pos;
        if let Some(o) = &orig {
            let left_clip = Rect::from_min_max(canvas_rect.min, pos2(split_x, canvas_rect.bottom()));
            painter.with_clip_rect(left_clip).image(o.tex.id(), dest, uv, Color32::WHITE);
        }
        if let Some(fl) = filt.as_ref().or(orig.as_ref()) {
            let right_clip = Rect::from_min_max(pos2(split_x, canvas_rect.top()), canvas_rect.max);
            painter.with_clip_rect(right_clip).image(fl.tex.id(), dest, uv, Color32::WHITE);
        }
        // Draw SpektraLab `.compare-line` and `.compare-handle`
        painter.line_segment([pos2(split_x, dest.top()), pos2(split_x, dest.bottom())], Stroke::new(1.5, Color32::from_rgb(250, 248, 244)));
        let handle_rect = Rect::from_center_size(pos2(split_x, dest.center().y), vec2(14.0, 28.0));
        painter.rect_filled(handle_rect, CornerRadius::same(7), Color32::from_rgb(250, 248, 244));
        painter.line_segment(
            [pos2(split_x - 2.0, handle_rect.center().y - 5.0), pos2(split_x - 2.0, handle_rect.center().y + 5.0)],
            Stroke::new(1.0, SpkColors::WELL),
        );
        painter.line_segment(
            [pos2(split_x + 2.0, handle_rect.center().y - 5.0), pos2(split_x + 2.0, handle_rect.center().y + 5.0)],
            Stroke::new(1.0, SpkColors::WELL),
        );

        // Floating labels
        let badge_l = Rect::from_min_size(pos2(dest.left() + 8.0, dest.top() + 8.0), vec2(68.0, 18.0));
        painter.rect_filled(badge_l, CornerRadius::same(4), Color32::from_black_alpha(150));
        painter.text(badge_l.center(), Align2::CENTER_CENTER, "原图 Original", theme::medium(10.0), SpkColors::TEXT);

        let badge_r = Rect::from_min_size(pos2((split_x + 8.0).min(dest.right() - 84.0), dest.top() + 8.0), vec2(80.0, 18.0));
        painter.rect_filled(badge_r, CornerRadius::same(4), Color32::from_black_alpha(150));
        painter.text(badge_r.center(), Align2::CENTER_CENTER, "显影 Processed", theme::medium(10.0), SpkColors::ACCENT);

        if (response.dragged() || response.clicked())
            && let Some(p) = response.interact_pointer_pos()
            && dest.width() > 1.0
        {
            split_pos = ((p.x - dest.left()) / dest.width()).clamp(0.05, 0.95);
            f.insert(SPLIT_POS_FIELD.into(), json!(split_pos));
        }
    } else {
        let holding = response.is_pointer_button_down_on() && !response.dragged();
        let show_orig = mode == "original" || holding;
        let shown = if show_orig { orig.as_ref() } else { filt.as_ref().or(orig.as_ref()) };
        if let Some(c) = shown {
            painter.image(c.tex.id(), dest, uv, Color32::WHITE);
        }
        if show_orig {
            let badge = Rect::from_min_size(pos2(dest.left() + 8.0, dest.top() + 8.0), vec2(78.0, 19.0));
            painter.rect_filled(badge, CornerRadius::same(4), Color32::from_black_alpha(165));
            painter.text(badge.center(), Align2::CENTER_CENTER, "原图 Original", theme::semibold(10.0), SpkColors::ACCENT);
        }
        if response.dragged() {
            let d = response.drag_delta();
            cx = (cx - d.x / zoom).clamp(0.0, doc_w);
            cy = (cy - d.y / zoom).clamp(0.0, doc_h);
            f.insert(FIT_FIELD.into(), json!(false));
            f.insert(ZOOM_FIELD.into(), json!(zoom));
            f.insert(CENTER_X_FIELD.into(), json!(cx));
            f.insert(CENTER_Y_FIELD.into(), json!(cy));
        }
    }

    // Mouse wheel zoom inside canvas_rect
    if ui.rect_contains_pointer(canvas_rect) {
        let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll_y.abs() > 0.5 {
            let factor = (1.0 + scroll_y * 0.003).clamp(0.8, 1.25);
            zoom = (zoom * factor).clamp(0.125, 8.0);
            f.insert(FIT_FIELD.into(), json!(false));
            f.insert(ZOOM_FIELD.into(), json!(zoom));
            f.insert(CENTER_X_FIELD.into(), json!(cx));
            f.insert(CENTER_Y_FIELD.into(), json!(cy));
        }
    }

    // Top-right corner RGB Histogram Overlay (`168 x 62`, exact match to `linux-real-host-cr2.png`)
    let hist_w = 168.0;
    let hist_h = 56.0;
    let hist_rect = Rect::from_min_size(pos2(canvas_rect.right() - hist_w - 10.0, canvas_rect.top() + 8.0), vec2(hist_w, hist_h));
    draw_corner_histogram(&painter, hist_rect, &active_stats);

    active_stats
}

fn draw_corner_histogram(painter: &egui::Painter, rect: Rect, stats: &ScopeStats) {
    painter.rect_filled(rect, CornerRadius::same(4), Color32::from_rgba_unmultiplied(24, 25, 24, 195));
    painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, Color32::from_white_alpha(28)), StrokeKind::Inside);

    let plot = rect.shrink2(vec2(5.0, 5.0));
    let n = 64;
    for (bins, col) in [
        (&stats.hist_y, SpkColors::HIST_Y),
        (&stats.hist_r, SpkColors::HIST_R),
        (&stats.hist_g, SpkColors::HIST_G),
        (&stats.hist_b, SpkColors::HIST_B),
    ] {
        let mut pts = Vec::with_capacity(n);
        for (i, &v) in bins.iter().enumerate() {
            let x = plot.left() + (i as f32 / (n - 1) as f32) * plot.width();
            let y = plot.bottom() - v.clamp(0.0, 1.0) * plot.height();
            pts.push(pos2(x, y));
        }
        painter.add(egui::Shape::line(pts, Stroke::new(1.15, col)));
    }
    painter.text(
        pos2(rect.right() - 4.0, rect.bottom() + 10.0),
        Align2::RIGHT_CENTER,
        "full · sRGB",
        theme::medium(9.5),
        SpkColors::TEXT,
    );
}

// ---------------------------------------------------------------------------
// Right Rail (`RightRail.tsx`: Latitude · Input/Camera · Chemistry · Enlarger)
// ---------------------------------------------------------------------------

fn draw_right_rail(ui: &mut Ui, f: &mut Map<String, Value>, stats: &ScopeStats) {
    let rect = ui.max_rect();
    // Header
    let header_rect = Rect::from_min_size(rect.min, vec2(rect.width(), 34.0));
    {
        let mut h_ui = ui.new_child(egui::UiBuilder::new().max_rect(header_rect.shrink2(vec2(12.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        h_ui.label(RichText::new("显影与暗房参数 · Parameters").font(theme::semibold(12.5)).color(SpkColors::TEXT));
        h_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if chip_button(ui, "重置参数 ↺", false).on_hover_text("将曝光、成色剂与放大机滤色头全部恢复为默认值").clicked() {
                f.insert("ev".into(), json!(0.0));
                f.insert("couplers".into(), json!(false));
                f.insert("couplersAmount".into(), json!(1.0));
                f.insert("inputGamut".into(), json!(false));
                f.insert("outputGamut".into(), json!(false));
                f.insert("printEv".into(), json!(0.0));
                f.insert("ccCyan".into(), json!(0.0));
                f.insert("ccMagenta".into(), json!(0.0));
                f.insert("ccYellow".into(), json!(0.0));
            }
        });
    }

    // Pre-Dev / Post-Dev / All Segmented Switch
    let seg_rect = Rect::from_min_size(pos2(rect.left(), header_rect.bottom()), vec2(rect.width(), 28.0));
    let tab = f.get(PARAM_TAB_FIELD).and_then(Value::as_str).unwrap_or("all").to_string();
    {
        let mut s_ui = ui.new_child(egui::UiBuilder::new().max_rect(seg_rect.shrink2(vec2(12.0, 2.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        s_ui.spacing_mut().item_spacing.x = 6.0;
        for (id, label) in [("all", "全部展开 All"), ("preDev", "显影前 Pre-Dev"), ("postDev", "印相 Post-Dev")] {
            if segmented_pill(&mut s_ui, label, tab == id).clicked() {
                f.insert(PARAM_TAB_FIELD.into(), json!(id));
            }
        }
    }
    ui.painter().line_segment([pos2(rect.left(), seg_rect.bottom() + 2.0), pos2(rect.right(), seg_rect.bottom() + 2.0)], Stroke::new(1.0, SpkColors::RULE));

    let scroll_rect = Rect::from_min_max(pos2(rect.left(), seg_rect.bottom() + 3.0), rect.max);
    let mut r_ui = ui.new_child(egui::UiBuilder::new().max_rect(scroll_rect));
    egui::ScrollArea::vertical()
        .id_salt("spektra-right-rail-scroll")
        .auto_shrink([false, false])
        .show(&mut r_ui, |ui| {
            ui.add_space(6.0);
            let show_pre = tab == "all" || tab == "preDev";
            let show_post = tab == "all" || tab == "postDev";

            let cur_film = f.get("film").and_then(Value::as_str).unwrap_or("kodak_portra_400").to_string();
            let cur_paper = f.get("paper").and_then(Value::as_str).unwrap_or("kodak_portra_endura").to_string();
            let print_chain = f.get("printChain").and_then(Value::as_bool).unwrap_or(true);
            let film_info = find_film(&cur_film);
            let paper_info = find_paper(&cur_paper);
            let is_positive = film_info.polarity == Polarity::Positive;
            let effective_print = print_chain && !is_positive;

            // 1. Latitude Section
            if show_pre {
                let trailing = if is_positive {
                    format!("{} · 正片直扫", film_info.short_name)
                } else if effective_print {
                    format!("{} · {}", film_info.short_name, paper_info.short_name)
                } else {
                    format!("{} · 负片直扫", film_info.short_name)
                };
                section_header(ui, "▾  宽容度 · Latitude", Some(&trailing));
                draw_latitude_plot(ui, stats);
                ui.add_space(8.0);
                rail_hairline(ui);
                ui.add_space(6.0);

                // 2. Input / Camera Section
                if section_header_with_reset(ui, "▾  输入 / 相机 · Input / Camera", "重置相机曝光与输入色域") {
                    f.insert("ev".into(), json!(0.0));
                    f.insert("inputGamut".into(), json!(false));
                }
                let mut ev = f.get("ev").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                if scrub_slider_row(ui, "胶片曝光 EV", &mut ev, -4.0..=4.0, 0.0, None, |v| format!("{v:+.2}")) {
                    f.insert("ev".into(), json!((f64::from(ev) * 100.0).round() / 100.0));
                }
                // As Shot checkbox + 1/3 EV step buttons
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let as_shot = ev.abs() < 0.005;
                    if spk_checkbox(ui, as_shot, "拍摄时设置 (0 EV)") {
                        f.insert("ev".into(), json!(0.0));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        ui.add_space(10.0);
                        for (label, delta) in [("+1", 1.0f32), ("+⅓", 0.3333), ("0", 0.0), ("-⅓", -0.3333), ("-1", -1.0)] {
                            if small_pill_button(ui, label, false).clicked() {
                                let next = if label == "0" { 0.0 } else { (ev + delta).clamp(-4.0, 4.0) };
                                f.insert("ev".into(), json!((f64::from(next) * 100.0).round() / 100.0));
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                let mut in_gamut = f.get("inputGamut").and_then(Value::as_bool).unwrap_or(false);
                if toggle_row(
                    ui,
                    "输入光谱色域压缩 (Input Gamut)",
                    &mut in_gamut,
                    true,
                    "ACES RGC 光谱轨迹径向压缩，防止极端霓虹饱和色在光谱上采样时溢出",
                ) {
                    f.insert("inputGamut".into(), json!(in_gamut));
                }

                ui.add_space(8.0);
                rail_hairline(ui);
                ui.add_space(6.0);

                // 3. Film Development & DIR Couplers Section
                if section_header_with_reset(ui, "▾  显影与成色剂 · Film Chemistry", "重置 DIR 成色剂") {
                    f.insert("couplers".into(), json!(false));
                    f.insert("couplersAmount".into(), json!(1.0));
                }
                let mut couplers = f.get("couplers").and_then(Value::as_bool).unwrap_or(false) && !is_positive;
                let couplers_tip = if is_positive {
                    "正片反转胶片的 DIR 反射成色剂分支暂未开放，当前仅支持负片与电影胶片"
                } else {
                    "开启物理层间与同层显影抑制成色剂矩阵，增强胶片色彩分离度与特征曲线"
                };
                if toggle_row(ui, "DIR 显影抑制成色剂 (Couplers)", &mut couplers, !is_positive, couplers_tip) {
                    f.insert("couplers".into(), json!(couplers));
                }
                let mut amount = f.get("couplersAmount").and_then(Value::as_f64).unwrap_or(1.0) as f32;
                ui.add_enabled_ui(couplers && !is_positive, |ui| {
                    if scrub_slider_row(ui, "成色剂强度", &mut amount, 0.0..=2.0, 1.0, None, |v| format!("{v:.2}")) {
                        f.insert("couplersAmount".into(), json!((f64::from(amount) * 100.0).round() / 100.0));
                    }
                });

                ui.add_space(8.0);
                rail_hairline(ui);
                ui.add_space(6.0);
            }

            if show_post {
                // 4. Enlarger & Dichroic Color Head (Print stage)
                if section_header_with_reset(ui, "▾  放大机滤光头 · Enlarger (TH-KG3)", "将放大机恢复为数据库中性配光结果") {
                    f.insert("printEv".into(), json!(0.0));
                    f.insert("ccCyan".into(), json!(0.0));
                    f.insert("ccMagenta".into(), json!(0.0));
                    f.insert("ccYellow".into(), json!(0.0));
                }

                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    let note = if is_positive {
                        "当前为反转正片，已自动跳过放大机与相纸反相链".to_string()
                    } else if effective_print {
                        let base_cc = photocraft_engine::spektra_cmds::neutral_cc_for(&cur_paper, &cur_film);
                        format!("配光基准: C {:.1} · M {:.1} · Y {:.1}", base_cc[0], base_cc[1], base_cc[2])
                    } else {
                        "当前为胶片直扫模式，放大机与相纸未启用".to_string()
                    };
                    ui.label(RichText::new(note).font(theme::mono(10.5)).color(SpkColors::DIM));
                });
                ui.add_space(2.0);

                ui.add_enabled_ui(effective_print, |ui| {
                    let mut print_ev = f.get("printEv").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    if scrub_slider_row(ui, "印相曝光补偿", &mut print_ev, -2.0..=2.0, 0.0, None, |v| format!("{v:+.2}")) {
                        f.insert("printEv".into(), json!((f64::from(print_ev) * 100.0).round() / 100.0));
                    }
                    let mut cc_c = f.get("ccCyan").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    if scrub_slider_row(
                        ui,
                        "青色滤片 Cyan",
                        &mut cc_c,
                        -50.0..=50.0,
                        0.0,
                        Some((Color32::from_rgb(210, 68, 68), Color32::from_rgb(52, 188, 212))),
                        |v| format!("{v:+.1}"),
                    ) {
                        f.insert("ccCyan".into(), json!((f64::from(cc_c) * 10.0).round() / 10.0));
                    }
                    let mut cc_m = f.get("ccMagenta").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    if scrub_slider_row(
                        ui,
                        "品红滤片 Magenta",
                        &mut cc_m,
                        -50.0..=50.0,
                        0.0,
                        Some((Color32::from_rgb(72, 188, 92), Color32::from_rgb(212, 68, 168))),
                        |v| format!("{v:+.1}"),
                    ) {
                        f.insert("ccMagenta".into(), json!((f64::from(cc_m) * 10.0).round() / 10.0));
                    }
                    let mut cc_y = f.get("ccYellow").and_then(Value::as_f64).unwrap_or(0.0) as f32;
                    if scrub_slider_row(
                        ui,
                        "黄色滤片 Yellow",
                        &mut cc_y,
                        -50.0..=50.0,
                        0.0,
                        Some((Color32::from_rgb(68, 108, 218), Color32::from_rgb(232, 198, 52))),
                        |v| format!("{v:+.1}"),
                    ) {
                        f.insert("ccYellow".into(), json!((f64::from(cc_y) * 10.0).round() / 10.0));
                    }
                });

                ui.add_space(8.0);
                rail_hairline(ui);
                ui.add_space(6.0);

                // 5. Scanner & Output Gamut Section
                section_header(ui, "▾  扫描与输出 · Scanner & Output", None);
                let mut out_gamut = f.get("outputGamut").and_then(Value::as_bool).unwrap_or(false);
                if toggle_row(
                    ui,
                    "CAM16-UCS 输出色域压缩",
                    &mut out_gamut,
                    true,
                    "在 CIECAM16-UCS 均匀色彩空间对高饱和色彩做平滑色度 roll-off 至 sRGB 色域内",
                ) {
                    f.insert("outputGamut".into(), json!(out_gamut));
                }
                let mut preview = f.get("__preview").and_then(Value::as_bool).unwrap_or(true);
                if toggle_row(ui, "同步主画布实时预览 (Preview)", &mut preview, true, "开启后主窗口画布同步显示冲印效果") {
                    f.insert("__preview".into(), json!(preview));
                }
                ui.add_space(12.0);
            }
        });
}

fn draw_latitude_plot(ui: &mut Ui, stats: &ScopeStats) {
    let avail_w = (ui.available_width() - 24.0).max(160.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let (rect, _) = ui.allocate_exact_size(vec2(avail_w, 54.0), Sense::hover());
        let p = ui.painter_at(rect);
        p.rect_filled(rect, CornerRadius::same(3), SpkColors::PLOT);
        // Latitude sweet-spot window (-3.2 EV .. +3.2 EV -> middle 64% of width)
        let win_l = rect.left() + rect.width() * 0.18;
        let win_r = rect.left() + rect.width() * 0.82;
        p.rect_filled(Rect::from_min_max(pos2(win_l, rect.top()), pos2(win_r, rect.bottom())), CornerRadius::ZERO, SpkColors::LAT_BEYOND);

        let n = stats.lat_bins.len();
        let bar_w = (rect.width() / n as f32).max(1.0);
        for (i, &v) in stats.lat_bins.iter().enumerate() {
            let h = v.clamp(0.0, 1.0) * (rect.height() - 6.0);
            let x = rect.left() + i as f32 * bar_w;
            let bar = Rect::from_min_max(pos2(x, rect.bottom() - h), pos2(x + bar_w - 0.5, rect.bottom()));
            p.rect_filled(bar, CornerRadius::ZERO, SpkColors::LAT_INSIDE);
        }
        p.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, SpkColors::RULE), StrokeKind::Inside);
    });
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(
            RichText::new(format!(
                "低于 {:.1}% · 范围内 {:.1}% · 高于 {:.1}% · 6.4 档保留",
                stats.below_pct, stats.within_pct, stats.above_pct
            ))
            .font(theme::mono(9.5))
            .color(SpkColors::DIM),
        );
    });
}

// ---------------------------------------------------------------------------
// SpektraLab Custom Micro-Widgets (ScrubSlider, Pills, Checkboxes, Headers)
// ---------------------------------------------------------------------------

fn section_header(ui: &mut Ui, title: &str, trailing: Option<&str>) {
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(title).font(theme::semibold(11.5)).color(SpkColors::TEXT));
        if let Some(tr) = trailing {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(tr).font(theme::medium(10.0)).color(SpkColors::DIM));
            });
        }
    });
    ui.add_space(2.0);
}

fn section_header_with_reset(ui: &mut Ui, title: &str, reset_tip: &str) -> bool {
    let mut reset = false;
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(title).font(theme::semibold(11.5)).color(SpkColors::TEXT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            if small_pill_button(ui, "↺", false).on_hover_text(reset_tip).clicked() {
                reset = true;
            }
        });
    });
    ui.add_space(2.0);
    reset
}

fn stock_group_header(ui: &mut Ui, label: &str) {
    ui.add_space(3.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(label).font(theme::medium(10.5)).color(SpkColors::DIM));
    });
    ui.add_space(1.0);
}

fn rail_hairline(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, SpkColors::RULE));
}

fn action_pill(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(11.5), Color32::WHITE);
    let w = (galley.size().x + 20.0).max(88.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 23.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let border_col = if active {
        SpkColors::ACCENT
    } else if resp.hovered() {
        SpkColors::SECONDARY
    } else {
        SpkColors::DIM
    };
    let text_col = if active {
        SpkColors::ACCENT
    } else if resp.hovered() {
        SpkColors::TEXT
    } else {
        SpkColors::SECONDARY
    };
    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(99), Color32::from_rgba_unmultiplied(225, 169, 95, 28));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(99), Color32::from_white_alpha(12));
    }
    ui.painter().rect_stroke(rect, CornerRadius::same(99), Stroke::new(1.0, border_col), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::semibold(11.5), text_col);
    resp
}

fn segmented_pill(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(10.5), Color32::WHITE);
    let w = (galley.size().x + 14.0).max(74.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let col = if active { SpkColors::ACCENT } else { SpkColors::DIM };
    ui.painter().rect_stroke(rect, CornerRadius::same(99), Stroke::new(1.0, col), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::semibold(10.5), col);
    resp
}

fn chip_button(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::medium(10.5), Color32::WHITE);
    let w = galley.size().x + 12.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 19.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(99), SpkColors::ACCENT);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::semibold(10.5), SpkColors::ON_SELECTION);
    } else {
        let bg = if resp.hovered() { Color32::from_white_alpha(22) } else { SpkColors::PILL };
        ui.painter().rect_filled(rect, CornerRadius::same(99), bg);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::medium(10.5), SpkColors::TEXT);
    }
    resp
}

fn small_pill_button(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(10.5), Color32::WHITE);
    let w = (galley.size().x + 12.0).max(24.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 19.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let bg = if active {
        SpkColors::ACCENT
    } else if resp.hovered() {
        Color32::from_white_alpha(28)
    } else {
        SpkColors::PILL
    };
    let fg = if active { SpkColors::ON_SELECTION } else { SpkColors::TEXT };
    ui.painter().rect_filled(rect, CornerRadius::same(99), bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::semibold(10.5), fg);
    resp
}

fn spk_checkbox(ui: &mut Ui, on: bool, label: &str) -> bool {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::medium(10.0), SpkColors::DIM);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 16.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, theme::medium(10.0), SpkColors::DIM);
    let box_r = Rect::from_min_size(pos2(rect.right() - 11.0, rect.center().y - 5.5), vec2(11.0, 11.0));
    if on {
        ui.painter().rect_filled(box_r, CornerRadius::same(2), SpkColors::ACCENT);
    } else {
        ui.painter().rect_stroke(box_r, CornerRadius::same(2), Stroke::new(1.0, SpkColors::DIM), StrokeKind::Inside);
    }
    resp.clicked()
}

fn toggle_row(ui: &mut Ui, label: &str, on: &mut bool, enabled: bool, tip: &str) -> bool {
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let avail = (ui.available_width() - 12.0).max(120.0);
            let (rect, resp) = ui.allocate_exact_size(vec2(avail, 22.0), Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
            ui.painter().text(pos2(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, theme::semibold(11.0), SpkColors::TEXT);
            let box_r = Rect::from_min_size(pos2(rect.right() - 12.0, rect.center().y - 6.0), vec2(12.0, 12.0));
            if *on {
                ui.painter().rect_filled(box_r, CornerRadius::same(3), SpkColors::ACCENT);
            } else {
                ui.painter().rect_stroke(box_r, CornerRadius::same(3), Stroke::new(1.0, SpkColors::DIM), StrokeKind::Inside);
            }
            if resp.clicked() {
                *on = !*on;
                changed = true;
            }
        });
    });
    changed
}

fn scrub_slider_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    zero: f32,
    gradient: Option<(Color32, Color32)>,
    fmt: impl Fn(f32) -> String,
) -> bool {
    let (min, max) = (*range.start(), *range.end());
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let label_r = ui.allocate_exact_size(vec2(88.0, 22.0), Sense::click());
        if label_r.1.double_clicked() {
            *value = zero;
            changed = true;
        }
        ui.painter().text(
            pos2(label_r.0.left(), label_r.0.center().y),
            Align2::LEFT_CENTER,
            label,
            theme::semibold(11.0),
            SpkColors::TEXT,
        );

        let track_w = (ui.available_width() - 62.0).max(48.0);
        let (s_rect, s_resp) = ui.allocate_exact_size(vec2(track_w, 20.0), Sense::click_and_drag());
        let s_resp = s_resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text("拖动调节，双击恢复默认值");
        if s_resp.double_clicked() {
            *value = zero;
            changed = true;
        } else if (s_resp.dragged() || s_resp.clicked())
            && let Some(p) = s_resp.interact_pointer_pos()
            && s_rect.width() > 1.0
        {
            let t = ((p.x - s_rect.left()) / s_rect.width()).clamp(0.0, 1.0);
            *value = min + t * (max - min);
            changed = true;
        }

        // Paint track (or colour gradient bar for Enlarger C/M/Y)
        let cy = s_rect.center().y;
        if let Some((c0, c1)) = gradient {
            let steps = 16;
            let seg_w = s_rect.width() / steps as f32;
            for i in 0..steps {
                let k = i as f32 / (steps - 1) as f32;
                let col = Color32::from_rgb(
                    (c0.r() as f32 * (1.0 - k) + c1.r() as f32 * k) as u8,
                    (c0.g() as f32 * (1.0 - k) + c1.g() as f32 * k) as u8,
                    (c0.b() as f32 * (1.0 - k) + c1.b() as f32 * k) as u8,
                );
                let r = Rect::from_min_size(pos2(s_rect.left() + i as f32 * seg_w, cy - 1.5), vec2(seg_w + 0.5, 3.0));
                ui.painter().rect_filled(r, CornerRadius::same(1), col);
            }
        } else {
            ui.painter().line_segment([pos2(s_rect.left(), cy), pos2(s_rect.right(), cy)], Stroke::new(1.5, SpkColors::DIM));
        }

        // Zero tick mark
        let zt = ((zero - min) / (max - min)).clamp(0.0, 1.0);
        let zx = s_rect.left() + zt * s_rect.width();
        ui.painter().line_segment([pos2(zx, cy - 3.5), pos2(zx, cy + 3.5)], Stroke::new(1.0, SpkColors::SECONDARY));

        // Knob
        let vt = ((*value - min) / (max - min)).clamp(0.0, 1.0);
        let kx = s_rect.left() + vt * s_rect.width();
        let knob_r = Rect::from_center_size(pos2(kx, cy), vec2(8.0, 8.0));
        ui.painter().rect_filled(knob_r, CornerRadius::same(4), SpkColors::KNOB);

        // Value pill readout
        let (pill_r, pill_resp) = ui.allocate_exact_size(vec2(46.0, 17.0), Sense::click());
        if pill_resp.on_hover_text("点击重置为默认值").clicked() {
            *value = zero;
            changed = true;
        }
        ui.painter().rect_filled(pill_r, CornerRadius::same(8), SpkColors::PILL);
        ui.painter().text(pill_r.center(), Align2::CENTER_CENTER, fmt(*value), theme::mono(10.0), SpkColors::TEXT);
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::framework::BuildApp;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn catalog_covers_and_ids_match_engine_schema() {
        assert_eq!(FILM_STOCKS.len(), 22);
        assert_eq!(PAPER_STOCKS.len(), 6);
        for f in FILM_STOCKS {
            assert!(PAPER_STOCKS.iter().any(|p| p.id == f.target_paper), "missing target paper for {}", f.id);
            if let Some(bytes) = f.cover_jpg {
                assert!(!bytes.is_empty(), "empty cover for {}", f.id);
            }
        }
        let spec = photocraft_engine::commands::find(COMMAND).expect("filmSim command exists");
        for param in Param::parse_list(spec.params) {
            if let ParamKind::Choice { options, .. } = param.kind {
                if param.key == "film" {
                    assert_eq!(options.len(), FILM_STOCKS.len());
                    for (opt, _) in &options {
                        assert!(FILM_STOCKS.iter().any(|s| s.id == opt), "film catalog missing {opt}");
                    }
                } else if param.key == "paper" {
                    assert_eq!(options.len(), PAPER_STOCKS.len());
                    for (opt, _) in &options {
                        assert!(PAPER_STOCKS.iter().any(|p| p.id == opt), "paper catalog missing {opt}");
                    }
                }
            }
        }
    }

    #[test]
    fn studio_dialog_renders_auto_pairs_paper_and_resets_params() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 32, "height": 24})).unwrap();
        let id = crate::dialogs::open_command_dialog(&mut app, COMMAND, "Film Simulation…").unwrap();
        {
            let d = app.framework.dialogs.get_by_id::<SpektraDialog>(id).unwrap();
            assert!(owns(d.fields()));
            assert_eq!(d.fields().get("printChain").and_then(Value::as_bool), Some(true));
        }

        let mut h = egui_kittest::Harness::builder().allow_missing_glyphs().with_size(vec2(1280.0, 820.0)).build_app(
            |ui, app| {
                crate::dialogs::run(app, ui);
            },
            app,
        );
        h.run_steps(5);

        // Clicking Fujifilm Pro 400H should select it and auto-pair Fujifilm Crystal Archive Type II.
        h.get_by_label("Fujifilm Pro 400H").click();
        h.run_steps(3);
        {
            let d = h.state_mut().framework.dialogs.get_mut_by_id::<SpektraDialog>(id).unwrap();
            assert_eq!(d.fields().get("film").and_then(Value::as_str), Some("fujifilm_pro_400h"));
            assert_eq!(d.fields().get("paper").and_then(Value::as_str), Some("fujifilm_crystal_archive_typeii"));
            d.fields_mut().insert("ev".into(), json!(1.5));
            d.fields_mut().insert("couplers".into(), json!(true));
            d.fields_mut().insert("ccYellow".into(), json!(12.0));
        }
        h.run_steps(2);

        // Reset button should restore extended params while preserving selected film/paper.
        h.get_by_label("重置参数 ↺").click();
        h.run_steps(2);
        let d = h.state().framework.dialogs.get_by_id::<SpektraDialog>(id).unwrap();
        assert_eq!(d.fields().get("film").and_then(Value::as_str), Some("fujifilm_pro_400h"));
        assert_eq!(d.fields().get("ev").and_then(Value::as_f64), Some(0.0));
        assert_eq!(d.fields().get("couplers").and_then(Value::as_bool), Some(false));
        assert_eq!(d.fields().get("ccYellow").and_then(Value::as_f64), Some(0.0));
    }
}
