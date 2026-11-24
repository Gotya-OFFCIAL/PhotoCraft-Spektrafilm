//! The options bar: the framework's `Bar` with the Home button, the active tool's icon and the members of the active
//! tool. The tools' own members live with their tools; this file orders them and hosts the bar's commit and cancel
//! buttons.

use std::ops::RangeInclusive;

use craft::behavior::{EditEvents, NumberCodec};
use craft::ui::bars::{Bar, BarHome, BarKind, BarOutput, BarTool};
use craft::ui::choice::Checkbox;
use craft::ui::contract::{Bind, Entry, Props as _, Show as _, ShowContainer as _};
use craft::ui::display::Divider;
use craft::ui::form::Field;
use craft::ui::menu::Menu;
use craft::ui::runtime::env::{Env, Label, WheelScope, with_env};
use craft::ui::text::NumberField;
use photocraft_paint::BrushSettings;

use super::{PhotocraftApp, Tool, Value, icons, json};
use paint::PaintMode;

pub(crate) mod fill;
mod navigate;
mod paint;
pub(crate) mod selection;

/// The right-click menu of the tool icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Reset {
    /// Reset Tool.
    Tool,
    /// Reset All Tools.
    All,
}

impl Reset {
    /// The menu command the row runs.
    fn command(self) -> &'static str {
        match self {
            Reset::Tool => "tool.reset",
            Reset::All => "tool.resetAll",
        }
    }
}

/// What the bar's commit and cancel buttons end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum End {
    /// The Transform Symmetry session.
    Symmetry,
    /// The live crop.
    Crop,
    /// The Perspective Crop's pending crop.
    PerspectiveCrop,
    /// The Remove or Content-Aware Move edit that waits for an answer.
    Retouch,
    /// The text being typed.
    Type,
}

/// The commit and cancel buttons of the bar and what they end.
struct Ending {
    end: End,
    commit: Label,
    cancel: Label,
}

/// Whether `tool` is one of the four type tools.
fn is_type_tool(tool: Tool) -> bool {
    matches!(tool, Tool::Type | Tool::VerticalType | Tool::HorizontalTypeMask | Tool::VerticalTypeMask)
}

/// The edit the bar's commit and cancel buttons belong to, if one is waiting. A bar carries at most one pair.
fn ending(app: &PhotocraftApp, tool: Tool) -> Option<Ending> {
    if app.ui.transform.is_some() {
        return None;
    }
    let pair = |end, commit: &str, cancel: &str| Some(Ending { end, commit: commit.to_owned().into(), cancel: cancel.to_owned().into() });
    if crate::symmetry_ui::is_open(app) {
        return pair(End::Symmetry, tl!("Commit"), tl!("Cancel"));
    }
    match tool {
        Tool::Crop if crate::crop_ui::bar::commits_shown(app) => pair(End::Crop, tl!("Commit current crop operation"), tl!("Cancel current crop operation")),
        Tool::PerspectiveCrop if crate::perspective_crop_ui::commit_shown(app) => pair(End::PerspectiveCrop, tl!("Commit"), tl!("Cancel")),
        tool if is_type_tool(tool) && app.ui.text_edit.is_some() => pair(End::Type, tl!("Commit any current edits"), tl!("Cancel any current edits")),
        tool => crate::retouch_ui::bar::pending(app, tool).map(|p| Ending { end: End::Retouch, commit: p.commit, cancel: p.cancel }),
    }
}

/// The options bar of the active tool. It is the same arrangement in every theme.
///
/// Each pass applies the commit or cancel its own bar took; a second pass of a frame applies none of the first pass's.
pub fn options_bar(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    options_bar_with_brand(app, ui, crate::panels::brand::is_spektra());
}

