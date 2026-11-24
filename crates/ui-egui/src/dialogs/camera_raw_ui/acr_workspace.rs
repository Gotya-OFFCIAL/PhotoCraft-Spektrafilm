//! Photoshop Adobe Camera Raw (ACR)-style full-window RAW Develop workspace.
//!
//! Renders inside the main window via [`FullWindowDialog`] (`Mount::FullWindow`), avoiding OS secondary
//! viewport collapse on Windows while preserving 100% of PhotoCraft's RAW develop pipeline, XMP sidecar
//! round-trip, filmstrip switching, undo/redo history, held-OK finish, and automation JSON contract.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use craft::behavior::edit_history::Step;
use craft::behavior::kurbo::{Point, Rect as DocRect};
use craft::behavior::{CancelReason, Cursor, CursorIcon, EditEvent, EditHistory, Mixed, PixelSize};
use craft::color_ui::ui::{ColorSamplerMarks, ColorSamplerTool, ColorSamplers, SamplerEvent, SamplerReadout};
use craft::imaging::raw::{Demosaic, RawDevelopSettings as CameraRaw, VignetteStyle};
use craft::imaging::scopes::ClippingMask;
use craft::imaging::ui::live_preview::settings_hash;
use craft::imaging::ui::raw_develop::{RawDevelopCommands, RawDevelopOpen, RawDevelopSource, RawFilmItem, RawFinish, RawPreviewRequest, RawTarget};
use craft::imaging::ui::scopes::{
    ClippingDiagnostic, ClippingMode, ClippingOverlay, ScopeCommand, ScopePanelState, ScopeReadout, ScopeSettings, VectorscopeFrame,
    VectorscopePlot,
};
use craft::ui::canvas::{
    CanvasTool, NavigationOptions, PixelRect, RasterSource, ToolAction, ToolContext, ToolPath, ToolPointer,
    ViewCommands, ZoomTool,
};
use craft::ui::contract::{Bind, Commands as _, Pattern, Remember as _, Show as _, ShowContainer as _};
use craft::ui::dialog::{
    DialogResult, FieldRefusal, FullWindowCanvas, FullWindowDialog, FullWindowTitle, FullWindowView, FullWindowWorkspace, WorkspaceCx,
    WorkspacePass, serde_fields, set_serde_field,
};
use craft::ui::layout_components::Stack;
use craft::ui::primitives::{Checker, Draw, Revision};
use craft::ui::runtime::commands::{CommandSet as _, FirePhase};
use craft::ui::runtime::env::{Env, Label};
use craft::ui::runtime::focus::EnterRule;
use craft::ui::runtime::memory::{SaveKey, saved_value, set_saved_value};
use craft::ui::runtime::pending::Pending;
use egui::{Align2, Color32, ColorImage, Context, CornerRadius, Id, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use photocraft_algo::vectorscope::{HUES, SATURATIONS};

use super::PcRawSource;
use super::acr_widgets::{
    AcrColors, HistogramDragOutcome, SliderEdit, ToolIcon, acr_pill_button, acr_primary_button, acr_section, acr_slider_row, draw_acr_histogram,
    draw_color_grading_panel, draw_color_mixer_panel, draw_curve_panel, draw_filmstrip, draw_readout_strip, tool_rail_button, tr,
};
use crate::theme;

/// Shared saved slot of the scope's settings (`filter.cameraRaw.scope`).
const SCOPE_KEY: SaveKey = SaveKey::new("craft_imaging.raw_develop.scope");

/// Output events consumed by `camera_raw_ui::take_with`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AcrDevelopEvent {
    Settings(Box<EditEvent<CameraRaw>>),
    Show(usize),
    LoadSettings,
    SaveSettings,
}

/// Output result consumed by `camera_raw_ui::finish`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct AcrDevelopResult {
    pub finish: RawFinish,
    pub settings: CameraRaw,
    pub demosaic: Option<Demosaic>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ViewMode {
    #[default]
    Single,
    Split,
    Before,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AcrTool {
    #[default]
    Hand,
    Eyedropper,
    Zoom,
    Sampler,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum WbPreset {
    #[default]
    AsShot,
    Auto,
    Daylight,
    Cloudy,
    Shade,
    Tungsten,
    Fluorescent,
    Custom,
}

impl WbPreset {
    fn label(self) -> &'static str {
        match self {
            WbPreset::AsShot => tr("原照设置", "As Shot"),
            WbPreset::Auto => tr("自动", "Auto"),
            WbPreset::Daylight => tr("日光", "Daylight"),
            WbPreset::Cloudy => tr("阴天", "Cloudy"),
            WbPreset::Shade => tr("阴影", "Shade"),
            WbPreset::Tungsten => tr("钨丝灯", "Tungsten"),
            WbPreset::Fluorescent => tr("荧光灯", "Fluorescent"),
            WbPreset::Custom => tr("自定义", "Custom"),
        }
    }

    fn fixed_values(self) -> Option<(f32, f32)> {
        match self {
            WbPreset::AsShot => Some((0.0, 0.0)),
            WbPreset::Daylight => Some((4.0, 2.0)),
            WbPreset::Cloudy => Some((18.0, 4.0)),
            WbPreset::Shade => Some((32.0, 6.0)),
            WbPreset::Tungsten => Some((-32.0, -4.0)),
            WbPreset::Fluorescent => Some((-14.0, 22.0)),
            WbPreset::Auto | WbPreset::Custom => None,
        }
    }
}

/// Computes `(temperature, tint)` in `-100.0..=100.0` that neutralizes a linear RGB sample using the exact
/// inverse of `photocraft_algo::camera_raw::white_balance_gains`.
pub(crate) fn compute_wb_from_rgb(rgb: [f32; 3]) -> Option<(f32, f32)> {
    let [r, g, b] = rgb;
    if !rgb.iter().all(|v| v.is_finite() && *v > 1e-5) {
        return None;
    }
    let rb_sum = r + b;
    if rb_sum <= 1e-5 || g <= 1e-5 {
        return None;
    }
    let t = ((b - r) / (0.35 * rb_sum)).clamp(-1.0, 1.0);
    let r_bal = r * (1.0 + 0.35 * t);
    let b_bal = b * (1.0 - 0.35 * t);
    let avg_rb = 0.5 * (r_bal + b_bal);
    let tn = ((1.0 - avg_rb / g) / 0.25).clamp(-1.0, 1.0);
    let temp = (t * 100.0).round().clamp(-100.0, 100.0);
    let tint = (tn * 100.0).round().clamp(-100.0, 100.0);
    Some((temp, tint))
}

fn decode_to_linear(v: f32, encoding_gamma: f32) -> f32 {
    if encoding_gamma.is_finite() && encoding_gamma > 0.0 {
        v.max(0.0).powf(encoding_gamma.clamp(0.5, 4.0))
    } else if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Composite `RasterSource` rendering `before` for document pixels `x < split_x` and `after` for `x >= split_x`.
pub(crate) struct SplitRaster<'a> {
    before: &'a dyn RasterSource,
    after: &'a dyn RasterSource,
    split_x: i32,
}

impl<'a> SplitRaster<'a> {
    pub(crate) fn new(before: &'a dyn RasterSource, after: &'a dyn RasterSource, split_x: i32) -> Self {
        Self { before, after, split_x }
    }
}

impl RasterSource for SplitRaster<'_> {
    fn size(&self) -> PixelSize {
        self.after.size()
    }

    fn revision(&self) -> Revision {
        Revision::of(&(self.before.revision(), self.after.revision(), self.split_x))
    }

    fn damage(&self, _since: Revision) -> Option<PixelRect> {
        // Split line sweeps invalidate across the raster; both sides are already cached in memory.
        None
    }

    fn read(&self, region: PixelRect, factor: u32, out: &mut ColorImage) {
        self.after.read(region, factor, out);
        let step = i64::from(factor.max(1));
        let [w, h] = out.size;
        if w == 0 || h == 0 || region.x0 >= self.split_x {
            return;
        }
        let before_texels = ((i64::from(self.split_x) - i64::from(region.x0) + step - 1) / step).clamp(0, w as i64) as usize;
        if before_texels == 0 {
            return;
        }
        let mut left_img = ColorImage::new([before_texels, h], vec![Color32::TRANSPARENT; before_texels * h]);
        let left_end_x = region.x0.saturating_add((before_texels as i64 * step).clamp(0, i64::from(i32::MAX)) as i32);
        let left_region = PixelRect::new(region.x0, region.y0, left_end_x, region.y1);
        self.before.read(left_region, factor, &mut left_img);
        for ty in 0..h {
            let src_row = &left_img.pixels[ty * before_texels..(ty + 1) * before_texels];
            let dst_row = &mut out.pixels[ty * w..ty * w + before_texels];
            dst_row.copy_from_slice(src_row);
        }
    }
}

/// Lightweight canvas tool for White Balance Eyedropper (`I`).
struct WbEyedropperTool {
    size: [u32; 2],
    picked: Option<[f64; 2]>,
}

impl WbEyedropperTool {
    fn new(size: [u32; 2]) -> Self {
        Self { size, picked: None }
    }

    fn doc_at(cx: &ToolContext<'_>, pos: Pos2) -> Option<[f64; 2]> {
        let Point { x, y } = cx.space()?.view.to_doc(pos);
        Some([x, y])
    }
}

impl CanvasTool for WbEyedropperTool {
    fn press(&mut self, pointer: &ToolPointer, cx: &mut ToolContext<'_>) -> ToolAction {
        let Some([x, y]) = Self::doc_at(cx, pointer.pos) else {
            return ToolAction::Ignored;
        };
        if x >= 0.0 && y >= 0.0 && x < f64::from(self.size[0]) && y < f64::from(self.size[1]) {
            self.picked = Some([x, y]);
            return ToolAction::Done;
        }
        ToolAction::Ignored
    }