pub(crate) fn options_bar_with_brand(app: &mut PhotocraftApp, ui: &mut egui::Ui, is_spektra: bool) {
    let tool = app.ui.tool;
    let ending = ending(app, tool);
    let show_spektra = is_spektra && app.session.active().is_some();
    let active_job = show_spektra && app.session.active_job().is_some();
    let spektra_enabled = show_spektra && !active_job && crate::menus::is_enabled(app, crate::spektra_ui::COMMAND);
    let mut open_spektra = false;
    let mut bar = Bar::new(tl!("Options").to_owned(), BarKind::Options);
    if show_spektra {
        bar = bar.trailing(|ui, _| {
            if spektra_darkroom_pill(ui, spektra_enabled, active_job) {
                open_spektra = true;
            }
        });
    }
    if let Some(ending) = &ending {
        bar = bar.commit_cancel(ending.commit.clone(), ending.cancel.clone());
    }
    // A value control of the bar takes the wheel without a click, as the Mode dropdown and the fields did.
    let shown = bar.show(ui, |ui, _| with_env(ui, |set| set.wheel = Some(WheelScope::Free), |ui| body(app, ui)));
    if let Some(ending) = ending {
        answer(app, ui, tool, ending.end, &shown.output);
    }
    if open_spektra
        && !craft::ui::runtime::measure::active(ui.ctx())
        && let Err(e) = crate::menus::invoke(app, ui.ctx(), crate::spektra_ui::COMMAND, json!({}))
    {
        app.ui.status = e;
    }
}