    fn drag(&mut self, _pointer: &ToolPointer, _cx: &mut ToolContext<'_>) {}
    fn release(&mut self, _pointer: &ToolPointer, _cx: &mut ToolContext<'_>) {}
    fn cancel(&mut self, _reason: CancelReason, _cx: &mut ToolContext<'_>) {}
    fn paint(&self, _draw: &Draw<'_>, _cx: &ToolContext<'_>) {}

    fn cursor(&self, _cx: &ToolContext<'_>) -> Cursor {
        Cursor::System(CursorIcon::Crosshair)
    }

    fn autoscrolls(&self) -> bool {
        false
    }

    fn command_form(&self) -> ToolPath {
        ToolPath::Command("White Balance Eyedropper")
    }
}

/// Actions queued during a single frame pass before mutating workspace state.
enum PassAction {
    Apply { next: CameraRaw, label: Arc<str> },
    Live { next: CameraRaw },
    Commit { before: CameraRaw, after: CameraRaw, label: Arc<str> },
    Auto,
    AutoWb,
    ResetAll,
    ResetToOriginal,
    Demosaic(Demosaic),
    Step(Step),
    ShowFilmstrip(usize),
    Finish(RawFinish),
    Cancel,
    SaveSettings,
    LoadSettings,
}

fn target_accept(target: RawTarget) -> RawFinish {
    match target {
        RawTarget::OpenRaw => RawFinish::Open,
        RawTarget::Filter | RawTarget::SmartFilter | _ => RawFinish::Ok,
    }
}

#[derive(Pattern)]
pub(crate) struct AcrDevelopWorkspace {
    subject: String,
    size: PixelSize,
    target: RawTarget,
    settings: CameraRaw,
    stored: CameraRaw,
    original: CameraRaw,
    demosaic: Option<Demosaic>,
    drag_before: Option<CameraRaw>,
    history: EditHistory<CameraRaw>,
    filmstrip: Vec<RawFilmItem>,
    current: usize,
    navigation: NavigationOptions,
    checkerboard: Option<Checker>,
    view: FullWindowView,
    before: bool,
    hold_before: bool,
    view_mode: ViewMode,
    split_fraction: f32,
    show_panel: bool,
    wb_preset: WbPreset,
    tool: AcrTool,
    zoom_tool: Option<ZoomTool>,
    wb_picked: Option<[f64; 2]>,
    canvas: Option<Id>,
    hover: Option<[f64; 2]>,
    scope: Option<ScopePanelState>,
    scope_written: Option<ScopeSettings>,
    samplers: ColorSamplers,
    sampler_tool: ColorSamplerTool,
    diagnostic: Option<ClippingDiagnostic>,
    status_error: Option<Label>,
    last_done: Option<(Duration, Option<Label>)>,
    queued: Vec<AcrDevelopEvent>,
    requested: Option<RawFinish>,
    cancel_requested: bool,
}

impl AcrDevelopWorkspace {
    pub(crate) fn new(open: RawDevelopOpen) -> Self {
        Self {
            subject: open.subject,
            size: open.size,
            target: open.target,
            history: EditHistory::new(&open.settings),
            stored: open.settings.clone(),
            settings: open.settings,
            original: open.original,
            demosaic: open.demosaic,
            drag_before: None,
            filmstrip: open.filmstrip,
            current: open.current,
            navigation: open.navigation,
            checkerboard: open.checkerboard,
            view: FullWindowView::new(),
            before: false,
            hold_before: false,
            view_mode: ViewMode::Single,
            split_fraction: 0.5,
            show_panel: true,
            wb_preset: WbPreset::AsShot,
            tool: AcrTool::default(),
            zoom_tool: None,
            wb_picked: None,
            canvas: None,
            hover: None,
            scope: None,
            scope_written: None,
            samplers: ColorSamplers::new(),
            sampler_tool: ColorSamplerTool::new(),
            diagnostic: None,
            status_error: None,
            last_done: None,
            queued: Vec::new(),
            requested: None,
            cancel_requested: false,
        }
    }

    pub(crate) fn session(&self) -> &CameraRaw {
        &self.settings
    }

    pub(crate) fn load(&mut self, ctx: &Context, settings: CameraRaw) {
        let _ = ctx;
        self.apply(settings, Arc::from(tr("载入设置", "Load Settings")));
    }

    pub(crate) fn show_item(&mut self, current: usize, settings: CameraRaw, original: CameraRaw, demosaic: Option<Demosaic>) {
        self.history = EditHistory::new(&settings);
        self.stored = settings.clone();
        self.settings = settings;
        self.original = original;
        self.demosaic = demosaic;
        self.drag_before = None;
        self.current = current;
        self.diagnostic = None;
        self.status_error = None;
        self.wb_preset = WbPreset::AsShot;
    }

    pub(crate) fn set_item_picture(&mut self, index: usize, picture: egui::load::SizedTexture, edited: bool) {
        if let Some(item) = self.filmstrip.get_mut(index) {
            item.picture = Some(picture);
            item.edited = edited;
        }
    }

    /// Switches program view mode (Single / Split / Before); used by preview snapshots & tests.
    #[cfg(test)]
    pub(crate) fn set_view_mode_for_test(&mut self, mode: ViewMode, split_fraction: f32) {
        self.view_mode = mode;
        self.before = mode == ViewMode::Before;
        self.split_fraction = split_fraction.clamp(0.1, 0.9);
    }

    fn effective_before(&self) -> bool {
        self.before || self.hold_before || self.view_mode == ViewMode::Before
    }

    fn apply(&mut self, next: CameraRaw, name: Arc<str>) -> bool {
        self.drag_before = None;
        if next == self.settings {
            return false;
        }
        let event = EditEvent::set(Mixed::Same(self.settings.clone()), next.clone(), name, None);
        self.history.record(&event);
        self.settings = next;
        self.diagnostic = None;
        self.status_error = None;
        self.queued.push(AcrDevelopEvent::Settings(Box::new(event)));
        true
    }

    fn apply_commit(&mut self, before: CameraRaw, after: CameraRaw, name: Arc<str>) {
        self.drag_before = None;
        if before == after {
            return;
        }
        let event = EditEvent::set(Mixed::Same(before), after.clone(), name, None);
        self.history.record(&event);
        self.settings = after;
        self.diagnostic = None;
        self.status_error = None;
        self.queued.push(AcrDevelopEvent::Settings(Box::new(event)));
    }

    fn apply_live(&mut self, next: CameraRaw, diagnostic: Option<ClippingDiagnostic>) {
        if next == self.settings && self.diagnostic == diagnostic {
            return;
        }
        self.drag_before.get_or_insert_with(|| self.settings.clone());
        self.settings = next;
        self.diagnostic = diagnostic;
        self.status_error = None;
    }

    fn step(&mut self, step: Step, name: Arc<str>) -> Option<AcrDevelopEvent> {
        self.drag_before = None;
        let before = self.settings.clone();
        if !self.history.step(&mut self.settings, step) {
            return None;
        }
        self.diagnostic = None;
        let event = EditEvent::set(Mixed::Same(before), self.settings.clone(), name, None);
        Some(AcrDevelopEvent::Settings(Box::new(event)))
    }

    fn result(&self, finish: RawFinish) -> AcrDevelopResult {
        AcrDevelopResult { finish, settings: self.settings.clone(), demosaic: self.demosaic }
    }

    fn visible(&self) -> (PixelRect, f64) {
        let img = DocRect::new(0.0, 0.0, f64::from(self.size.width), f64::from(self.size.height));
        let part = if self.view.size().width > 0.0 { self.view.view().visible().intersect(img) } else { img };
        let rect = PixelRect::new(part.x0.floor() as i32, part.y0.floor() as i32, part.x1.ceil() as i32, part.y1.ceil() as i32);
        (rect, self.view.device_zoom())
    }

    fn end_gesture(&mut self, ctx: &Context, reason: CancelReason) {
        let Some(canvas) = self.canvas else { return };
        let size = [self.size.width, self.size.height];
        match self.tool {
            AcrTool::Hand => {}
            AcrTool::Eyedropper => {
                let mut wb = WbEyedropperTool::new(size);
                craft::ui::canvas::ToolHost::new().cancel(ctx, canvas, &mut wb, reason);
            }
            AcrTool::Zoom => {
                if let Some(zoom) = self.zoom_tool.as_mut() {
                    craft::ui::canvas::ToolHost::new().cancel(ctx, canvas, zoom, reason);
                }
            }
            AcrTool::Sampler => {
                let mut pass = self.sampler_tool.pass(&self.samplers, size);
                craft::ui::canvas::ToolHost::new().cancel(ctx, canvas, &mut pass, reason);
            }
        }
    }

    fn take_sampler_events(&mut self) {
        let size = [self.size.width, self.size.height];
        for event in self.sampler_tool.take_events() {
            let _ = match event {
                SamplerEvent::Add { at, .. } => self.samplers.add(at, size).map(|_| ()),
                SamplerEvent::Move { index, to, .. } => self.samplers.move_point(index, to, size),
                SamplerEvent::Remove { index, .. } => self.samplers.remove(index),
                _ => Ok(()),
            };
        }
    }

    fn open_scope(&mut self, ctx: &Context, source: &PcRawSource) {
        if self.scope.is_some() {
            return;
        }
        let saved: ScopeSettings = saved_value(ctx, &SCOPE_KEY);
        let mut settings = saved.clone();
        settings.vectorscope = false;
        settings.selected_region = settings.selected_region && source.has_selection();
        let mut state = ScopePanelState::default();
        state.settings = settings;
        self.scope = Some(state);
        self.scope_written = Some(saved);
    }

    fn save_scope(&mut self, ctx: &Context) {
        let Some(state) = &self.scope else { return };
        if self.scope_written.as_ref() != Some(&state.settings) {
            set_saved_value(ctx, &SCOPE_KEY, state.settings.clone());
            self.scope_written = Some(state.settings.clone());
        }
    }

    fn run_auto(&mut self, source: &PcRawSource, name: Arc<str>) {
        let next = source.auto(&self.settings);
        self.apply(next, name);
    }

    fn sample_linear_patch(source: &PcRawSource, at: [f64; 2]) -> Option<[f32; 3]> {
        if source.pw == 0 || source.ph == 0 || source.full_w == 0 || source.full_h == 0 {
            return None;
        }
        let fx = (at[0] / source.full_w as f64).clamp(0.0, 1.0);
        let fy = (at[1] / source.full_h as f64).clamp(0.0, 1.0);
        let cx = ((fx * source.pw as f64) as isize).clamp(0, source.pw as isize - 1);
        let cy = ((fy * source.ph as f64) as isize).clamp(0, source.ph as isize - 1);
        let gamma = source.inputs.encoding_gamma;
        let mut acc = [0.0f64; 3];
        let mut count = 0usize;
        for dy in -2isize..=2 {
            for dx in -2isize..=2 {
                let x = (cx + dx).clamp(0, source.pw as isize - 1) as usize;
                let y = (cy + dy).clamp(0, source.ph as isize - 1) as usize;
                if let Some(p) = source.proxy.get(y * source.pw + x)
                    && p[3] > 0.0
                    && p.iter().all(|v| v.is_finite())
                {
                    acc[0] += f64::from(decode_to_linear(p[0], gamma));
                    acc[1] += f64::from(decode_to_linear(p[1], gamma));
                    acc[2] += f64::from(decode_to_linear(p[2], gamma));
                    count += 1;
                }
            }
        }
        if count == 0 {
            return None;
        }
        Some([(acc[0] / count as f64) as f32, (acc[1] / count as f64) as f32, (acc[2] / count as f64) as f32])
    }

    fn run_auto_wb(source: &PcRawSource) -> Option<(f32, f32)> {
        let gamma = source.inputs.encoding_gamma;
        let mut acc = [0.0f64; 3];
        let mut count = 0usize;
        let step = (source.proxy.len() / 4096).max(1);
        for p in source.proxy.iter().step_by(step) {
            if p[3] > 0.5 && p.iter().all(|v| v.is_finite()) {
                let lr = decode_to_linear(p[0], gamma);
                let lg = decode_to_linear(p[1], gamma);
                let lb = decode_to_linear(p[2], gamma);
                let luma = 0.2126 * lr + 0.7152 * lg + 0.0722 * lb;
                if luma > 0.06 && luma < 0.88 {
                    acc[0] += f64::from(lr);
                    acc[1] += f64::from(lg);
                    acc[2] += f64::from(lb);
                    count += 1;
                }
            }
        }
        if count == 0 {
            return None;
        }
        compute_wb_from_rgb([(acc[0] / count as f64) as f32, (acc[1] / count as f64) as f32, (acc[2] / count as f64) as f32])
    }

    fn set_view(&mut self, value: &serde_json::Value) -> Result<(), String> {
        match value {
            serde_json::Value::String(text) if text == "fit" => {
                self.view.apply(ViewCommands::FitOnScreen);
                Ok(())
            }
            serde_json::Value::Number(percent) => match percent.as_f64().filter(|p| p.is_finite() && *p > 0.0) {
                Some(percent) => {
                    self.view.set_device_zoom(percent / 100.0);
                    Ok(())
                }
                None => Err(FieldRefusal::Rejects(value).message("view")),
            },
            _ => Err(FieldRefusal::Rejects(value).message("view")),
        }
    }
}

fn flag(field: &str, value: &serde_json::Value) -> Result<bool, String> {
    value.as_bool().ok_or_else(|| FieldRefusal::Rejects(value).message(field))
}

fn compute_sampler_readouts(source: &PcRawSource, samplers: &ColorSamplers, before: bool, lab: bool) -> Vec<SamplerReadout> {
    samplers
        .points()
        .iter()
        .enumerate()
        .map(|(i, &at)| {
            let [r, g, b] = match source.sample(at, before, false) {
                Some(ScopeReadout::Rgb(rgb)) => rgb.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8),
                _ => [0, 0, 0],
            };
            let lab_val = if lab {
                match source.sample(at, before, true) {
                    Some(ScopeReadout::Lab([l, a, bv])) => Some([
                        l.round().clamp(0.0, 100.0) as i16,
                        a.round().clamp(-128.0, 127.0) as i16,
                        bv.round().clamp(-128.0, 127.0) as i16,
                    ]),
                    _ => None,
                }
            } else {
                None
            };
            let mut ro = SamplerReadout::from_values(
                i + 1,
                [at[0].floor() as i64, at[1].floor() as i64],
                [r, g, b],
                [0, 0, 0, 0],
            );
            ro.lab = lab_val;
            ro
        })
        .collect()
}

fn readout_json(source: &dyn RawDevelopSource, at: [f64; 2], before: bool, lab: bool) -> serde_json::Value {
    let (space, values) = match source.sample(at, before, lab) {
        Some(ScopeReadout::Rgb(rgb)) => ("rgb", rgb.map(|v| v * 255.0)),
        Some(ScopeReadout::Lab(l)) => ("lab", l),
        _ => return serde_json::Value::Null,
    };
    serde_json::json!({ "position": at, "space": space, "values": values })
}

impl FullWindowWorkspace for AcrDevelopWorkspace {
    type Source = PcRawSource;
    type Event = AcrDevelopEvent;
    type Result = AcrDevelopResult;

    const KEY: &'static str = "filter.cameraRaw";

    fn show(&mut self, ui: &mut Ui, cx: WorkspaceCx<'_, Self::Source>) -> WorkspacePass<Self::Event, Self::Result> {
        let WorkspaceCx { source, revision, open, hold_ok, ok_waits, .. } = cx;
        let ctx = ui.ctx().clone();
        self.open_scope(&ctx, source);
        let c = AcrColors::from_ctx(&ctx);

        let title_head = if self.target == RawTarget::OpenRaw {
            tr("Camera Raw 显影", "Camera Raw")
        } else {
            tr("Camera Raw 滤镜", "Camera Raw Filter")
        };
        let title: Label = FullWindowTitle::new(Label::from(title_head))
            .subject(self.subject.clone())
            .zoom(self.view.zoom_percent())
            .label(&Env::of(ui));

        let settings_hash_key = settings_hash(&[format!("{:?}", (&self.settings, self.demosaic)).as_bytes()]);
        let can_step = (self.history.can_undo(), self.history.can_redo());
        let actions = RefCell::new(Vec::<PassAction>::new());
        let ws = RefCell::new(&mut *self);

        let shown = FullWindowDialog::new(title)
            .opened(open)
            .enter(EnterRule::Pass)
            .command_scope(RawDevelopCommands::SCOPE)
            .show(ui, |ui, cx| {
                let mut guard = ws.borrow_mut();
                let w = &mut **guard;

                // Framework history & command plumbing
                while let Some(step) = cx.take_step(can_step.0, can_step.1) {
                    w.end_gesture(&ctx, CancelReason::UndoDuringGesture);
                    actions.borrow_mut().push(PassAction::Step(step));
                }
                let mut view = w.view;
                while let Some((cmd, _)) = cx.take_command::<ViewCommands>() {
                    view.apply(cmd);
                }
                w.view = view;
                while let Some((cmd, fired)) = cx.take_command::<RawDevelopCommands>() {
                    match (cmd, fired.phase) {
                        (RawDevelopCommands::Before, FirePhase::Press) => w.before = true,
                        (RawDevelopCommands::Before, FirePhase::Release) => w.before = false,
                        (RawDevelopCommands::SamplerTool, FirePhase::Press) => {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Sampler;
                        }
                        (RawDevelopCommands::Hand, FirePhase::Press) => {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Hand;
                        }
                        (RawDevelopCommands::Zoom, FirePhase::Press) => {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Zoom;
                        }
                        _ => {}
                    }
                }

                // Single-key ACR shortcuts when not typing inside a text field
                if !ctx.egui_wants_keyboard_input() {
                    let (p_hit, q_hit, i_hit, e_hit, enter_hit) = ctx.input(|inp| {
                        let bare = !inp.modifiers.ctrl && !inp.modifiers.command && !inp.modifiers.alt;
                        (
                            bare && inp.key_pressed(egui::Key::P),
                            bare && inp.key_pressed(egui::Key::Q),
                            bare && inp.key_pressed(egui::Key::I),
                            bare && inp.key_pressed(egui::Key::E),
                            bare && !inp.modifiers.shift && inp.key_pressed(egui::Key::Enter),
                        )
                    });
                    if p_hit {
                        w.view_mode = if w.view_mode == ViewMode::Before { ViewMode::Single } else { ViewMode::Before };
                        w.before = w.view_mode == ViewMode::Before;
                    }
                    if q_hit {
                        w.view_mode = match w.view_mode {
                            ViewMode::Single => ViewMode::Split,
                            ViewMode::Split => ViewMode::Before,
                            ViewMode::Before => ViewMode::Single,
                        };
                        w.before = w.view_mode == ViewMode::Before;
                    }
                    if i_hit {
                        w.end_gesture(&ctx, CancelReason::Removed);
                        w.tool = AcrTool::Eyedropper;
                    }
                    if e_hit {
                        w.show_panel = !w.show_panel;
                    }
                    if enter_hit && !ok_waits {
                        actions.borrow_mut().push(PassAction::Finish(target_accept(w.target)));
                    }
                }

                // Trigger background preview develop
                let (visible, zoom) = w.visible();
                let key = (revision, settings_hash_key, format!("{visible:?}"), zoom.to_bits());
                let request = RawPreviewRequest::new(revision, w.settings.clone(), w.demosaic, visible, zoom);
                let status = cx.pending(key, || source.develop(&request)).clone();
                if let Pending::Ready(done) = &status {
                    w.last_done = Some((done.timing, done.backend.clone()));
                }
                let developing = matches!(status, Pending::Loading { .. }) || source.pending();

                let mut pending_pick = None;
                let mut hover = None;
                let mut canvas_id = None;

                Stack::new().fill().command_scope(ScopeCommand::SCOPE).show(ui, |ui, scope_cx| {
                    while let Some((cmd, _)) = scope_cx.take_command::<ScopeCommand>() {
                        if let Some(state) = w.scope.as_mut() {
                            match cmd {
                                ScopeCommand::ShadowWarning => state.settings.shadow_warning = !state.settings.shadow_warning,
                                ScopeCommand::HighlightWarning => state.settings.highlight_warning = !state.settings.highlight_warning,
                                _ => {}
                            }
                        }
                    }

                    let total_rect = ui.available_rect_before_wrap();
                    if total_rect.width() <= 1.0 || total_rect.height() <= 1.0 {
                        return;
                    }
                    let rail_w = 40.0;
                    let panel_w = if w.show_panel {
                        (total_rect.width() * 0.38).clamp(240.0, 326.0)
                    } else {
                        0.0
                    };
                    let right_total_w = rail_w + panel_w;
                    let bottom_h = 42.0;
                    let show_strip = w.filmstrip.len() > 1;
                    let strip_h = if show_strip { 74.0 } else { 0.0 };

                    let left_right_x = (total_rect.right() - right_total_w).max(total_rect.left() + 180.0);
                    let bottom_split_x = left_right_x.min((total_rect.right() - 280.0).max(total_rect.left() + 180.0));
                    let bottom_top_y = (total_rect.bottom() - bottom_h).max(total_rect.top() + 80.0);

                    let canvas_rect = Rect::from_min_max(total_rect.min, pos2(left_right_x, (bottom_top_y - strip_h).max(total_rect.top() + 40.0)));
                    let strip_rect = Rect::from_min_max(pos2(total_rect.left(), canvas_rect.bottom()), pos2(left_right_x, bottom_top_y));
                    let bottom_left_rect = Rect::from_min_max(pos2(total_rect.left(), bottom_top_y), pos2(bottom_split_x, total_rect.bottom()));

                    let panel_rect = Rect::from_min_max(pos2(left_right_x, total_rect.top()), pos2(total_rect.right() - rail_w, bottom_top_y));
                    let rail_rect = Rect::from_min_max(pos2(total_rect.right() - rail_w, total_rect.top()), pos2(total_rect.right(), bottom_top_y));
                    let bottom_right_rect = Rect::from_min_max(pos2(bottom_split_x, bottom_top_y), total_rect.max);

                    // Paint clean ACR region backdrops & crisp 1px separators
                    ui.painter().rect_filled(canvas_rect, CornerRadius::ZERO, c.bg_canvas);
                    if w.show_panel {
                        ui.painter().rect_filled(panel_rect, CornerRadius::ZERO, c.bg_panel);
                        ui.painter().line_segment([pos2(panel_rect.left(), panel_rect.top()), pos2(panel_rect.left(), bottom_top_y)], Stroke::new(1.0, c.border));
                    }
                    ui.painter().rect_filled(rail_rect, CornerRadius::ZERO, c.bg_rail);
                    ui.painter().line_segment([pos2(rail_rect.left(), rail_rect.top()), pos2(rail_rect.left(), rail_rect.bottom())], Stroke::new(1.0, c.border));
                    ui.painter().rect_filled(bottom_left_rect, CornerRadius::ZERO, c.bg_rail);
                    ui.painter().rect_filled(bottom_right_rect, CornerRadius::ZERO, c.bg_panel);
                    ui.painter().line_segment([pos2(total_rect.left(), bottom_top_y), pos2(total_rect.right(), bottom_top_y)], Stroke::new(1.0, c.border));
                    ui.painter().line_segment([pos2(bottom_split_x, bottom_top_y), pos2(bottom_split_x, total_rect.bottom())], Stroke::new(1.0, c.border));

                    // 1. Center-Left Main Preview Canvas
                    {
                        let mut c_ui = ui.new_child(egui::UiBuilder::new().max_rect(canvas_rect));
                        let eff_before = w.effective_before();
                        let state_settings = w.scope.as_ref().map(|s| s.settings.clone()).unwrap_or_default();
                        let developed = source.developed();
                        let mode = state_settings.clipping_mode(w.diagnostic);
                        let clipping_held: Option<(Arc<ClippingMask>, Revision)> = if matches!(mode, ClippingMode::None) {
                            None
                        } else {
                            source.clipping()
                        };

                        let settled_now = view.settled(canvas_rect, PixelSize::new(w.size.width, w.size.height), c_ui.pixels_per_point());
                        let split_screen_x = canvas_rect.left() + w.split_fraction.clamp(0.08, 0.92) * canvas_rect.width();
                        let split_doc_x = settled_now.view().to_doc(pos2(split_screen_x, canvas_rect.center().y)).x.round() as i32;
                        let split_src = SplitRaster::new(source.before(), source.after(), split_doc_x);
                        let raster: &dyn RasterSource = match w.view_mode {
                            ViewMode::Before => source.before(),
                            ViewMode::Single if eff_before => source.before(),
                            ViewMode::Single => source.after(),
                            ViewMode::Split if w.hold_before => source.before(),
                            ViewMode::Split => &split_src,
                        };

                        let readouts = if w.samplers.points().is_empty() {
                            None
                        } else {
                            Some(compute_sampler_readouts(source, &w.samplers, eff_before, state_settings.lab_readouts))
                        };

                        let carried = w.sampler_tool.carried();
                        let samplers = &w.samplers;
                        let controls = move |ui: &mut Ui, rect: Rect| {
                            if let Some((held, rev)) = &clipping_held {
                                ClippingOverlay::new(Label::from("Clipping"), held, *rev, mode).show_at(ui, rect);
                            }
                            let marks = ColorSamplerMarks::new(Label::from("Color samplers"), samplers);
                            let marks = match carried {
                                Some(carried) => marks.carried(carried),
                                None => marks,
                            };
                            marks.show_at(ui, rect);
                        };

                        let mut canvas = FullWindowCanvas::new(Bind::bound(&mut view), raster)
                            .navigation(w.navigation)
                            .controls(controls);
                        if let Some(checker) = w.checkerboard {
                            canvas = canvas.checkerboard(checker);
                        }

                        let doc_size = [w.size.width, w.size.height];
                        let out = match w.tool {
                            AcrTool::Hand => canvas.show(&mut c_ui),
                            AcrTool::Zoom => {
                                let z = w.zoom_tool.get_or_insert_with(|| ZoomTool::new(w.navigation));
                                z.options = w.navigation;
                                canvas.tool(z).show(&mut c_ui)
                            }
                            AcrTool::Sampler => {
                                let mut pass = w.sampler_tool.pass(&w.samplers, doc_size);
                                canvas.tool(&mut pass).show(&mut c_ui)
                            }
                            AcrTool::Eyedropper => {
                                let mut wb = WbEyedropperTool::new(doc_size);
                                let res = canvas.tool(&mut wb).show(&mut c_ui);
                                if let Some(pt) = wb.picked.take() {
                                    w.wb_picked = Some(pt);
                                }
                                res
                            }
                        };
                        canvas_id = Some(out.response.id);
                        hover = out.hover.map(|pt| [pt.x, pt.y]);

                        // Sampler readouts overlay at top-left if samplers exist
                        if let Some(readouts) = &readouts {
                            let badge_r = Rect::from_min_size(pos2(canvas_rect.left() + 10.0, canvas_rect.top() + 10.0), vec2(220.0, 22.0));
                            let mut txt = String::new();
                            for (i, ro) in readouts.iter().enumerate() {
                                if i > 0 {
                                    txt.push_str("   ");
                                }
                                match ro.lab {
                                    Some([l, a, b]) => txt.push_str(&format!("#{} L:{l} a:{a} b:{b}", ro.number)),
                                    None => txt.push_str(&format!("#{} R:{} G:{} B:{}", ro.number, ro.rgb[0], ro.rgb[1], ro.rgb[2])),
                                }
                            }
                            let g = ui.painter().layout_no_wrap(txt, theme::mono(10.0), c.text);
                            let bg = Rect::from_min_size(badge_r.min, g.size() + vec2(12.0, 6.0));
                            ui.painter().rect_filled(bg, CornerRadius::same(4), Color32::from_black_alpha(180));
                            ui.painter().galley(pos2(bg.left() + 6.0, bg.top() + 3.0), g, c.text);
                        }

                        // Interactive Split Compare divider & Before/After pills
                        if w.view_mode == ViewMode::Split && !w.hold_before {
                            let sx = canvas_rect.left() + w.split_fraction.clamp(0.08, 0.92) * canvas_rect.width();
                            ui.painter().line_segment(
                                [pos2(sx, canvas_rect.top()), pos2(sx, canvas_rect.bottom())],
                                Stroke::new(1.5, c.accent),
                            );
                            let handle_r = Rect::from_center_size(pos2(sx, canvas_rect.center().y), vec2(22.0, 32.0));
                            let h_resp = ui
                                .interact(handle_r, ui.scope_id().with("acr_split_handle"), Sense::click_and_drag())
                                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
                                .on_hover_text(tr("左右拖动分屏线对比调整前后", "Drag split line to compare Before / After"));
                            if h_resp.dragged()
                                && let Some(p) = h_resp.interact_pointer_pos()
                            {
                                w.split_fraction = ((p.x - canvas_rect.left()) / canvas_rect.width().max(1.0)).clamp(0.08, 0.92);
                            }
                            ui.painter().rect_filled(handle_r, CornerRadius::same(11), if h_resp.hovered() || h_resp.dragged() { c.accent } else { c.bg_rail });
                            ui.painter().rect_stroke(handle_r, CornerRadius::same(11), Stroke::new(1.2, c.accent), StrokeKind::Inside);
                            let fg = if h_resp.hovered() || h_resp.dragged() { c.primary_text } else { c.text };
                            ui.painter().text(handle_r.center(), Align2::CENTER_CENTER, "‹ ›", theme::semibold(11.0), fg);

                            for (lbl, x_pos, align) in [
                                (tr("调整前", "Before"), canvas_rect.left() + 10.0, Align2::LEFT_TOP),
                                (tr("调整后", "After"), canvas_rect.right() - 10.0, Align2::RIGHT_TOP),
                            ] {
                                let g = ui.painter().layout_no_wrap(lbl.to_string(), theme::medium(10.5), c.text);
                                let r = align.anchor_size(pos2(x_pos, canvas_rect.top() + 10.0), g.size() + vec2(12.0, 6.0));
                                ui.painter().rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(165));
                                ui.painter().galley(pos2(r.left() + 6.0, r.top() + 3.0), g, c.text);
                            }
                        } else if w.hold_before || w.view_mode == ViewMode::Before {
                            let lbl = tr("原图 (调整前)", "Before");
                            let g = ui.painter().layout_no_wrap(lbl.to_string(), theme::medium(10.5), c.accent);
                            let r = Rect::from_min_size(pos2(canvas_rect.left() + 10.0, canvas_rect.top() + 10.0), g.size() + vec2(12.0, 6.0));
                            ui.painter().rect_filled(r, CornerRadius::same(4), Color32::from_black_alpha(180));
                            ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, c.accent_border), StrokeKind::Inside);
                            ui.painter().galley(pos2(r.left() + 6.0, r.top() + 3.0), g, c.accent);
                        }

                        // Resolve WB eyedropper click now that source is available
                        if let Some(doc_pt) = w.wb_picked.take()
                            && let Some(lin) = Self::sample_linear_patch(source, doc_pt)
                            && let Some((temp, tint)) = compute_wb_from_rgb(lin)
                        {
                            let mut next = w.settings.clone();
                            next.temperature = temp;
                            next.tint = tint;
                            w.wb_preset = WbPreset::Custom;
                            actions.borrow_mut().push(PassAction::Apply {
                                next,
                                label: Arc::from(tr("白平衡吸管", "White Balance Eyedropper")),
                            });
                        }

                        let _ = developed;
                    }

                    // 2. Optional Horizontal Filmstrip
                    if show_strip {
                        let mut s_ui = ui.new_child(egui::UiBuilder::new().max_rect(strip_rect));
                        if let Some(pick) = draw_filmstrip(&mut s_ui, &w.filmstrip, w.current, &c) {
                            pending_pick = Some(pick);
                        }
                    }

                    // 3. Bottom-Left Canvas Toolbar (Zoom | View Mode | Hold Compare | Warnings | Status)
                    {
                        let mut b_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(bottom_left_rect.shrink2(vec2(10.0, 4.0)))
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        );
                        b_ui.spacing_mut().item_spacing.x = 6.0;

                        // Zoom stepper + preset ComboBox
                        let z_label = if view.is_fit() {
                            tr("适合视图", "Fit").to_string()
                        } else {
                            format!("{:.0}%", view.zoom_percent())
                        };
                        if acr_pill_button(&mut b_ui, vec2(24.0, 26.0), "−", false, view.can_step(-1), &c).clicked() {
                            view.apply(ViewCommands::ZoomOut);
                        }
                        egui::ComboBox::from_id_salt("acr_zoom_combo")
                            .width(78.0)
                            .selected_text(RichText::new(z_label).font(theme::medium(11.0)))
                            .show_ui(&mut b_ui, |ui| {
                                if ui.selectable_label(view.is_fit(), tr("适合视图 (Ctrl+0)", "Fit in View")).clicked() {
                                    view.apply(ViewCommands::FitOnScreen);
                                }
                                for pct in [25.0, 50.0, 100.0, 200.0, 400.0] {
                                    let is_cur = !view.is_fit() && (view.zoom_percent() - pct).abs() < 0.6;
                                    if ui.selectable_label(is_cur, format!("{pct:.0}%")).clicked() {
                                        view.set_device_zoom(pct / 100.0);
                                    }
                                }
                            });
                        if acr_pill_button(&mut b_ui, vec2(24.0, 26.0), "+", false, view.can_step(1), &c).clicked() {
                            view.apply(ViewCommands::ZoomIn);
                        }
                        if acr_pill_button(&mut b_ui, vec2(36.0, 26.0), "1:1", !view.is_fit() && (view.zoom_percent() - 100.0).abs() < 0.6, true, &c).clicked() {
                            view.set_device_zoom(1.0);
                        }

                        b_ui.add_space(6.0);

                        // View mode segmented buttons (单图 | 分屏 | 原图)
                        for (mode, lbl) in [
                            (ViewMode::Single, tr("单图", "Single")),
                            (ViewMode::Split, tr("分屏对比", "Split")),
                            (ViewMode::Before, tr("原图", "Before")),
                        ] {
                            let active = w.view_mode == mode;
                            if acr_pill_button(&mut b_ui, vec2(58.0, 26.0), lbl, active, true, &c).clicked() {
                                w.view_mode = mode;
                                w.before = mode == ViewMode::Before;
                            }
                        }

                        // Momentary press-and-hold compare button
                        let hold_resp = acr_pill_button(&mut b_ui, vec2(66.0, 26.0), tr("按住原图", "Hold Orig"), w.hold_before, true, &c)
                            .on_hover_text(tr("按住对比调整前原图 (快捷键 Y)", "Hold to view Before (Y)"));
                        w.hold_before = hold_resp.is_pointer_button_down_on();

                        // Right-aligned diagnostics + render telemetry inside bottom-left bar
                        b_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(err) = &w.status_error {
                                ui.label(RichText::new(err.as_str()).font(theme::medium(10.5)).color(c.danger));
                            } else if developing {
                                ui.label(RichText::new(tr("正在显影…", "Developing…")).font(theme::medium(10.5)).color(c.accent));
                            } else if let Some((dur, backend)) = &w.last_done {
                                let ms = dur.as_millis();
                                let eng = backend.as_ref().map_or("CPU", |b| b.as_str());
                                let txt = format!("{eng} · {ms} ms");
                                ui.label(RichText::new(txt).font(theme::mono(10.0)).color(c.text_dim));
                            }

                            if let Some(state) = w.scope.as_mut() {
                                if acr_pill_button(ui, vec2(26.0, 24.0), "O", state.settings.highlight_warning, true, &c)
                                    .on_hover_text(tr("高光剪贴警告 (O)", "Highlight Clipping (O)"))
                                    .clicked()
                                {
                                    state.settings.highlight_warning = !state.settings.highlight_warning;
                                }
                                if acr_pill_button(ui, vec2(26.0, 24.0), "U", state.settings.shadow_warning, true, &c)
                                    .on_hover_text(tr("阴影剪贴警告 (U)", "Shadow Clipping (U)"))
                                    .clicked()
                                {
                                    state.settings.shadow_warning = !state.settings.shadow_warning;
                                }
                            }
                        });
                    }

                    // 4. Right Parameter Column (Histogram + Action Bar + Accordion Sections)
                    if w.show_panel {
                        let p_inner = panel_rect.shrink2(vec2(10.0, 8.0));
                        let mut p_ui = ui.new_child(egui::UiBuilder::new().max_rect(p_inner));
                        let eff_before = w.effective_before();
                        let developed = source.developed();

                        // Histogram + Clipping Triangles + Tone Zone Drag
                        let hist_ref = if eff_before {
                            Some(&source.before_histogram)
                        } else {
                            developed.as_ref().map(|d| &d.after_histogram)
                        };
                        let channels_opt = hist_ref.map(|h| [&h.channels[0], &h.channels[1], &h.channels[2]]);
                        let shadows_c = hist_ref.map_or([0, 0, 0], |h| h.shadows);
                        let highlights_c = hist_ref.map_or([0, 0, 0], |h| h.highlights);

                        if let Some(state) = w.scope.as_mut() {
                            if let Some(HistogramDragOutcome { edit, field, diagnostic }) =
                                draw_acr_histogram(&mut p_ui, channels_opt, shadows_c, highlights_c, developing, &mut state.settings, &w.settings, &c)
                                && let Some(ev) = edit
                            {
                                let apply_val = |s: &mut CameraRaw, val: f32| match field {
                                    "blacks" => s.blacks = val,
                                    "shadows" => s.shadows = val,
                                    "exposure" => s.exposure = val,
                                    "highlights" => s.highlights = val,
                                    "whites" => s.whites = val,
                                    _ => {}
                                };
                                match ev {
                                    SliderEdit::Live(v) => {
                                        let mut next = w.settings.clone();
                                        apply_val(&mut next, v);
                                        w.apply_live(next, diagnostic);
                                    }
                                    SliderEdit::Commit { before: b, after: a } | SliderEdit::Immediate { before: b, after: a } => {
                                        let mut sb = w.settings.clone();
                                        apply_val(&mut sb, b);
                                        let mut sa = w.settings.clone();
                                        apply_val(&mut sa, a);
                                        actions.borrow_mut().push(PassAction::Commit {
                                            before: sb,
                                            after: sa,
                                            label: Arc::from(tr("色调区域", "Tone Zone")),
                                        });
                                    }
                                }
                            }
                        }

                        // Cursor / Dimensions Readout Strip
                        let hover_readout = w.hover.and_then(|at| {
                            let lab = w.scope.as_ref().is_some_and(|s| s.settings.lab_readouts);
                            source.sample(at, eff_before, lab)
                        });
                        draw_readout_strip(&mut p_ui, hover_readout, [w.size.width as usize, w.size.height as usize], &c);

                        // Optional Vectorscope Plot when enabled from `···` menu
                        if let Some(state) = w.scope.as_ref()
                            && state.settings.vectorscope
                            && let Some(dev) = &developed
                        {
                            let vs = dev.scope.vectorscope(eff_before, state.settings.selected_region);
                            let vf = VectorscopeFrame::new(&vs.counts, HUES, SATURATIONS, None);
                            VectorscopePlot::new(tr("矢量示波器", "Vectorscope"), &vf)
                                .skin_tone(state.settings.skin_tone)
                                .red_right(state.settings.red_right)
                                .show(&mut p_ui);
                            p_ui.add_space(4.0);
                        }

                        // Header bar: 编辑 + [自动] [复位] + Undo/Redo
                        p_ui.add_space(4.0);
                        p_ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(RichText::new(tr("编辑", "Edit")).font(theme::semibold(12.5)).color(c.text));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if tool_rail_button(ui, 22.0, ToolIcon::Redo, false, can_step.1, tr("重做 (Ctrl+Shift+Z)", "Redo"), &c).clicked() {
                                    actions.borrow_mut().push(PassAction::Step(Step::Redo));
                                }
                                if tool_rail_button(ui, 22.0, ToolIcon::Undo, false, can_step.0, tr("撤销 (Ctrl+Z)", "Undo"), &c).clicked() {
                                    actions.borrow_mut().push(PassAction::Step(Step::Undo));
                                }
                                let can_reset = !w.settings.is_identity();
                                if acr_pill_button(ui, vec2(44.0, 22.0), tr("复位", "Reset"), false, can_reset, &c).clicked() {
                                    actions.borrow_mut().push(PassAction::ResetAll);
                                }
                                if acr_pill_button(ui, vec2(44.0, 22.0), tr("自动", "Auto"), false, true, &c).clicked() {
                                    actions.borrow_mut().push(PassAction::Auto);
                                }
                            });
                        });
                        p_ui.add_space(4.0);

                        // Scrollable ACR Accordion Sections
                        egui::ScrollArea::vertical()
                            .id_salt("acr-right-sections")
                            .auto_shrink([false, false])
                            .show(&mut p_ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;

                                // Section 1: 基本 (Basic = WB + Tone + Presence)
                                let basic_mod = w.settings.temperature != 0.0
                                    || w.settings.tint != 0.0
                                    || w.settings.exposure != 0.0
                                    || w.settings.contrast != 0.0
                                    || w.settings.highlights != 0.0
                                    || w.settings.shadows != 0.0
                                    || w.settings.whites != 0.0
                                    || w.settings.blacks != 0.0
                                    || w.settings.texture != 0.0
                                    || w.settings.clarity != 0.0
                                    || w.settings.dehaze != 0.0
                                    || w.settings.vibrance != 0.0
                                    || w.settings.saturation != 0.0;

                                if acr_section(ui, "basic", tr("基本", "Basic"), basic_mod, true, &c, |ui| {
                                    // White Balance preset ComboBox + Eyedropper button
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 6.0;
                                        ui.label(RichText::new(tr("白平衡", "White Balance")).font(theme::medium(11.0)).color(c.text_dim));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let eye_active = w.tool == AcrTool::Eyedropper;
                                            if tool_rail_button(
                                                ui,
                                                22.0,
                                                ToolIcon::Eyedropper,
                                                eye_active,
                                                true,
                                                tr("白平衡吸管工具 (I)：点击画面中性灰采样", "White Balance Eyedropper (I)"),
                                                &c,
                                            )
                                            .clicked()
                                            {
                                                w.end_gesture(&ctx, CancelReason::Removed);
                                                w.tool = if eye_active { AcrTool::Hand } else { AcrTool::Eyedropper };
                                            }
                                            egui::ComboBox::from_id_salt("acr_wb_preset")
                                                .width(106.0)
                                                .selected_text(RichText::new(w.wb_preset.label()).font(theme::medium(11.0)))
                                                .show_ui(ui, |ui| {
                                                    for preset in [
                                                        WbPreset::AsShot,
                                                        WbPreset::Auto,
                                                        WbPreset::Daylight,
                                                        WbPreset::Cloudy,
                                                        WbPreset::Shade,
                                                        WbPreset::Tungsten,
                                                        WbPreset::Fluorescent,
                                                        WbPreset::Custom,
                                                    ] {
                                                        if ui.selectable_label(w.wb_preset == preset, preset.label()).clicked() {
                                                            w.wb_preset = preset;
                                                            if preset == WbPreset::Auto {
                                                                actions.borrow_mut().push(PassAction::AutoWb);
                                                            } else if let Some((t, tn)) = preset.fixed_values() {
                                                                let mut next = w.settings.clone();
                                                                next.temperature = t;
                                                                next.tint = tn;
                                                                actions.borrow_mut().push(PassAction::Apply {
                                                                    next,
                                                                    label: Arc::from(tr("白平衡预设", "White Balance Preset")),
                                                                });
                                                            }
                                                        }
                                                    }
                                                });
                                        });
                                    });
                                    ui.add_space(2.0);

                                    let temp_stops = [Color32::from_rgb(0x46, 0x82, 0xff), Color32::from_rgb(0xd8, 0xd8, 0xd6), Color32::from_rgb(0xf5, 0xc2, 0x30)];
                                    let tint_stops = [Color32::from_rgb(0x4c, 0xaf, 0x50), Color32::from_rgb(0xd8, 0xd8, 0xd6), Color32::from_rgb(0xd0, 0x40, 0xb0)];
                                    let sat_stops = [Color32::from_rgb(108, 108, 106), Color32::from_rgb(200, 128, 86), Color32::from_rgb(235, 76, 66)];

                                    for (key, lbl, val, min, max, step, prec, stops, set_fn, is_wb, diag_opt) in [
                                        ("temp", tr("色温", "Temperature"), w.settings.temperature, -100.0, 100.0, 1.0, 0, Some(&temp_stops[..]), (|s: &mut CameraRaw, v: f32| s.temperature = v) as fn(&mut CameraRaw, f32), true, None),
                                        ("tint", tr("色调", "Tint"), w.settings.tint, -100.0, 100.0, 1.0, 0, Some(&tint_stops[..]), |s, v| s.tint = v, true, None),
                                        ("exp", tr("曝光", "Exposure"), w.settings.exposure, -5.0, 5.0, 0.05, 2, None, |s, v| s.exposure = v, false, Some(ClippingDiagnostic::Highlights)),
                                        ("con", tr("对比度", "Contrast"), w.settings.contrast, -100.0, 100.0, 1.0, 0, None, |s, v| s.contrast = v, false, None),
                                        ("hi", tr("高光", "Highlights"), w.settings.highlights, -100.0, 100.0, 1.0, 0, None, |s, v| s.highlights = v, false, None),
                                        ("sh", tr("阴影", "Shadows"), w.settings.shadows, -100.0, 100.0, 1.0, 0, None, |s, v| s.shadows = v, false, None),
                                        ("wh", tr("白色色阶", "Whites"), w.settings.whites, -100.0, 100.0, 1.0, 0, None, |s, v| s.whites = v, false, Some(ClippingDiagnostic::Highlights)),
                                        ("bk", tr("黑色色阶", "Blacks"), w.settings.blacks, -100.0, 100.0, 1.0, 0, None, |s, v| s.blacks = v, false, Some(ClippingDiagnostic::Shadows)),
                                        ("tex", tr("纹理", "Texture"), w.settings.texture, -100.0, 100.0, 1.0, 0, None, |s, v| s.texture = v, false, None),
                                        ("clr", tr("清晰度", "Clarity"), w.settings.clarity, -100.0, 100.0, 1.0, 0, None, |s, v| s.clarity = v, false, None),
                                        ("dhz", tr("去除薄雾", "Dehaze"), w.settings.dehaze, -100.0, 100.0, 1.0, 0, None, |s, v| s.dehaze = v, false, None),
                                        ("vib", tr("自然饱和度", "Vibrance"), w.settings.vibrance, -100.0, 100.0, 1.0, 0, Some(&sat_stops[..]), |s, v| s.vibrance = v, false, None),
                                        ("sat", tr("饱和度", "Saturation"), w.settings.saturation, -100.0, 100.0, 1.0, 0, Some(&sat_stops[..]), |s, v| s.saturation = v, false, None),
                                    ] {
                                        if key == "exp" || key == "tex" {
                                            ui.add_space(3.0);
                                            let sub = if key == "exp" { tr("影调与曝光", "Tone") } else { tr("偏好 · 质感与色彩", "Presence") };
                                            ui.label(RichText::new(sub).font(theme::medium(10.0)).color(c.text_faint));
                                            ui.add_space(1.0);
                                        }
                                        let (edit, dragging) = acr_slider_row(ui, key, lbl, val, 0.0, min..=max, step, prec, stops, &c);
                                        if let Some(ev) = edit {
                                            if is_wb {
                                                w.wb_preset = WbPreset::Custom;
                                            }
                                            match ev {
                                                SliderEdit::Live(v) => {
                                                    let mut next = w.settings.clone();
                                                    set_fn(&mut next, v);
                                                    let alt = dragging && ui.input(|i| i.modifiers.alt);
                                                    w.apply_live(next, alt.then_some(diag_opt).flatten());
                                                }
                                                SliderEdit::Commit { before: b, after: a } | SliderEdit::Immediate { before: b, after: a } => {
                                                    let mut sb = w.settings.clone();
                                                    set_fn(&mut sb, b);
                                                    let mut sa = w.settings.clone();
                                                    set_fn(&mut sa, a);
                                                    actions.borrow_mut().push(PassAction::Commit {
                                                        before: sb,
                                                        after: sa,
                                                        label: Arc::from(lbl),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }) {
                                    let mut next = w.settings.clone();
                                    next.temperature = 0.0;
                                    next.tint = 0.0;
                                    next.exposure = 0.0;
                                    next.contrast = 0.0;
                                    next.highlights = 0.0;
                                    next.shadows = 0.0;
                                    next.whites = 0.0;
                                    next.blacks = 0.0;
                                    next.texture = 0.0;
                                    next.clarity = 0.0;
                                    next.dehaze = 0.0;
                                    next.vibrance = 0.0;
                                    next.saturation = 0.0;
                                    w.wb_preset = WbPreset::AsShot;
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位基本面板", "Reset Basic")) });
                                }

                                // Section 2: 曲线 (Curve)
                                let curve_mod = w.settings.curve_highlights != 0.0
                                    || w.settings.curve_lights != 0.0
                                    || w.settings.curve_darks != 0.0
                                    || w.settings.curve_shadows != 0.0
                                    || w.settings.curve_splits != [25.0, 50.0, 75.0]
                                    || !w.settings.point_curve.is_empty()
                                    || !w.settings.point_curve_red.is_empty()
                                    || !w.settings.point_curve_green.is_empty()
                                    || !w.settings.point_curve_blue.is_empty();
                                if acr_section(ui, "curve", tr("曲线", "Curve"), curve_mod, false, &c, |ui| {
                                    if let Some((next, lbl, live)) = draw_curve_panel(ui, &w.settings, channels_opt, &c) {
                                        if live {
                                            actions.borrow_mut().push(PassAction::Live { next });
                                        } else {
                                            actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(lbl) });
                                        }
                                    }
                                }) {
                                    let mut next = w.settings.clone();
                                    next.curve_highlights = 0.0;
                                    next.curve_lights = 0.0;
                                    next.curve_darks = 0.0;
                                    next.curve_shadows = 0.0;
                                    next.curve_splits = [25.0, 50.0, 75.0];
                                    next.point_curve.clear();
                                    next.point_curve_red.clear();
                                    next.point_curve_green.clear();
                                    next.point_curve_blue.clear();
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位曲线", "Reset Curve")) });
                                }

                                // Section 3: 细节 (Detail)
                                let detail_mod = w.settings.sharpen_amount != 0.0
                                    || (w.settings.sharpen_radius - 1.0).abs() > 1e-3
                                    || (w.settings.sharpen_detail - 25.0).abs() > 1e-3
                                    || w.settings.sharpen_masking != 0.0
                                    || w.settings.noise_luminance != 0.0
                                    || (w.settings.noise_luminance_detail - 50.0).abs() > 1e-3
                                    || w.settings.noise_color != 0.0
                                    || (w.settings.noise_color_detail - 50.0).abs() > 1e-3;
                                if acr_section(ui, "detail", tr("细节", "Detail"), detail_mod, false, &c, |ui| {
                                    if let Some(cur_demosaic) = w.demosaic {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(tr("去马赛克", "Demosaic")).font(theme::medium(11.0)).color(c.text_dim));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                let lbl = match cur_demosaic {
                                                    Demosaic::Ahd => "AHD",
                                                    Demosaic::Mhc => "MHC",
                                                    Demosaic::Bilinear => tr("双线性", "Bilinear"),
                                                    _ => "AHD",
                                                };
                                                egui::ComboBox::from_id_salt("acr_demosaic")
                                                    .width(106.0)
                                                    .selected_text(RichText::new(lbl).font(theme::medium(11.0)))
                                                    .show_ui(ui, |ui| {
                                                        for (d, dl) in [(Demosaic::Ahd, "AHD"), (Demosaic::Mhc, "MHC"), (Demosaic::Bilinear, tr("双线性", "Bilinear"))] {
                                                            if ui.selectable_label(cur_demosaic == d, dl).clicked() {
                                                                actions.borrow_mut().push(PassAction::Demosaic(d));
                                                            }
                                                        }
                                                    });
                                            });
                                        });
                                        ui.add_space(4.0);
                                    }

                                    ui.label(RichText::new(tr("锐化", "Sharpening")).font(theme::medium(10.0)).color(c.text_faint));
                                    for (key, lbl, val, def, min, max, step, prec, set_fn) in [
                                        ("shp_amt", tr("数量", "Amount"), w.settings.sharpen_amount, 0.0, 0.0, 150.0, 1.0, 0, (|s: &mut CameraRaw, v: f32| s.sharpen_amount = v) as fn(&mut CameraRaw, f32)),
                                        ("shp_rad", tr("半径", "Radius"), w.settings.sharpen_radius, 1.0, 0.5, 3.0, 0.1, 1, |s, v| s.sharpen_radius = v),
                                        ("shp_det", tr("细节", "Detail"), w.settings.sharpen_detail, 25.0, 0.0, 100.0, 1.0, 0, |s, v| s.sharpen_detail = v),
                                        ("shp_msk", tr("蒙版", "Masking"), w.settings.sharpen_masking, 0.0, 0.0, 100.0, 1.0, 0, |s, v| s.sharpen_masking = v),
                                        ("nr_lum", tr("减少杂色 (明度)", "Noise Reduction"), w.settings.noise_luminance, 0.0, 0.0, 100.0, 1.0, 0, |s, v| s.noise_luminance = v),
                                        ("nr_ld", tr("明度细节", "Luminance Detail"), w.settings.noise_luminance_detail, 50.0, 0.0, 100.0, 1.0, 0, |s, v| s.noise_luminance_detail = v),
                                        ("nr_col", tr("减少杂色 (颜色)", "Color Noise Reduction"), w.settings.noise_color, 0.0, 0.0, 100.0, 1.0, 0, |s, v| s.noise_color = v),
                                        ("nr_cd", tr("颜色细节", "Color Detail"), w.settings.noise_color_detail, 50.0, 0.0, 100.0, 1.0, 0, |s, v| s.noise_color_detail = v),
                                    ] {
                                        if key == "nr_lum" {
                                            ui.add_space(4.0);
                                            ui.label(RichText::new(tr("减少杂色", "Noise Reduction")).font(theme::medium(10.0)).color(c.text_faint));
                                        }
                                        let (edit, _) = acr_slider_row(ui, key, lbl, val, def, min..=max, step, prec, None, &c);
                                        if let Some(ev) = edit {
                                            match ev {
                                                SliderEdit::Live(v) => {
                                                    let mut next = w.settings.clone();
                                                    set_fn(&mut next, v);
                                                    actions.borrow_mut().push(PassAction::Live { next });
                                                }
                                                SliderEdit::Commit { before: b, after: a } | SliderEdit::Immediate { before: b, after: a } => {
                                                    let mut sb = w.settings.clone();
                                                    set_fn(&mut sb, b);
                                                    let mut sa = w.settings.clone();
                                                    set_fn(&mut sa, a);
                                                    actions.borrow_mut().push(PassAction::Commit { before: sb, after: sa, label: Arc::from(lbl) });
                                                }
                                            }
                                        }
                                    }
                                }) {
                                    let mut next = w.settings.clone();
                                    next.sharpen_amount = 0.0;
                                    next.sharpen_radius = 1.0;
                                    next.sharpen_detail = 25.0;
                                    next.sharpen_masking = 0.0;
                                    next.noise_luminance = 0.0;
                                    next.noise_luminance_detail = 50.0;
                                    next.noise_color = 0.0;
                                    next.noise_color_detail = 50.0;
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位细节", "Reset Detail")) });
                                }

                                // Section 4: 混色器 (Color Mixer)
                                let mixer_mod = w.settings.hsl_hue != [0.0; 8] || w.settings.hsl_sat != [0.0; 8] || w.settings.hsl_lum != [0.0; 8];
                                if acr_section(ui, "mixer", tr("混色器", "Color Mixer"), mixer_mod, false, &c, |ui| {
                                    if let Some((next, lbl, live)) = draw_color_mixer_panel(ui, &w.settings, &c) {
                                        if live {
                                            actions.borrow_mut().push(PassAction::Live { next });
                                        } else {
                                            actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(lbl) });
                                        }
                                    }
                                }) {
                                    let mut next = w.settings.clone();
                                    next.hsl_hue = [0.0; 8];
                                    next.hsl_sat = [0.0; 8];
                                    next.hsl_lum = [0.0; 8];
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位混色器", "Reset Color Mixer")) });
                                }

                                // Section 5: 颜色分级 (Color Grading)
                                let grade_def = CameraRaw::default();
                                let grade_mod = w.settings.grade_shadows != grade_def.grade_shadows
                                    || w.settings.grade_midtones != grade_def.grade_midtones
                                    || w.settings.grade_highlights != grade_def.grade_highlights
                                    || w.settings.grade_global != grade_def.grade_global
                                    || (w.settings.grade_blending - 50.0).abs() > 1e-3
                                    || w.settings.grade_balance != 0.0;
                                if acr_section(ui, "grading", tr("颜色分级", "Color Grading"), grade_mod, false, &c, |ui| {
                                    if let Some((next, lbl, live)) = draw_color_grading_panel(ui, &w.settings, &c) {
                                        if live {
                                            actions.borrow_mut().push(PassAction::Live { next });
                                        } else {
                                            actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(lbl) });
                                        }
                                    }
                                }) {
                                    let mut next = w.settings.clone();
                                    next.grade_shadows = grade_def.grade_shadows;
                                    next.grade_midtones = grade_def.grade_midtones;
                                    next.grade_highlights = grade_def.grade_highlights;
                                    next.grade_global = grade_def.grade_global;
                                    next.grade_blending = 50.0;
                                    next.grade_balance = 0.0;
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位颜色分级", "Reset Color Grading")) });
                                }

                                // Section 6: 效果 (Effects: Grain + Vignette)
                                let effects_mod = w.settings.grain_amount != 0.0
                                    || (w.settings.grain_size - 25.0).abs() > 1e-3
                                    || (w.settings.grain_roughness - 50.0).abs() > 1e-3
                                    || w.settings.vignette_amount != 0.0
                                    || (w.settings.vignette_midpoint - 50.0).abs() > 1e-3
                                    || w.settings.vignette_roundness != 0.0
                                    || (w.settings.vignette_feather - 50.0).abs() > 1e-3
                                    || w.settings.vignette_highlights != 0.0
                                    || w.settings.vignette_style != VignetteStyle::HighlightPriority;
                                if acr_section(ui, "effects", tr("效果", "Effects"), effects_mod, false, &c, |ui| {
                                    ui.label(RichText::new(tr("颗粒与暗角", "Grain & Vignette")).font(theme::medium(10.0)).color(c.text_faint));
                                    for (key, lbl, val, def, min, max, set_fn) in [
                                        ("gr_amt", tr("颗粒", "Grain"), w.settings.grain_amount, 0.0, 0.0, 100.0, (|s: &mut CameraRaw, v: f32| s.grain_amount = v) as fn(&mut CameraRaw, f32)),
                                        ("gr_sz", tr("大小", "Size"), w.settings.grain_size, 25.0, 0.0, 100.0, |s, v| s.grain_size = v),
                                        ("gr_rg", tr("粗糙度", "Roughness"), w.settings.grain_roughness, 50.0, 0.0, 100.0, |s, v| s.grain_roughness = v),
                                        ("vig_amt", tr("去除薄雾暗角 / 晕影", "Vignetting"), w.settings.vignette_amount, 0.0, -100.0, 100.0, |s, v| s.vignette_amount = v),
                                        ("vig_mid", tr("中点", "Midpoint"), w.settings.vignette_midpoint, 50.0, 0.0, 100.0, |s, v| s.vignette_midpoint = v),
                                        ("vig_rnd", tr("圆度", "Roundness"), w.settings.vignette_roundness, 0.0, -100.0, 100.0, |s, v| s.vignette_roundness = v),
                                        ("vig_fth", tr("羽化", "Feather"), w.settings.vignette_feather, 50.0, 0.0, 100.0, |s, v| s.vignette_feather = v),
                                        ("vig_hi", tr("高光", "Highlights"), w.settings.vignette_highlights, 0.0, 0.0, 100.0, |s, v| s.vignette_highlights = v),
                                    ] {
                                        let (edit, _) = acr_slider_row(ui, key, lbl, val, def, min..=max, 1.0, 0, None, &c);
                                        if let Some(ev) = edit {
                                            match ev {
                                                SliderEdit::Live(v) => {
                                                    let mut next = w.settings.clone();
                                                    set_fn(&mut next, v);
                                                    actions.borrow_mut().push(PassAction::Live { next });
                                                }
                                                SliderEdit::Commit { before: b, after: a } | SliderEdit::Immediate { before: b, after: a } => {
                                                    let mut sb = w.settings.clone();
                                                    set_fn(&mut sb, b);
                                                    let mut sa = w.settings.clone();
                                                    set_fn(&mut sa, a);
                                                    actions.borrow_mut().push(PassAction::Commit { before: sb, after: sa, label: Arc::from(lbl) });
                                                }
                                            }
                                        }
                                    }
                                    ui.add_space(2.0);
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(tr("暗角样式", "Style")).font(theme::medium(11.0)).color(c.text_dim));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let slbl = match w.settings.vignette_style {
                                                VignetteStyle::HighlightPriority => tr("高光优先", "Highlight Priority"),
                                                VignetteStyle::ColorPriority => tr("颜色优先", "Color Priority"),
                                                VignetteStyle::PaintOverlay => tr("绘画叠加", "Paint Overlay"),
                                            };
                                            egui::ComboBox::from_id_salt("acr_vig_style")
                                                .width(120.0)
                                                .selected_text(RichText::new(slbl).font(theme::medium(11.0)))
                                                .show_ui(ui, |ui| {
                                                    for (st, stl) in [
                                                        (VignetteStyle::HighlightPriority, tr("高光优先", "Highlight Priority")),
                                                        (VignetteStyle::ColorPriority, tr("颜色优先", "Color Priority")),
                                                        (VignetteStyle::PaintOverlay, tr("绘画叠加", "Paint Overlay")),
                                                    ] {
                                                        if ui.selectable_label(w.settings.vignette_style == st, stl).clicked() {
                                                            let mut next = w.settings.clone();
                                                            next.vignette_style = st;
                                                            actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("暗角样式", "Vignette Style")) });
                                                        }
                                                    }
                                                });
                                        });
                                    });
                                }) {
                                    let mut next = w.settings.clone();
                                    next.grain_amount = 0.0;
                                    next.grain_size = 25.0;
                                    next.grain_roughness = 50.0;
                                    next.vignette_amount = 0.0;
                                    next.vignette_midpoint = 50.0;
                                    next.vignette_roundness = 0.0;
                                    next.vignette_feather = 50.0;
                                    next.vignette_highlights = 0.0;
                                    next.vignette_style = VignetteStyle::HighlightPriority;
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位效果", "Reset Effects")) });
                                }

                                // Section 7: 校准 (Calibration)
                                let cal_mod = w.settings.shadow_tint != 0.0 || w.settings.calibration_hue != [0.0; 3] || w.settings.calibration_sat != [0.0; 3];
                                if acr_section(ui, "calibration", tr("校准", "Calibration"), cal_mod, false, &c, |ui| {
                                    let cal_tint_stops = [Color32::from_rgb(0x4c, 0xaf, 0x50), Color32::from_rgb(0xd8, 0xd8, 0xd6), Color32::from_rgb(0xd0, 0x40, 0xb0)];
                                    let (ed_st, _) = acr_slider_row(ui, "cal_sh_tint", tr("阴影色调", "Shadows Tint"), w.settings.shadow_tint, 0.0, -100.0..=100.0, 1.0, 0, Some(&cal_tint_stops), &c);
                                    if let Some(ev) = ed_st {
                                        let (live, before_v, after_v) = match ev {
                                            SliderEdit::Live(v) => (true, v, v),
                                            SliderEdit::Commit { before, after } | SliderEdit::Immediate { before, after } => (false, before, after),
                                        };
                                        if live {
                                            let mut next = w.settings.clone();
                                            next.shadow_tint = after_v;
                                            actions.borrow_mut().push(PassAction::Live { next });
                                        } else {
                                            let mut sb = w.settings.clone();
                                            sb.shadow_tint = before_v;
                                            let mut sa = w.settings.clone();
                                            sa.shadow_tint = after_v;
                                            actions.borrow_mut().push(PassAction::Commit { before: sb, after: sa, label: Arc::from(tr("阴影色调", "Shadows Tint")) });
                                        }
                                    }

                                    let primaries = [
                                        (0usize, tr("红原色", "Red Primary"), Color32::from_rgb(228, 72, 68)),
                                        (1usize, tr("绿原色", "Green Primary"), Color32::from_rgb(72, 198, 86)),
                                        (2usize, tr("蓝原色", "Blue Primary"), Color32::from_rgb(78, 128, 245)),
                                    ];
                                    for (pi, p_lbl, p_col) in primaries {
                                        ui.add_space(3.0);
                                        ui.label(RichText::new(p_lbl).font(theme::medium(10.0)).color(p_col));
                                        let h_lbl = tr("色相", "Hue");
                                        let s_lbl = tr("饱和度", "Saturation");
                                        let (eh, _) = acr_slider_row(ui, &format!("cal_h_{pi}"), h_lbl, w.settings.calibration_hue[pi], 0.0, -100.0..=100.0, 1.0, 0, None, &c);
                                        if let Some(ev) = eh {
                                            let (live, before_v, after_v) = match ev {
                                                SliderEdit::Live(v) => (true, v, v),
                                                SliderEdit::Commit { before, after } | SliderEdit::Immediate { before, after } => (false, before, after),
                                            };
                                            if live {
                                                let mut next = w.settings.clone();
                                                next.calibration_hue[pi] = after_v;
                                                actions.borrow_mut().push(PassAction::Live { next });
                                            } else {
                                                let mut sb = w.settings.clone();
                                                sb.calibration_hue[pi] = before_v;
                                                let mut sa = w.settings.clone();
                                                sa.calibration_hue[pi] = after_v;
                                                actions.borrow_mut().push(PassAction::Commit { before: sb, after: sa, label: Arc::from(p_lbl) });
                                            }
                                        }
                                        let (es, _) = acr_slider_row(ui, &format!("cal_s_{pi}"), s_lbl, w.settings.calibration_sat[pi], 0.0, -100.0..=100.0, 1.0, 0, None, &c);
                                        if let Some(ev) = es {
                                            let (live, before_v, after_v) = match ev {
                                                SliderEdit::Live(v) => (true, v, v),
                                                SliderEdit::Commit { before, after } | SliderEdit::Immediate { before, after } => (false, before, after),
                                            };
                                            if live {
                                                let mut next = w.settings.clone();
                                                next.calibration_sat[pi] = after_v;
                                                actions.borrow_mut().push(PassAction::Live { next });
                                            } else {
                                                let mut sb = w.settings.clone();
                                                sb.calibration_sat[pi] = before_v;
                                                let mut sa = w.settings.clone();
                                                sa.calibration_sat[pi] = after_v;
                                                actions.borrow_mut().push(PassAction::Commit { before: sb, after: sa, label: Arc::from(p_lbl) });
                                            }
                                        }
                                    }
                                }) {
                                    let mut next = w.settings.clone();
                                    next.shadow_tint = 0.0;
                                    next.calibration_hue = [0.0; 3];
                                    next.calibration_sat = [0.0; 3];
                                    actions.borrow_mut().push(PassAction::Apply { next, label: Arc::from(tr("复位校准", "Reset Calibration")) });
                                }
                            });
                    }

                    // 5. Far-Right Vertical ACR Tool Rail (40px)
                    {
                        let r_inner = rail_rect.shrink2(vec2(4.0, 8.0));
                        let mut rail_ui = ui.new_child(egui::UiBuilder::new().max_rect(r_inner).layout(egui::Layout::top_down(egui::Align::Center)));
                        rail_ui.spacing_mut().item_spacing.y = 6.0;

                        if tool_rail_button(&mut rail_ui, 30.0, ToolIcon::Edit, w.show_panel, true, tr("展开/收起编辑面板 (E)", "Edit Panel (E)"), &c).clicked() {
                            w.show_panel = !w.show_panel;
                        }
                        if tool_rail_button(&mut rail_ui, 30.0, ToolIcon::Eyedropper, w.tool == AcrTool::Eyedropper, true, tr("白平衡工具 (I)", "White Balance Tool (I)"), &c).clicked() {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Eyedropper;
                        }
                        if tool_rail_button(&mut rail_ui, 30.0, ToolIcon::Hand, w.tool == AcrTool::Hand, true, tr("抓手工具 (H / 空格)", "Hand Tool (H)"), &c).clicked() {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Hand;
                        }
                        if tool_rail_button(&mut rail_ui, 30.0, ToolIcon::Zoom, w.tool == AcrTool::Zoom, true, tr("缩放工具 (Z)", "Zoom Tool (Z)"), &c).clicked() {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Zoom;
                        }
                        if tool_rail_button(&mut rail_ui, 30.0, ToolIcon::Sampler, w.tool == AcrTool::Sampler, true, tr("颜色取样器工具 (S)", "Color Sampler Tool (S)"), &c).clicked() {
                            w.end_gesture(&ctx, CancelReason::Removed);
                            w.tool = AcrTool::Sampler;
                        }

                        // Subtle divider
                        let (div_r, _) = rail_ui.allocate_exact_size(vec2(24.0, 6.0), Sense::hover());
                        rail_ui.painter().line_segment([pos2(div_r.left(), div_r.center().y), pos2(div_r.right(), div_r.center().y)], Stroke::new(1.0, c.border));

                        if tool_rail_button(
                            &mut rail_ui,
                            30.0,
                            ToolIcon::SplitCompare,
                            w.view_mode == ViewMode::Split,
                            true,
                            tr("循环切换单图/分屏对比 (Q)", "Cycle Views / Split (Q)"),
                            &c,
                        )
                        .clicked()
                        {
                            w.view_mode = if w.view_mode == ViewMode::Split { ViewMode::Single } else { ViewMode::Split };
                            w.before = w.view_mode == ViewMode::Before;
                        }
                        if tool_rail_button(
                            &mut rail_ui,
                            30.0,
                            ToolIcon::BeforeAfter,
                            w.view_mode == ViewMode::Before,
                            true,
                            tr("切换调整前/后视图 (P)", "Toggle Before / After (P)"),
                            &c,
                        )
                        .clicked()
                        {
                            w.view_mode = if w.view_mode == ViewMode::Before { ViewMode::Single } else { ViewMode::Before };
                            w.before = w.view_mode == ViewMode::Before;
                        }

                        if tool_rail_button(
                            &mut rail_ui,
                            30.0,
                            ToolIcon::Reset,
                            false,
                            !w.settings.is_identity(),
                            tr("复位为默认设置", "Reset to Defaults"),
                            &c,
                        )
                        .clicked()
                        {
                            actions.borrow_mut().push(PassAction::ResetAll);
                        }

                        // `···` More Options menu (Save/Load Settings, Scope options)
                        let more_resp = tool_rail_button(
                            &mut rail_ui,
                            30.0,
                            ToolIcon::More,
                            false,
                            true,
                            tr("更多选项与 XMP 设置", "More Settings & XMP"),
                            &c,
                        );
                        egui::Popup::menu(&more_resp).show(|ui| {
                            if ui.button(tr("存储设置… (.xmp)", "Save Settings…")).clicked() {
                                actions.borrow_mut().push(PassAction::SaveSettings);
                                ui.close();
                            }
                            if ui.button(tr("载入设置… (.xmp)", "Load Settings…")).clicked() {
                                actions.borrow_mut().push(PassAction::LoadSettings);
                                ui.close();
                            }
                            ui.separator();
                            if ui.button(tr("复位为默认设置", "Reset to Defaults")).clicked() {
                                actions.borrow_mut().push(PassAction::ResetAll);
                                ui.close();
                            }
                            if ui.button(tr("复位为打开时设置", "Reset to Open Settings")).clicked() {
                                actions.borrow_mut().push(PassAction::ResetToOriginal);
                                ui.close();
                            }
                            ui.separator();
                            if let Some(state) = w.scope.as_mut() {
                                if ui.checkbox(&mut state.settings.highlight_warning, tr("高光溢出警告 (O)", "Highlight Clipping (O)")).changed() {
                                    ui.close();
                                }
                                if ui.checkbox(&mut state.settings.shadow_warning, tr("阴影溢出警告 (U)", "Shadow Clipping (U)")).changed() {
                                    ui.close();
                                }
                                if ui.checkbox(&mut state.settings.lab_readouts, tr("显示 Lab 读数", "Show Lab Readouts")).changed() {
                                    ui.close();
                                }
                                if ui.checkbox(&mut state.settings.vectorscope, tr("显示矢量示波器", "Show Vectorscope")).changed() {
                                    ui.close();
                                }
                            }
                        });
                    }

                    // 6. Bottom-Right Action Bar ([取消] [完成] [打开] or [取消] [确定])
                    {
                        let mut br_ui = ui.new_child(
                            egui::UiBuilder::new()
                                .max_rect(bottom_right_rect.shrink2(vec2(12.0, 6.0)))
                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                        );
                        br_ui.spacing_mut().item_spacing.x = 8.0;

                        let accept_label = match w.target {
                            RawTarget::OpenRaw => tr("打开", "Open"),
                            RawTarget::Filter | RawTarget::SmartFilter | _ => tr("确定", "OK"),
                        };
                        if acr_primary_button(&mut br_ui, vec2(84.0, 28.0), accept_label, !ok_waits, &c).clicked() {
                            actions.borrow_mut().push(PassAction::Finish(target_accept(w.target)));
                        }
                        if w.target == RawTarget::OpenRaw
                            && acr_pill_button(&mut br_ui, vec2(74.0, 28.0), tr("完成", "Done"), false, !ok_waits, &c)
                                .on_hover_text(tr("保存设置到 XMP 并关闭，不打开文档", "Save settings and close without opening"))
                                .clicked()
                        {
                            actions.borrow_mut().push(PassAction::Finish(RawFinish::Done));
                        }
                        if acr_pill_button(&mut br_ui, vec2(74.0, 28.0), tr("取消", "Cancel"), false, true, &c).clicked() {
                            actions.borrow_mut().push(PassAction::Cancel);
                        }
                    }
                });

                w.view = view;
                w.canvas = canvas_id;
                w.hover = hover;
                w.take_sampler_events();
                if let Some(pick) = pending_pick {
                    actions.borrow_mut().push(PassAction::ShowFilmstrip(pick));
                }
            });

        let output = shown.output;
        let mut events = std::mem::take(&mut self.queued);
        for action in actions.into_inner() {
            match action {
                PassAction::Apply { next, label } => {
                    if let Some(before) = self.drag_before.take() {
                        self.apply_commit(before, next, label);
                    } else {
                        self.apply(next, label);
                    }
                }
                PassAction::Live { next } => {
                    self.apply_live(next, self.diagnostic);
                }
                PassAction::Commit { before, after, label } => {
                    let before = self.drag_before.take().unwrap_or(before);
                    self.apply_commit(before, after, label);
                }
                PassAction::Auto => self.run_auto(source, Arc::from(tr("自动", "Auto"))),
                PassAction::AutoWb => {
                    if let Some((temp, tint)) = Self::run_auto_wb(source) {
                        let mut next = self.settings.clone();
                        next.temperature = temp;
                        next.tint = tint;
                        self.apply(next, Arc::from(tr("自动白平衡", "Auto White Balance")));
                    }
                }
                PassAction::ResetAll => {
                    self.wb_preset = WbPreset::AsShot;
                    self.apply(CameraRaw::default(), Arc::from(tr("复位全部参数", "Reset Defaults")));
                }
                PassAction::ResetToOriginal => {
                    self.wb_preset = WbPreset::AsShot;
                    self.apply(self.original.clone(), Arc::from(tr("复位初始设置", "Reset Original")));
                }
                PassAction::Demosaic(d) => {
                    self.demosaic = Some(d);
                }
                PassAction::Step(step) => {
                    if let Some(ev) = self.step(step, Arc::from(tr("RAW 显影", "RAW Develop"))) {
                        events.push(ev);
                    }
                }
                PassAction::ShowFilmstrip(idx) => events.push(AcrDevelopEvent::Show(idx)),
                PassAction::Finish(finish) => self.requested = Some(finish),
                PassAction::Cancel => self.cancel_requested = true,
                PassAction::SaveSettings => events.push(AcrDevelopEvent::SaveSettings),
                PassAction::LoadSettings => events.push(AcrDevelopEvent::LoadSettings),
            }
        }
        events.append(&mut self.queued);
        self.save_scope(&ctx);

        let (result, held) = if self.cancel_requested || output.closed.is_some() {
            self.cancel_requested = false;
            (Some(DialogResult::Cancel), None)
        } else {
            match (self.requested.take(), hold_ok) {
                (Some(finish), true) => (None, Some(self.result(finish))),
                (Some(finish), false) => (Some(DialogResult::Ok(self.result(finish))), None),
                (None, _) => (None, None),
            }
        };

        WorkspacePass::new(output.response, events).held(held).result(result).canvas(self.canvas)
    }

    fn json_fields(&self, source: &Self::Source) -> Option<serde_json::Value> {
        let mut fields = serde_fields(&self.settings)?;
        if let serde_json::Value::Object(map) = &mut fields {
            let before = self.effective_before();
            map.insert("before".to_owned(), serde_json::Value::Bool(before));
            map.insert("demosaic".to_owned(), serde_json::to_value(self.demosaic).unwrap_or(serde_json::Value::Null));
            let view = if self.view.is_fit() {
                serde_json::Value::from("fit")
            } else {
                serde_json::Value::from(self.view.zoom_percent())
            };
            map.insert("view".to_owned(), view);
            let scope = self.scope.as_ref().map(|state| state.settings.clone()).unwrap_or_default();
            let lab = scope.lab_readouts;
            map.insert("scope".to_owned(), serde_json::to_value(scope).unwrap_or(serde_json::Value::Null));
            let read = |at: [f64; 2]| readout_json(source, at, before, lab);
            map.insert("pointerReadout".to_owned(), self.hover.map_or(serde_json::Value::Null, read));
            map.insert("samplerReadouts".to_owned(), self.samplers.points().iter().copied().map(read).collect());
        }
        Some(fields)
    }

    fn set_json_field(&mut self, ctx: &Context, source: &Self::Source, field: &str, value: serde_json::Value) -> Result<(), String> {
        match field {
            "auto" => {
                if flag(field, &value)? {
                    self.run_auto(source, Arc::from(tr("自动", "Auto")));
                }
                Ok(())
            }
            "undo" | "redo" => {
                if flag(field, &value)? {
                    let step = if field == "undo" { Step::Undo } else { Step::Redo };
                    self.end_gesture(ctx, CancelReason::UndoDuringGesture);
                    let events = self.step(step, Arc::from(tr("RAW 显影", "RAW Develop")));
                    self.queued.extend(events);
                }
                Ok(())
            }
            "done" => {
                let on = flag(field, &value)?;
                if self.target != RawTarget::OpenRaw {
                    return Err(FieldRefusal::Inactive.message(field));
                }
                if on {
                    self.requested = Some(RawFinish::Done);
                }
                Ok(())
            }
            "saveSettings" | "loadSettings" => {
                if flag(field, &value)? {
                    self.queued.push(if field == "saveSettings" {
                        AcrDevelopEvent::SaveSettings
                    } else {
                        AcrDevelopEvent::LoadSettings
                    });
                }
                Ok(())
            }
            "before" => {
                let on = flag(field, &value)?;
                self.before = on;
                self.view_mode = if on {
                    ViewMode::Before
                } else if self.view_mode == ViewMode::Before {
                    ViewMode::Single
                } else {
                    self.view_mode
                };
                Ok(())
            }
            "demosaic" => {
                if self.demosaic.is_none() {
                    return Err(FieldRefusal::Inactive.message(field));
                }
                self.demosaic = Some(serde_json::from_value(value.clone()).map_err(|_| FieldRefusal::Rejects(&value).message(field))?);
                Ok(())
            }
            "view" => self.set_view(&value),
            "scope" => {
                let settings: ScopeSettings = serde_json::from_value(value.clone()).map_err(|_| FieldRefusal::Rejects(&value).message(field))?;
                self.scope.get_or_insert_with(ScopePanelState::default).settings = settings;
                self.save_scope(ctx);
                Ok(())
            }
            _ => {
                let mut next = self.settings.clone();
                set_serde_field(&mut next, field, value)?;
                next.validate_changes(&self.settings)?;
                self.apply(next, Arc::from(field));
                Ok(())
            }
        }
    }

    fn confirm(&mut self, ctx: &Context, source: &Self::Source) -> Option<Self::Result> {
        let _ = (ctx, source);
        self.settings.validate_changes(&self.stored).ok()?;
        Some(self.result(target_accept(self.target)))
    }

    fn ok_failed(&mut self, reason: Label) {
        self.status_error = Some(reason);
    }
}