/// Crisp capsule entry point at the far right of the options bar for SpektraFilm Darkroom Studio.
/// Returns `true` when clicked while enabled.
fn spektra_darkroom_pill(ui: &mut egui::Ui, enabled: bool, active_job: bool) -> bool {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (fg, bg_normal, bg_hover, border) = if !enabled {
        (t.text_faint, t.field, t.field, t.separator)
    } else if t.dark() {
        let a = t.accent;
        (
            a,
            egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 22),
            egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 46),
            egui::Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 180),
        )
    } else {
        (t.accent_text, t.accent_soft, t.hover, t.accent_border)
    };
    let pill_label = "✦ SpektraFilm 胶片暗房";
    let font = crate::theme::semibold(11.5);
    let galley = ui.painter().layout_no_wrap(pill_label.into(), font.clone(), fg);
    let row_h = ui.spacing().interact_size.y.max(22.0);
    let pill_w = galley.size().x + 18.0;
    let sense = if enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let (alloc_rect, mut resp) = ui.allocate_exact_size(egui::vec2(pill_w, row_h), sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, pill_label));
    if enabled {
        resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let tip = if enabled {
        "打开 SpektraFilm 81 波长光谱胶片与暗房印相工作台 (Filter › SpektraFilm › Film Simulation…)"
    } else if active_job {
        "后台处理进行中，完成后即可进入 SpektraFilm 胶片暗房"
    } else {
        "请先选中像素图层或智能对象图层以启用光谱胶片暗房"
    };
    resp = resp.on_hover_text(tip);
    let pill_rect = egui::Rect::from_center_size(alloc_rect.center(), egui::vec2(pill_w, 22.0));
    let bg = if enabled && resp.hovered() { bg_hover } else { bg_normal };
    ui.painter().rect_filled(pill_rect, egui::CornerRadius::same(99), bg);
    ui.painter().rect_stroke(pill_rect, egui::CornerRadius::same(99), egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
    ui.painter().text(pill_rect.center(), egui::Align2::CENTER_CENTER, pill_label, font, fg);
    enabled && resp.clicked()
}

/// Runs the ending the bar's commit or cancel button asked for.
fn answer(app: &mut PhotocraftApp, ui: &egui::Ui, tool: Tool, end: End, out: &BarOutput) {
    match end {
        End::Symmetry => {
            if out.committed {
                crate::symmetry_ui::commit(app);
            } else if out.cancelled {
                crate::symmetry_ui::cancel(app);
            }
        }
        End::Crop => {
            if out.committed || out.cancelled {
                crate::crop_ui::bar::committed(app, out.committed);
            }
        }
        End::PerspectiveCrop => crate::perspective_crop_ui::committed(app, out),
        End::Retouch => crate::retouch_ui::bar::answer(app, tool, out.committed, out.cancelled),
        End::Type => {
            if out.committed {
                let mods = ui.input(|i| i.modifiers);
                crate::type_tool::commit_with(app, mods);
            } else if out.cancelled {
                crate::type_tool::cancel(app);
            }
        }
    }
}

/// The bar's members in order: Home, the tool icon, then the active tool's options.
fn body(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    lead(app, ui);
    if app.ui.transform.is_some() {
        crate::transform_tool::options_bar(app, ui);
        return;
    }
    if crate::symmetry_ui::is_open(app) {
        crate::symmetry_ui::bar_members(ui);
        return;
    }
    if crate::distort_ui::options_bar(app, ui) {
        return;
    }
    let tool = app.ui.tool;
    // The bar shows the active tool's brush: switch it in before drawing, so a tool change swaps the brush with it
    // (Photoshop keeps one per tool).
    crate::paint_mouse::sync_tool_brush(app);
    if (tool.is_brushlike() && !matches!(tool, Tool::Brush | Tool::Pencil | Tool::MixerBrush | Tool::Eraser)) || tool == Tool::QuickSelection {
        crate::brush_picker::show_field(ui, app, true);
        crate::brush_picker::settings_toggle(ui, app);
        Divider::vertical().show(ui);
    }
    if matches!(tool, Tool::CloneStamp | Tool::PatternStamp | Tool::HistoryBrush | Tool::Healing | Tool::ArtHistoryBrush) {
        stamp_members(app, ui, tool);
        Divider::vertical().show(ui);
    }
    if crate::eraser_ui::options_bar(ui, app, tool)
        || crate::retouch_ui::bar::options_bar(ui, app, tool)
        || crate::vector_ui::options_bar(app, ui, tool)
        || crate::analysis::bars::options(app, ui, tool)
        || crate::slice_ui::options_bar(ui, app, tool)
        || crate::frame_tool::options_bar(app, ui, tool)
        || crate::artboard_tool::options_bar(app, ui, tool)
    {
        return;
    }
    match tool {
        Tool::Crop => crate::crop_ui::bar::bar(app, ui),
        Tool::PerspectiveCrop => crate::perspective_crop_ui::options(app, ui),
        Tool::Gradient => crate::gradient_ui::bar(app, ui),
        tool if is_type_tool(tool) => crate::type_tool::options_bar(app, ui),
        _ => {}
    }
    let _ = paint::members(app, ui, tool) || selection::members(app, ui, tool);
    fill::members(app, ui);
    navigate::members(app, ui);
}

/// The Home button and the tool's icon with its Reset Tool and Reset All Tools menu.
fn lead(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let documents = app.session.documents().len();
    let auto = app.session.prefs().general.auto_show_home_screen;
    let on = app.ui.chrome.shows_home(documents, auto);
    // With no documents and auto-show on, Home can't be dismissed (there is nothing behind it).
    let home = BarHome::new(tl!("Home").to_owned(), Bind::given(on)).enabled(documents > 0 || !auto).show(ui);
    if home.events.value().is_some() {
        app.ui.chrome.home = if on { None } else { Some(documents) };
    }
    // The tool picker is greyed while warping, as in the reference.
    let warping = app.ui.transform.as_ref().is_some_and(|t| t.session.warp().is_some());
    let tool = BarTool::new(icons::tool_icon(app.ui.tool), tl!(app.ui.tool.label()).to_owned()).enabled(!warping).show(ui);
    // Photoshop: right-click the tool icon for Reset Tool / Reset All Tools.
    let rows = [Entry::new(Reset::Tool, tl!("Reset Tool").to_owned()), Entry::new(Reset::All, tl!("Reset All Tools").to_owned())];
    if let Some(reset) = Menu::context(tl!("Tool").to_owned(), &tool.response).pick(ui, &rows).picked
        && let Err(e) = crate::menus::invoke(app, ui.ctx(), reset.command(), json!({}))
    {
        app.ui.status = e;
    }
}

/// The Mode, Opacity and Flow of the stamp tools, Healing and Art History: the tool's own brush's, kept per tool like its
/// size. Art History has no Mode (the engine paints Normal) and no Flow.
fn stamp_members(app: &mut PhotocraftApp, ui: &mut egui::Ui, tool: Tool) {
    let brush = app.session.tools.brush.clone();
    // An Indexed Color document names the Clone Stamp's and the Pattern Stamp's Normal "Threshold"
    // (#12613, #12887); the other entries weren't measured there.
    let indexed = app.session.active().is_some_and(|st| st.doc.mode == photocraft_doc::ColorMode::Indexed);
    let threshold = crate::retouch_ui::clone_stamp_indexed(app) || (tool == Tool::PatternStamp && indexed);
    // Pattern Stamp's list has Behind, greyed on the Background.
    let entries = fill::mode_entries(tool, crate::brush_mode_menu::enabled(app, crate::brush_mode_menu::Item::Behind), threshold);
    if !entries.is_empty() {
        let current =
            if tool == Tool::PatternStamp && brush.behind { PaintMode::Behind } else { PaintMode::Blend(crate::blend_keys::brush_tool_mode(tool, brush.mode)) };
        let picked = labelled(ui, tl!("Mode"), |ui| fill::mode_select(Bind::given(current), &entries).show(ui));
        if let Some(mode) = picked.and_then(|shown| shown.events.value().cloned()) {
            let mut after = brush.clone();
            match mode {
                PaintMode::Blend(blend) => {
                    after.mode = blend;
                    if tool == Tool::PatternStamp {
                        after.behind = false;
                    }
                }
                PaintMode::Behind => after.behind = true,
                PaintMode::Clear => {}
            }
            // A pick is one step of its own.
            crate::brush_panel::commit_keyed(app, &brush, &after, None);
        }
    }
    // The Pattern Stamp's Opacity and Flow are whole percents with the Brush's tooltips (#12616).
    if tool == Tool::PatternStamp {
        let opacity = whole_percent_field(ui, tl!("Opacity"), brush.opacity, tl!("Set opacity for stroke"));
        edit_brush(app, &opacity, |b, v| b.opacity = v);
        let flow = whole_percent_field(ui, tl!("Flow"), brush.flow, tl!("Set flow rate for stroke"));
        edit_brush(app, &flow, |b, v| b.flow = v);
        return;
    }
    if matches!(tool, Tool::CloneStamp | Tool::HistoryBrush | Tool::ArtHistoryBrush) {
        let opacity = percent_field(ui, tl!("Opacity"), brush.opacity, 1.0..=100.0);
        edit_brush(app, &opacity, |b, v| b.opacity = v);
    }
    if matches!(tool, Tool::CloneStamp | Tool::HistoryBrush) {
        let flow = percent_field(ui, tl!("Flow"), brush.flow, 1.0..=100.0);
        edit_brush(app, &flow, |b, v| b.flow = v);
    }
}

/// Sends each event of a brush field to the engine, a scrub as one history step (`history_keys::coalesce`). The value
/// each event carries goes onto the brush as it is now, so events of one pass apply in order.
fn edit_brush(app: &mut PhotocraftApp, events: &EditEvents<f32>, set: impl Fn(&mut BrushSettings, f32)) {
    for event in events.iter() {
        let Some(value) = event.landed() else { continue };
        let before = app.session.tools.brush.clone();
        let mut after = before.clone();
        set(&mut after, *value);
        crate::brush_panel::commit_keyed(app, &before, &after, crate::history_keys::coalesce("brush", event));
    }
}

/// `add` captioned with `label` as a field: the label sits beside the control and a drag on it scrubs a number field.
/// `None` when the body did not run. `add` can run twice in a pass (the measuring run, then the real one).
pub(super) fn labelled<R>(ui: &mut egui::Ui, label: &str, mut add: impl FnMut(&mut egui::Ui) -> R) -> Option<R> {
    Field::new(label.to_owned()).show(ui, |ui, _| add(ui)).inner
}

/// A check box named `label` over `value`.
pub(super) fn check(ui: &mut egui::Ui, label: &str, value: &mut bool) {
    Checkbox::new(Bind::bound(value)).label(label.to_owned()).show(ui);
}

/// A number field named `label` over `value` (in `unit` when it is not empty), limited to `range`. The events of this
/// pass carry the edited value; an emptied field gives none.
pub(super) fn number_field(ui: &mut egui::Ui, label: &str, value: f32, range: RangeInclusive<f64>, unit: &'static str) -> EditEvents<f32> {
    let events = labelled(ui, label, |ui| {
        let field = NumberField::new(label.to_owned(), Bind::given(Some(f64::from(value)))).range(range.clone()).scrub_by_label();
        let field = if unit.is_empty() { field } else { field.unit(unit) };
        field.show(ui).events.clone()
    });
    events.unwrap_or_default().filter_map(|v| v.map(|v| v as f32))
}

/// A percent field over a fraction `fraction` (0.5 shows as 50 %); its events carry fractions. `range` is in percent.
pub(super) fn percent_field(ui: &mut egui::Ui, label: &str, fraction: f32, range: RangeInclusive<f64>) -> EditEvents<f32> {
    number_field(ui, label, fraction * 100.0, range, "%").map(|percent| percent / 100.0)
}

/// A whole-percent field 1..100 % over a fraction `fraction`, with `tip`; its events carry fractions. A fraction set
/// elsewhere (a `tools.setBrush` value) shows as its whole percent; a typed value past the range is clamped silently, as
/// the Brush's fields do.
fn whole_percent_field(ui: &mut egui::Ui, label: &str, fraction: f32, tip: &str) -> EditEvents<f32> {
    let percent = f64::from((fraction * 100.0).round());
    let events = labelled(ui, label, |ui| {
        let codec = NumberCodec::new(Env::of(ui).units().reader(None), 0);
        NumberField::new(label.to_owned(), Bind::given(Some(percent)))
            .range(1.0..=100.0)
            .scrub_by_label()
            .unit("%")
            .step(1.0)
            .commit_decimals(0)
            .codec(codec)
            .tooltip(tip.to_owned())
            .show(ui)
            .events
            .clone()
    });
    events.unwrap_or_default().filter_map(|v| v.map(|v| v.round() as f32 / 100.0))
}

/// The `layer.setProps` edit of a Layers panel Opacity or Fill (`key`) field at `pct` percent. A drag of the pop-up
/// slider carries its history step's `coalesce` key, so it is one step, as in Photoshop.
pub(super) fn pct_action(target: &Value, key: &str, pct: f32, coalesce: Option<String>) -> (String, Value) {
    let mut p = target.clone();
    p[key] = json!(pct / 100.0);
    if let Some(key) = coalesce {
        p["coalesce"] = json!(key);
    }
    ("layer.setProps".into(), p)
}

#[cfg(test)]
#[path = "../pattern_stamp_opacity_flow_tests.rs"]
mod pattern_stamp_opacity_flow_tests;

#[cfg(test)]
mod spektra_button_tests {
    use egui::accesskit::Role;
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use serde_json::json;

    use crate::PhotocraftApp;
    use crate::state::Tool;

    const PILL_LABEL: &str = "✦ SpektraFilm 胶片暗房";

    fn app() -> Box<PhotocraftApp> {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        app.run("file.new", json!({"width": 64, "height": 64})).unwrap();
        app.sync_views();
        app.ui.tool = Tool::Move;
        app
    }

    #[test]
    fn non_spektra_brand_omits_darkroom_pill() {
        let app = app();
        let mut h = Harness::builder().allow_missing_glyphs().with_size(vec2(1280.0, 200.0)).with_context(app.ctx().clone()).build_ui_state(
            |ui, app: &mut Box<PhotocraftApp>| {
                if !ui.ctx().fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    return;
                }
                super::options_bar_with_brand(app, ui, false);
            },
            app,
        );
        PhotocraftApp::setup_context(&h.ctx, crate::theme::AppTheme::studio());
        h.run_steps(4);
        assert!(h.query_by_label(PILL_LABEL).is_none());
    }

    #[test]
    fn spektra_pill_is_trailing_vertically_centered_and_reopens_on_second_click() {
        let app = app();
        let mut h = Harness::builder().allow_missing_glyphs().with_size(vec2(1280.0, 820.0)).with_context(app.ctx().clone()).build_ui_state(
            |ui, app: &mut Box<PhotocraftApp>| {
                if !ui.ctx().fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    return;
                }
                super::options_bar_with_brand(app, ui, true);
                crate::dialogs::run(app, ui);
            },
            app,
        );
        PhotocraftApp::setup_context(&h.ctx, crate::theme::AppTheme::studio());
        h.run_steps(4);

        let pill = h.get_by_label(PILL_LABEL);
        let pill_rect = pill.rect();
        let combo = h
            .get_all_by_role(Role::ComboBox)
            .find(|n| n.accesskit_node().value().as_deref() == Some("Layer") || n.accesskit_node().label().as_deref() == Some("Layer"))
            .expect("Move tool target dropdown renders in options bar");
        let combo_rect = combo.rect();

        assert!(
            pill_rect.min.x > 900.0 && pill_rect.min.x > combo_rect.max.x,
            "Spektra darkroom pill should sit at far right of options bar: pill={pill_rect:?}, combo={combo_rect:?}"
        );
        assert!(
            (pill_rect.center().y - combo_rect.center().y).abs() <= 1.5,
            "Spektra darkroom pill should be vertically centered with options bar controls: pill_cy={}, combo_cy={}",
            pill_rect.center().y,
            combo_rect.center().y
        );

        // First open via options-bar button -> Cancel -> second open via options-bar button.
        pill.click();
        h.run_steps(5);
        assert!(
            h.state().framework.dialogs.get_by_id::<crate::spektra_ui::SpektraDialog>(craft::ui::dialog::dialog_id(crate::spektra_ui::COMMAND)).is_some(),
            "first click on Spektra darkroom pill must open SpektraDialog"
        );
        h.get_by_label(tl!("Cancel")).click();
        h.run_steps(5);
        assert!(
            h.state().framework.dialogs.get_by_id::<crate::spektra_ui::SpektraDialog>(craft::ui::dialog::dialog_id(crate::spektra_ui::COMMAND)).is_none(),
            "Cancel must close SpektraDialog"
        );

        h.get_by_label(PILL_LABEL).click();
        h.run_steps(5);
        assert!(
            h.state().framework.dialogs.get_by_id::<crate::spektra_ui::SpektraDialog>(craft::ui::dialog::dialog_id(crate::spektra_ui::COMMAND)).is_some(),
            "second click on Spektra darkroom pill must open SpektraDialog again"
        );
    }

    #[test]
    #[ignore = "Manual visual snapshot generator (writes PNG to F:\\spektra-ui-preview)"]
    fn dump_spektra_options_bar_preview() {
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
        let _ = app.run("file.new", json!({"width": 64, "height": 64})).unwrap();
        app.sync_views();
        app.ui.tool = Tool::Move;
        let theme = if let Some(def) = crate::theme::spektra_theme_def() {
            let appearance = craft::theme::Appearance {
                mode: craft::theme::ramp::Mode::Dark,
                contrast: craft::theme::Contrast::Standard,
                size: craft::theme::Size::Compact,
                text_size: craft::theme::TextSize::Small,
                reduced_motion: false,
            };
            let resolved = craft::theme::resolve(&def, appearance, None);
            crate::theme::AppTheme::resolved(def, true, &resolved)
        } else {
            crate::theme::AppTheme::studio()
        };
        app.session.edit_prefs(|p| p.interface.language = "zh-hans".into());
        app.ui.theme = theme.clone();
        let theme_for_ctx = theme.clone();
        let theme_after = theme.clone();
        let ctx = app.ctx().clone();
        PhotocraftApp::setup_context(&ctx, theme);
        crate::i18n::sync_context(&ctx, "zh-hans");
        let mut h = Harness::builder().allow_missing_glyphs().with_size(vec2(1440.0, 96.0)).with_context(ctx).build_ui_state(
            move |ui, app: &mut Box<PhotocraftApp>| {
                if !ui.ctx().fonts(|f| f.families().contains(&egui::FontFamily::Name("medium".into()))) {
                    return;
                }
                app.ui.theme = theme_for_ctx.clone();
                crate::i18n::sync_context(ui.ctx(), "zh-hans");
                super::options_bar_with_brand(app, ui, true);
            },
            app,
        );
        PhotocraftApp::setup_context(&h.ctx, theme_after);
        crate::i18n::sync_context(&h.ctx, "zh-hans");
        for _ in 0..4 {
            h.step();
        }
        let preview_dir = std::path::PathBuf::from(r"F:\spektra-ui-preview");
        let _ = std::fs::create_dir_all(&preview_dir);
        h.snapshot_options(
            "spektra_options_bar",
            &egui_kittest::SnapshotOptions::default().output_path(&preview_dir),
        );
        let _ = h.take_snapshot_results();
    }
}
