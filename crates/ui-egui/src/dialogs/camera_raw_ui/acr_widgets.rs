//! Adobe Camera Raw (ACR)-inspired custom widgets, vector icons, additive RGB histogram,
//! interactive tone curve editor, HSV color grading wheels, and bilingual terminology.

use std::ops::RangeInclusive;

use craft::imaging::raw::{GradeWheel, MAX_CURVE_POINTS, RawDevelopSettings as CameraRaw};
use craft::imaging::ui::raw_develop::RawFilmItem;
use craft::imaging::ui::scopes::{ClippingDiagnostic, ScopeReadout, ScopeSettings};
use egui::{
    Align2, Color32, CornerRadius, Id, Mesh, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2, epaint::Vertex, pos2, vec2,
};

use crate::theme::{self, Tokens};

/// Returns `zh` when the active UI language is Chinese (`zh-*`), otherwise `en`.
pub(crate) fn tr<'a>(zh: &'a str, en: &'a str) -> &'a str {
    if crate::i18n::current().code().starts_with("zh") {
        zh
    } else {
        en
    }
}

/// Harmonized ACR color tokens derived from the active PhotoCraft / Spektra theme.
#[derive(Clone, Copy)]
pub(crate) struct AcrColors {
    pub(crate) bg_canvas: Color32,
    pub(crate) bg_rail: Color32,
    pub(crate) bg_panel: Color32,
    pub(crate) bg_well: Color32,
    pub(crate) bg_group: Color32,
    pub(crate) bg_pill: Color32,
    pub(crate) bg_pill_hover: Color32,
    pub(crate) border: Color32,
    pub(crate) border_control: Color32,
    pub(crate) text: Color32,
    pub(crate) text_dim: Color32,
    pub(crate) text_faint: Color32,
    pub(crate) accent: Color32,
    pub(crate) accent_soft: Color32,
    pub(crate) accent_border: Color32,
    pub(crate) primary_bg: Color32,
    pub(crate) primary_text: Color32,
    pub(crate) danger: Color32,
    pub(crate) dark: bool,
}

impl AcrColors {
    pub(crate) fn from_ctx(ctx: &egui::Context) -> Self {
        let t = Tokens::get(ctx);
        let bg_group = if t.dark {
            Color32::from_rgb(
                t.card.r().saturating_sub(5),
                t.card.g().saturating_sub(5),
                t.card.b().saturating_sub(5),
            )
        } else {
            t.chrome
        };
        let bg_well = if t.dark {
            Color32::from_rgb(
                t.histogram_bg.r().min(26),
                t.histogram_bg.g().min(26),
                t.histogram_bg.b().min(26),
            )
        } else {
            t.field
        };
        let accent_soft = if t.dark {
            Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 52)
        } else {
            Color32::from_rgba_unmultiplied(t.accent.r(), t.accent.g(), t.accent.b(), 36)
        };
        Self {
            bg_canvas: t.canvas,
            bg_rail: t.chrome,
            bg_panel: t.card,
            bg_well,
            bg_group,
            bg_pill: t.hover,
            bg_pill_hover: t.pressed,
            border: t.separator,
            border_control: t.field_border,
            text: t.text,
            text_dim: t.text_dim,
            text_faint: t.text_faint,
            accent: t.accent,
            accent_soft,
            accent_border: t.accent_border,
            primary_bg: t.primary_bg,
            primary_text: t.primary_text,
            danger: t.danger,
            dark: t.dark,
        }
    }
}

/// Vector icons painted crisply via `egui::Painter`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolIcon {
    Edit,
    Eyedropper,
    Hand,
    Zoom,
    Sampler,
    SplitCompare,
    BeforeAfter,
    Reset,
    More,
    Undo,
    Redo,
}

/// Paints a crisp 16–20px vector icon centered at `c`.
pub(crate) fn draw_tool_icon(painter: &egui::Painter, c: Pos2, size: f32, icon: ToolIcon, color: Color32) {
    let r = size * 0.44;
    let stroke = Stroke::new(1.35, color);
    let thin = Stroke::new(1.15, color);
    match icon {
        ToolIcon::Edit => {
            for (i, kx) in [-0.35, 0.25, -0.10].into_iter().enumerate() {
                let y = c.y + (i as f32 - 1.0) * (r * 0.62);
                painter.line_segment([pos2(c.x - r, y), pos2(c.x + r, y)], thin);
                painter.circle_filled(pos2(c.x + kx * r, y), 2.1, color);
            }
        }
        ToolIcon::Eyedropper => {
            // Pipette rotated 45 deg top-right to bottom-left.
            let tip = pos2(c.x - r * 0.78, c.y + r * 0.78);
            let neck = pos2(c.x - r * 0.35, c.y + r * 0.35);
            let collar = pos2(c.x + r * 0.22, c.y - r * 0.22);
            let bulb = pos2(c.x + r * 0.68, c.y - r * 0.68);
            painter.line_segment([tip, neck], thin);
            painter.line_segment([neck, collar], Stroke::new(2.2, color));
            painter.line_segment([pos2(c.x - r * 0.02, c.y - r * 0.45), pos2(c.x + r * 0.45, c.y + r * 0.02)], stroke);
            painter.circle_stroke(bulb, r * 0.32, stroke);
        }
        ToolIcon::Hand => {
            // Four fingers + palm outline.
            let palm = Rect::from_center_size(pos2(c.x + r * 0.05, c.y + r * 0.28), vec2(r * 1.15, r * 0.92));
            painter.rect_stroke(palm, CornerRadius::same(3), stroke, StrokeKind::Inside);
            for (dx, top) in [(-0.36, -0.58), (-0.12, -0.82), (0.14, -0.78), (0.38, -0.55)] {
                let x = c.x + dx * r;
                painter.line_segment([pos2(x, palm.top() + 1.0), pos2(x, c.y + top * r)], stroke);
            }
            painter.line_segment(
                [pos2(palm.left(), palm.bottom() - 2.0), pos2(c.x - r * 0.85, c.y + r * 0.02)],
                stroke,
            );
        }
        ToolIcon::Zoom => {
            let center = pos2(c.x - r * 0.18, c.y - r * 0.18);
            painter.circle_stroke(center, r * 0.60, stroke);
            let handle_start = pos2(center.x + r * 0.42, center.y + r * 0.42);
            let handle_end = pos2(c.x + r * 0.85, c.y + r * 0.85);
            painter.line_segment([handle_start, handle_end], Stroke::new(1.8, color));
        }
        ToolIcon::Sampler => {
            painter.circle_stroke(c, r * 0.68, thin);
            painter.line_segment([pos2(c.x - r, c.y), pos2(c.x - r * 0.32, c.y)], thin);
            painter.line_segment([pos2(c.x + r * 0.32, c.y), pos2(c.x + r, c.y)], thin);
            painter.line_segment([pos2(c.x, c.y - r), pos2(c.x, c.y - r * 0.32)], thin);
            painter.line_segment([pos2(c.x, c.y + r * 0.32), pos2(c.x, c.y + r)], thin);
            painter.circle_filled(c, 1.4, color);
        }
        ToolIcon::SplitCompare => {
            let box_r = Rect::from_center_size(c, Vec2::splat(r * 1.75));
            painter.rect_stroke(box_r, CornerRadius::same(2), stroke, StrokeKind::Inside);
            let right_half = Rect::from_min_max(pos2(c.x, box_r.top() + 1.5), pos2(box_r.right() - 1.5, box_r.bottom() - 1.5));
            painter.rect_filled(right_half, CornerRadius::ZERO, color.linear_multiply(0.75));
            painter.line_segment([pos2(c.x, box_r.top() - 1.5), pos2(c.x, box_r.bottom() + 1.5)], stroke);
        }
        ToolIcon::BeforeAfter => {
            let left = Rect::from_min_max(pos2(c.x - r * 0.88, c.y - r * 0.68), pos2(c.x - 1.0, c.y + r * 0.68));
            let right = Rect::from_min_max(pos2(c.x + 1.0, c.y - r * 0.68), pos2(c.x + r * 0.88, c.y + r * 0.68));
            painter.rect_stroke(left, CornerRadius::same(1), thin, StrokeKind::Inside);
            painter.rect_filled(right, CornerRadius::same(1), color.linear_multiply(0.85));
        }
        ToolIcon::Reset => {
            // 270-degree circular arc + arrowhead.
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let ang = -std::f32::consts::FRAC_PI_4 + (i as f32 / 16.0) * std::f32::consts::PI * 1.5;
                    pos2(c.x + ang.cos() * r * 0.72, c.y + ang.sin() * r * 0.72)
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
            let tip = pos2(c.x - r * 0.72, c.y - r * 0.25);
            painter.line_segment([tip, pos2(tip.x - 3.2, tip.y - 2.6)], thin);
            painter.line_segment([tip, pos2(tip.x + 3.0, tip.y - 2.2)], thin);
        }
        ToolIcon::More => {
            for dx in [-r * 0.55, 0.0, r * 0.55] {
                painter.circle_filled(pos2(c.x + dx, c.y), 1.6, color);
            }
        }
        ToolIcon::Undo => {
            let tip = pos2(c.x - r * 0.75, c.y - r * 0.15);
            let pts = [
                tip,
                pos2(c.x - r * 0.1, c.y - r * 0.45),
                pos2(c.x + r * 0.55, c.y - r * 0.18),
                pos2(c.x + r * 0.65, c.y + r * 0.48),
            ];
            painter.add(Shape::line(pts.to_vec(), stroke));
            painter.line_segment([tip, pos2(tip.x + 3.8, tip.y - 2.8)], thin);
            painter.line_segment([tip, pos2(tip.x + 3.4, tip.y + 3.2)], thin);
        }
        ToolIcon::Redo => {
            let tip = pos2(c.x + r * 0.75, c.y - r * 0.15);
            let pts = [
                tip,
                pos2(c.x + r * 0.1, c.y - r * 0.45),
                pos2(c.x - r * 0.55, c.y - r * 0.18),
                pos2(c.x - r * 0.65, c.y + r * 0.48),
            ];
            painter.add(Shape::line(pts.to_vec(), stroke));
            painter.line_segment([tip, pos2(tip.x - 3.8, tip.y - 2.8)], thin);
            painter.line_segment([tip, pos2(tip.x - 3.4, tip.y + 3.2)], thin);
        }
    }
}

/// Iconic square button for the far-right ACR tool rail.
pub(crate) fn tool_rail_button(
    ui: &mut Ui,
    side: f32,
    icon: ToolIcon,
    active: bool,
    enabled: bool,
    tooltip: &str,
    c: &AcrColors,
) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), sense);
    let resp = if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
    .on_hover_text(tooltip);

    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(6), c.accent_soft);
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, c.accent), StrokeKind::Inside);
    } else if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(6), c.bg_pill);
    }
    let fg = if !enabled {
        c.text_faint
    } else if active {
        if c.dark { Color32::from_rgb(154, 208, 255) } else { c.accent }
    } else if resp.hovered() {
        c.text
    } else {
        c.text_dim
    };
    draw_tool_icon(ui.painter(), rect.center(), side * 0.58, icon, fg);
    resp
}

/// Secondary crisp pill button.
pub(crate) fn acr_pill_button(ui: &mut Ui, size: Vec2, label: &str, active: bool, enabled: bool, c: &AcrColors) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let resp = if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    };
    let (bg, border, fg) = if !enabled {
        (c.bg_group, c.border, c.text_faint)
    } else if active {
        (c.accent_soft, c.accent, c.text)
    } else if resp.hovered() {
        (c.bg_pill_hover, c.border_control, c.text)
    } else {
        (c.bg_pill, c.border_control, c.text)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
    ui.painter().rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, border), StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::medium(11.5), fg);
    resp
}

/// Primary accent button (Open / OK).
pub(crate) fn acr_primary_button(ui: &mut Ui, size: Vec2, label: &str, enabled: bool, c: &AcrColors) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let resp = if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    };
    let bg = if !enabled {
        c.bg_pill
    } else if resp.is_pointer_button_down_on() {
        c.primary_bg.linear_multiply(0.88)
    } else if resp.hovered() {
        Color32::from_rgb(
            c.primary_bg.r().saturating_add(12),
            c.primary_bg.g().saturating_add(12),
            c.primary_bg.b().saturating_add(10),
        )
    } else {
        c.primary_bg
    };
    let fg = if enabled { c.primary_text } else { c.text_faint };
    ui.painter().rect_filled(rect, CornerRadius::same(5), bg);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::semibold(12.0), fg);
    resp
}

fn sec_id(key: &str) -> Id {
    Id::unique("acr_sec").with(key)
}

/// Programmatically sets the open/collapsed state of an ACR accordion section (`"basic"`, `"curve"`, etc.).
#[cfg(test)]
pub(crate) fn set_section_open(ctx: &egui::Context, key: &str, open: bool) {
    ctx.data_mut(|d| d.insert_temp(sec_id(key), open));
}

/// Collapsible ACR accordion section with modified indicator and per-section reset button.
/// Returns `true` when the section reset icon is clicked.
pub(crate) fn acr_section(
    ui: &mut Ui,
    key: &str,
    title: &str,
    modified: bool,
    default_open: bool,
    c: &AcrColors,
    add_contents: impl FnOnce(&mut Ui),
) -> bool {
    let id = sec_id(key);
    let mut is_open = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(default_open);
    let mut reset_clicked = false;

    let w = ui.available_width();
    let (hdr_rect, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    ui.painter().line_segment(
        [pos2(hdr_rect.left(), hdr_rect.top()), pos2(hdr_rect.right(), hdr_rect.top())],
        Stroke::new(1.0, c.border),
    );

    let reset_w = if modified { 22.0 } else { 0.0 };
    let toggle_rect = Rect::from_min_max(hdr_rect.min, pos2((hdr_rect.right() - reset_w - 4.0).max(hdr_rect.left() + 20.0), hdr_rect.bottom()));
    let toggle_resp = ui
        .interact(toggle_rect, id.with("toggle"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if toggle_resp.hovered() {
        ui.painter().rect_filled(hdr_rect.shrink2(vec2(0.0, 1.0)), CornerRadius::same(4), Color32::from_white_alpha(if c.dark { 10 } else { 14 }));
    }

    // Vector chevron triangle.
    let chev_c = pos2(hdr_rect.left() + 10.0, hdr_rect.center().y);
    let chev_col = if toggle_resp.hovered() { c.text } else { c.text_dim };
    let tri = if is_open {
        vec![pos2(chev_c.x - 3.5, chev_c.y - 2.0), pos2(chev_c.x + 3.5, chev_c.y - 2.0), pos2(chev_c.x, chev_c.y + 3.0)]
    } else {
        vec![pos2(chev_c.x - 2.0, chev_c.y - 3.5), pos2(chev_c.x + 3.0, chev_c.y), pos2(chev_c.x - 2.0, chev_c.y + 3.5)]
    };
    ui.painter().add(Shape::convex_polygon(tri, chev_col, Stroke::NONE));

    let title_pos = pos2(hdr_rect.left() + 22.0, hdr_rect.center().y);
    let title_galley = ui.painter().layout_no_wrap(title.to_string(), theme::semibold(12.0), if is_open || toggle_resp.hovered() { c.text } else { c.text_dim });
    let title_w = title_galley.size().x;
    ui.painter().galley(pos2(title_pos.x, title_pos.y - title_galley.size().y * 0.5), title_galley, c.text);

    if modified {
        ui.painter().circle_filled(pos2(title_pos.x + title_w + 7.0, hdr_rect.center().y), 2.3, c.accent);
        let reset_rect = Rect::from_min_max(pos2(hdr_rect.right() - 24.0, hdr_rect.top() + 3.0), pos2(hdr_rect.right() - 2.0, hdr_rect.bottom() - 3.0));
        let reset_resp = ui
            .interact(reset_rect, id.with("reset"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tr("复位此面板参数", "Reset section"));
        if reset_resp.hovered() {
            ui.painter().rect_filled(reset_rect, CornerRadius::same(4), c.bg_pill);
        }
        draw_tool_icon(ui.painter(), reset_rect.center(), 12.0, ToolIcon::Reset, if reset_resp.hovered() { c.text } else { c.text_dim });
        if reset_resp.clicked() {
            reset_clicked = true;
        }
    }

    if toggle_resp.clicked() {
        is_open = !is_open;
        ui.ctx().data_mut(|d| d.insert_temp(id, is_open));
    }

    if is_open {
        ui.add_space(4.0);
        ui.indent(id.with("body"), |ui| {
            add_contents(ui);
        });
        ui.add_space(8.0);
    }

    reset_clicked
}

/// Result emitted by an interactive ACR slider row.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SliderEdit {
    Live(f32),
    Commit { before: f32, after: f32 },
    Immediate { before: f32, after: f32 },
}

/// Full-width ACR slider row: label on top-left, editable value pill on top-right,
/// gradient or bipolar track underneath, and double-click-to-reset.
#[allow(clippy::too_many_arguments)]
pub(crate) fn acr_slider_row(
    ui: &mut Ui,
    id_salt: &str,
    label: &str,
    value: f32,
    default: f32,
    range: RangeInclusive<f64>,
    step: f64,
    precision: usize,
    stops: Option<&[Color32]>,
    c: &AcrColors,
) -> (Option<SliderEdit>, bool) {
    let min = *range.start() as f32;
    let max = *range.end() as f32;
    let span = (max - min).max(1e-6);
    let id = ui.make_persistent_id(id_salt);
    let drag_key = id.with("drag_before");
    let edit_key = id.with("edit_text");
    let w = ui.available_width();

    let (row_rect, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
    let top_rect = Rect::from_min_size(row_rect.min, vec2(w, 16.0));
    let track_hit = Rect::from_min_max(pos2(row_rect.left(), top_rect.bottom()), row_rect.max);

    let modified = (value - default).abs() > 1e-4;
    let label_col = if modified { c.text } else { c.text_dim };

    let mut out: Option<SliderEdit> = None;

    // Numeric edit box state
    let editing: Option<String> = ui.ctx().data(|d| d.get_temp(edit_key));
    let pill_w = 50.0;
    let pill_rect = Rect::from_min_max(pos2((top_rect.right() - pill_w).max(top_rect.left() + 40.0), top_rect.top()), top_rect.right_bottom());
    let label_rect = Rect::from_min_max(top_rect.min, pos2(pill_rect.left() - 6.0, top_rect.bottom()));

    let label_resp = ui.interact(label_rect, id.with("lbl"), Sense::click()).on_hover_text(tr("双击恢复默认值", "Double-click to reset"));
    ui.painter().with_clip_rect(label_rect).text(
        pos2(label_rect.left(), label_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::medium(11.0),
        if label_resp.hovered() { c.text } else { label_col },
    );
    if label_resp.double_clicked() && modified {
        out = Some(SliderEdit::Immediate { before: value, after: default });
    }

    if let Some(mut buf) = editing {
        ui.painter().rect_filled(pill_rect, CornerRadius::same(3), c.bg_well);
        ui.painter().rect_stroke(pill_rect, CornerRadius::same(3), Stroke::new(1.0, c.accent), StrokeKind::Inside);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(pill_rect));
        let te = TextEdit::singleline(&mut buf)
            .id(id.with("te"))
            .font(theme::mono(10.5))
            .align(Align2::RIGHT_CENTER)
            .frame(egui::Frame::NONE)
            .desired_width(pill_w - 6.0);
        let resp = child.add(te);
        resp.request_focus();
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if esc {
            ui.ctx().data_mut(|d| d.remove::<String>(edit_key));
        } else if enter || resp.lost_focus() {
            if let Ok(parsed) = buf.trim().parse::<f32>() {
                let snapped = snap_step(parsed.clamp(min, max), step as f32, precision);
                if (snapped - value).abs() > 1e-5 {
                    out = Some(SliderEdit::Immediate { before: value, after: snapped });
                }
            }
            ui.ctx().data_mut(|d| d.remove::<String>(edit_key));
        } else {
            ui.ctx().data_mut(|d| d.insert_temp(edit_key, buf));
        }
    } else {
        let val_text = format!("{value:.precision$}");
        let pill_resp = ui
            .interact(pill_rect, id.with("pill"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::Text)
            .on_hover_text(tr("点击输入精确数值 · 双击复位", "Click to enter value · Double-click to reset"));
        ui.painter().rect_filled(
            pill_rect,
            CornerRadius::same(3),
            if pill_resp.hovered() { c.bg_pill } else { Color32::TRANSPARENT },
        );
        if pill_resp.hovered() {
            ui.painter().rect_stroke(pill_rect, CornerRadius::same(3), Stroke::new(1.0, c.border_control), StrokeKind::Inside);
        }
        ui.painter().text(
            pos2(pill_rect.right() - 4.0, pill_rect.center().y),
            Align2::RIGHT_CENTER,
            val_text,
            theme::mono(10.5),
            if modified { c.text } else { c.text_dim },
        );
        if pill_resp.double_clicked() && modified {
            out = Some(SliderEdit::Immediate { before: value, after: default });
        } else if pill_resp.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(edit_key, format!("{value:.precision$}")));
        }
    }

    // Track geometry + drag interaction
    let resp = ui
        .interact(track_hit, id.with("trk"), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let pad_x = 6.0;
    let bar_x0 = track_hit.left() + pad_x;
    let bar_x1 = (track_hit.right() - pad_x).max(bar_x0 + 16.0);
    let bar_w = bar_x1 - bar_x0;
    let cy = track_hit.center().y;
    let bar_rect = Rect::from_min_max(pos2(bar_x0, cy - 2.0), pos2(bar_x1, cy + 2.0));

    if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_key, value));
    }

    let mut active_dragging = false;
    if resp.double_clicked() && modified {
        out = Some(SliderEdit::Immediate { before: value, after: default });
        ui.ctx().data_mut(|d| d.remove::<f32>(drag_key));
    } else if resp.dragged() {
        active_dragging = true;
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - bar_x0) / bar_w).clamp(0.0, 1.0);
            let next = snap_step(min + t * span, step as f32, precision);
            if (next - value).abs() > 1e-5 {
                out = Some(SliderEdit::Live(next));
            }
        }
    } else if resp.drag_stopped() {
        let before = ui.ctx().data(|d| d.get_temp::<f32>(drag_key)).unwrap_or(value);
        ui.ctx().data_mut(|d| d.remove::<f32>(drag_key));
        let final_v = resp
            .interact_pointer_pos()
            .map(|p| snap_step(min + ((p.x - bar_x0) / bar_w).clamp(0.0, 1.0) * span, step as f32, precision))
            .unwrap_or(value);
        if (final_v - before).abs() > 1e-5 {
            out = Some(SliderEdit::Commit { before, after: final_v });
        }
    } else if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let t = ((pos.x - bar_x0) / bar_w).clamp(0.0, 1.0);
        let next = snap_step(min + t * span, step as f32, precision);
        if (next - value).abs() > 1e-5 {
            out = Some(SliderEdit::Immediate { before: value, after: next });
        }
    }

    // Draw track background or multi-stop gradient
    if let Some(stops) = stops
        && stops.len() >= 2
    {
        paint_gradient_track(ui.painter(), bar_rect, stops);
        ui.painter().rect_stroke(bar_rect, CornerRadius::same(2), Stroke::new(0.8, Color32::from_black_alpha(120)), StrokeKind::Inside);
    } else {
        ui.painter().rect_filled(bar_rect, CornerRadius::same(2), c.bg_well);
        ui.painter().rect_stroke(bar_rect, CornerRadius::same(2), Stroke::new(0.8, c.border_control), StrokeKind::Inside);
        // Active fill bar from 0 (bipolar) or min (unipolar) to thumb
        let anchor_t = if min < 0.0 && max > 0.0 { (-min / span).clamp(0.0, 1.0) } else { 0.0 };
        let val_t = ((value - min) / span).clamp(0.0, 1.0);
        let ax = bar_x0 + anchor_t * bar_w;
        let vx = bar_x0 + val_t * bar_w;
        let fill_r = Rect::from_min_max(pos2(ax.min(vx), bar_rect.top()), pos2(ax.max(vx), bar_rect.bottom()));
        if fill_r.width() > 1.0 {
            ui.painter().rect_filled(fill_r, CornerRadius::same(2), c.accent.linear_multiply(0.75));
        }
    }

    // Center zero notch for bipolar sliders
    if min < 0.0 && max > 0.0 {
        let zx = bar_x0 + ((-min / span).clamp(0.0, 1.0)) * bar_w;
        ui.painter().line_segment([pos2(zx, cy + 4.0), pos2(zx, cy + 7.5)], Stroke::new(1.0, c.text_faint));
    }

    // Thumb knob
    let val_t = ((value - min) / span).clamp(0.0, 1.0);
    let kx = bar_x0 + val_t * bar_w;
    let kc = pos2(kx, cy);
    if active_dragging || resp.hovered() {
        ui.painter().circle_filled(kc, 8.0, c.accent_soft);
    }
    let thumb_fill = Color32::from_rgb(245, 243, 238);
    let thumb_stroke = if active_dragging || resp.hovered() { c.accent } else { c.border_control };
    ui.painter().circle_filled(kc, 5.2, thumb_fill);
    ui.painter().circle_stroke(kc, 5.2, Stroke::new(1.3, thumb_stroke));

    (out, active_dragging)
}

fn snap_step(val: f32, step: f32, precision: usize) -> f32 {
    let stepped = if step > 0.0 { (val / step).round() * step } else { val };
    let factor = 10f32.powi(precision as i32);
    (stepped * factor).round() / factor
}

fn paint_gradient_track(painter: &egui::Painter, rect: Rect, stops: &[Color32]) {
    let mut mesh = Mesh::default();
    let n = stops.len();
    for (i, &col) in stops.iter().enumerate() {
        let t = i as f32 / (n - 1).max(1) as f32;
        let x = rect.left() + t * rect.width();
        let idx = (i * 2) as u32;
        mesh.vertices.push(Vertex { pos: pos2(x, rect.top()), uv: pos2(0.0, 0.0), color: col });
        mesh.vertices.push(Vertex { pos: pos2(x, rect.bottom()), uv: pos2(0.0, 1.0), color: col });
        if i + 1 < n {
            mesh.indices.extend_from_slice(&[idx, idx + 1, idx + 2, idx + 1, idx + 3, idx + 2]);
        }
    }
    painter.add(Shape::mesh(mesh));
}

pub(crate) struct HistogramDragOutcome {
    pub(crate) edit: Option<SliderEdit>,
    pub(crate) field: &'static str,
    pub(crate) diagnostic: Option<ClippingDiagnostic>,
}

/// Interactive ACR additive RGB histogram with 96th-percentile interior normalization,
/// shadow/highlight clipping warning triangles (`U`/`O`), and draggable exposure zones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_acr_histogram(
    ui: &mut Ui,
    channels: Option<[&[u64; 256]; 3]>,
    shadows_clipped: [u64; 3],
    highlights_clipped: [u64; 3],
    pending: bool,
    scope: &mut ScopeSettings,
    settings: &CameraRaw,
    c: &AcrColors,
) -> Option<HistogramDragOutcome> {
    let w = ui.available_width();
    let plot_h = 92.0;
    let (plot_rect, _) = ui.allocate_exact_size(vec2(w, plot_h), Sense::hover());

    ui.painter().rect_filled(plot_rect, CornerRadius::same(4), c.bg_well);
    ui.painter().rect_stroke(plot_rect, CornerRadius::same(4), Stroke::new(1.0, c.border_control), StrokeKind::Inside);

    let inner = plot_rect.shrink2(vec2(2.0, 3.0));
    let zones: [(&'static str, &str, f32, f32, fn(&CameraRaw) -> f32, f32, f32, f32); 5] = [
        ("blacks", tr("黑色色阶", "Blacks"), 0.0, 0.12, |s| s.blacks, -100.0, 100.0, 1.0),
        ("shadows", tr("阴影", "Shadows"), 0.12, 0.30, |s| s.shadows, -100.0, 100.0, 1.0),
        ("exposure", tr("曝光", "Exposure"), 0.30, 0.70, |s| s.exposure, -5.0, 5.0, 0.05),
        ("highlights", tr("高光", "Highlights"), 0.70, 0.88, |s| s.highlights, -100.0, 100.0, 1.0),
        ("whites", tr("白色色阶", "Whites"), 0.88, 1.0, |s| s.whites, -100.0, 100.0, 1.0),
    ];

    // Zone interaction (underneath clipping buttons)
    let id = ui.make_persistent_id("acr_histogram_zones");
    let zone_drag_key = id.with("zone_drag"); // Option<(usize, f32, f32)> -> (zone_idx, start_x, start_val)
    let alt_down = ui.input(|i| i.modifiers.alt);
    let mut active_zone: Option<usize> = None;
    let mut outcome: Option<HistogramDragOutcome> = None;

    let clip_btn_size = 18.0;
    let shadow_btn_r = Rect::from_min_size(pos2(plot_rect.left() + 4.0, plot_rect.top() + 4.0), Vec2::splat(clip_btn_size));
    let highlight_btn_r = Rect::from_min_size(pos2(plot_rect.right() - 4.0 - clip_btn_size, plot_rect.top() + 4.0), Vec2::splat(clip_btn_size));

    let s_resp = ui
        .interact(shadow_btn_r, id.with("warn_s"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tr("阴影剪贴警告 (U)", "Shadow Clipping Warning (U)"));
    let h_resp = ui
        .interact(highlight_btn_r, id.with("warn_h"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tr("高光剪贴警告 (O)", "Highlight Clipping Warning (O)"));

    let zone_resp = ui.interact(inner, id.with("plot"), Sense::click_and_drag());
    let over_clip_btn = s_resp.hovered() || h_resp.hovered();

    if let Some((idx, sx, sv)) = ui.ctx().data(|d| d.get_temp::<(usize, f32, f32)>(zone_drag_key)) {
        active_zone = Some(idx);
        let (key, _, _, _, _, min, max, step) = zones[idx];
        let range_scale = if key == "exposure" { 6.0 } else { 140.0 };
        if zone_resp.dragged() {
            if let Some(pos) = zone_resp.interact_pointer_pos() {
                let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                let dx = (pos.x - sx) / inner.width().max(1.0);
                let raw_next = (sv + dx * range_scale * fine).clamp(min, max);
                let prec = if step < 1.0 { 2 } else { 0 };
                let next = snap_step(raw_next, step, prec);
                let diag = if alt_down {
                    match key {
                        "blacks" | "shadows" => Some(ClippingDiagnostic::Shadows),
                        "exposure" | "highlights" | "whites" => Some(ClippingDiagnostic::Highlights),
                        _ => None,
                    }
                } else {
                    None
                };
                outcome = Some(HistogramDragOutcome {
                    edit: Some(SliderEdit::Live(next)),
                    field: key,
                    diagnostic: diag,
                });
            }
        } else if zone_resp.drag_stopped() {
            let final_v = zone_resp
                .interact_pointer_pos()
                .map(|pos| {
                    let fine = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                    let dx = (pos.x - sx) / inner.width().max(1.0);
                    let prec = if step < 1.0 { 2 } else { 0 };
                    snap_step((sv + dx * range_scale * fine).clamp(min, max), step, prec)
                })
                .unwrap_or((zones[idx].4)(settings));
            ui.ctx().data_mut(|d| d.remove::<(usize, f32, f32)>(zone_drag_key));
            outcome = Some(HistogramDragOutcome {
                edit: ((final_v - sv).abs() > 1e-5).then_some(SliderEdit::Commit { before: sv, after: final_v }),
                field: key,
                diagnostic: None,
            });
        }
    } else if !over_clip_btn
        && let Some(pos) = zone_resp.hover_pos()
        && inner.contains(pos)
    {
        let fx = ((pos.x - inner.left()) / inner.width().max(1.0)).clamp(0.0, 1.0);
        let idx = zones.iter().position(|z| fx < z.3).unwrap_or(zones.len() - 1);
        active_zone = Some(idx);
        if zone_resp.drag_started() {
            let start_val = (zones[idx].4)(settings);
            ui.ctx().data_mut(|d| d.insert_temp(zone_drag_key, (idx, pos.x, start_val)));
        } else if zone_resp.double_clicked() {
            let cur = (zones[idx].4)(settings);
            if cur.abs() > 1e-5 {
                outcome = Some(HistogramDragOutcome {
                    edit: Some(SliderEdit::Immediate { before: cur, after: 0.0 }),
                    field: zones[idx].0,
                    diagnostic: None,
                });
            }
        }
    }

    // Subtle exposure zone background highlight + vertical dividers
    for (i, z) in zones.iter().enumerate() {
        let zx0 = inner.left() + z.2 * inner.width();
        let zx1 = inner.left() + z.3 * inner.width();
        if active_zone == Some(i) {
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(zx0, plot_rect.top() + 1.0), pos2(zx1, plot_rect.bottom() - 1.0)),
                CornerRadius::ZERO,
                Color32::from_white_alpha(12),
            );
        }
        if i > 0 {
            ui.painter().line_segment(
                [pos2(zx0, plot_rect.top() + 2.0), pos2(zx0, plot_rect.bottom() - 2.0)],
                Stroke::new(1.0, Color32::from_white_alpha(16)),
            );
        }
    }

    // Paint additive RGB histogram ribbons using 96th percentile across interior bins [2..253]
    if let Some([r_bins, g_bins, b_bins]) = channels {
        let scale_peak = interior_96th_peak(r_bins, g_bins, b_bins);
        let cols = (inner.width().round() as usize).clamp(64, 320);
        let col_w = inner.width() / cols as f32;
        let max_h = (inner.height() - 4.0).max(16.0);
        let bot = inner.bottom();

        let mut r_pts = Vec::with_capacity(cols);
        let mut g_pts = Vec::with_capacity(cols);
        let mut b_pts = Vec::with_capacity(cols);

        for px in 0..cols {
            let bin_idx = ((px as f32 + 0.5) / cols as f32 * 255.0).round().clamp(0.0, 255.0) as usize;
            let hr = ((r_bins[bin_idx] as f32 / scale_peak).clamp(0.0, 1.0)) * max_h;
            let hg = ((g_bins[bin_idx] as f32 / scale_peak).clamp(0.0, 1.0)) * max_h;
            let hb = ((b_bins[bin_idx] as f32 / scale_peak).clamp(0.0, 1.0)) * max_h;

            let x0 = inner.left() + px as f32 * col_w;
            let x1 = x0 + col_w + 0.35;
            let xc = (x0 + x1) * 0.5;
            r_pts.push(pos2(xc, bot - hr));
            g_pts.push(pos2(xc, bot - hg));
            b_pts.push(pos2(xc, bot - hb));

            let mut pairs = [(hr, 1u8), (hg, 2u8), (hb, 4u8)];
            pairs.sort_by(|a, b| a.0.total_cmp(&b.0));

            if pairs[0].0 > 0.5 {
                let col = Color32::from_rgba_unmultiplied(192, 190, 186, 175);
                ui.painter().rect_filled(Rect::from_min_max(pos2(x0, bot - pairs[0].0), pos2(x1, bot)), CornerRadius::ZERO, col);
            }
            if pairs[1].0 > pairs[0].0 + 0.5 {
                let mask = pairs[1].1 | pairs[2].1;
                let col = channel_mask_fill(mask);
                ui.painter().rect_filled(
                    Rect::from_min_max(pos2(x0, bot - pairs[1].0), pos2(x1, bot - pairs[0].0)),
                    CornerRadius::ZERO,
                    col,
                );
            }
            if pairs[2].0 > pairs[1].0 + 0.5 {
                let col = channel_mask_fill(pairs[2].1);
                ui.painter().rect_filled(
                    Rect::from_min_max(pos2(x0, bot - pairs[2].0), pos2(x1, bot - pairs[1].0)),
                    CornerRadius::ZERO,
                    col,
                );
            }
        }

        ui.painter().add(Shape::line(r_pts, Stroke::new(1.0, Color32::from_rgb(240, 92, 86))));
        ui.painter().add(Shape::line(g_pts, Stroke::new(1.0, Color32::from_rgb(95, 214, 110))));
        ui.painter().add(Shape::line(b_pts, Stroke::new(1.0, Color32::from_rgb(96, 148, 248))));
    } else if pending {
        ui.painter().text(
            inner.center(),
            Align2::CENTER_CENTER,
            tr("正在计算直方图…", "Calculating histogram…"),
            theme::medium(10.5),
            c.text_faint,
        );
    }

    // Active tone-zone readout chip at bottom-left
    if let Some(idx) = active_zone {
        let (key, zlabel, _, _, get_val, _, _, step) = zones[idx];
        let val = (get_val)(settings);
        let text = if step < 1.0 || key == "exposure" {
            format!("{zlabel} {val:+.2}")
        } else {
            format!("{zlabel} {val:.0}")
        };
        let pos = pos2(inner.left() + 6.0, inner.bottom() - 8.0);
        let g = ui.painter().layout_no_wrap(text, theme::medium(10.0), c.text);
        let bg_r = Rect::from_min_size(pos2(pos.x - 4.0, pos.y - 6.0), g.size() + vec2(8.0, 4.0));
        ui.painter().rect_filled(bg_r, CornerRadius::same(3), Color32::from_black_alpha(165));
        ui.painter().galley(pos2(pos.x, pos.y - 4.0), g, c.text);
    }

    // Paint shadow/highlight clipping triangles
    paint_clip_button(ui.painter(), shadow_btn_r, shadows_clipped, scope.shadow_warning, s_resp.hovered(), c);
    paint_clip_button(ui.painter(), highlight_btn_r, highlights_clipped, scope.highlight_warning, h_resp.hovered(), c);
    if s_resp.clicked() {
        scope.shadow_warning = !scope.shadow_warning;
    }
    if h_resp.clicked() {
        scope.highlight_warning = !scope.highlight_warning;
    }

    outcome
}

fn interior_96th_peak(r: &[u64; 256], g: &[u64; 256], b: &[u64; 256]) -> f32 {
    let mut vals: Vec<u64> = (2..254).map(|i| r[i].max(g[i]).max(b[i])).filter(|v| *v > 0).collect();
    if vals.is_empty() {
        let all_max = (0..256).map(|i| r[i].max(g[i]).max(b[i])).max().unwrap_or(1);
        return (all_max as f32).max(1.0);
    }
    vals.sort_unstable();
    let idx = ((vals.len() - 1) as f32 * 0.96).round() as usize;
    (vals[idx.min(vals.len() - 1)] as f32 * 1.18).max(1.0)
}

fn channel_mask_fill(mask: u8) -> Color32 {
    match mask {
        1 => Color32::from_rgba_unmultiplied(228, 74, 70, 195),
        2 => Color32::from_rgba_unmultiplied(74, 198, 92, 195),
        4 => Color32::from_rgba_unmultiplied(78, 128, 245, 195),
        3 => Color32::from_rgba_unmultiplied(214, 194, 72, 185),
        6 => Color32::from_rgba_unmultiplied(74, 198, 206, 185),
        5 => Color32::from_rgba_unmultiplied(208, 88, 212, 185),
        _ => Color32::from_rgba_unmultiplied(195, 194, 190, 175),
    }
}

fn paint_clip_button(painter: &egui::Painter, rect: Rect, clipped: [u64; 3], active: bool, hovered: bool, c: &AcrColors) {
    let has_clip = clipped.iter().any(|n| *n > 0);
    if active || hovered {
        painter.rect_filled(rect, CornerRadius::same(3), if active { c.accent_soft } else { c.bg_pill });
    }
    if active {
        painter.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, c.text), StrokeKind::Inside);
    }
    let tri_col = if !has_clip {
        Color32::from_rgb(76, 76, 74)
    } else {
        Color32::from_rgb(
            if clipped[0] > 0 { 242 } else { 40 },
            if clipped[1] > 0 { 242 } else { 40 },
            if clipped[2] > 0 { 242 } else { 40 },
        )
    };
    let cx = rect.center().x;
    let cy = rect.center().y;
    let tri = vec![pos2(cx, cy - 4.2), pos2(cx + 4.5, cy + 4.0), pos2(cx - 4.5, cy + 4.0)];
    painter.add(Shape::convex_polygon(tri, tri_col, Stroke::new(0.8, Color32::from_black_alpha(140))));
}

/// Live pointer / image info strip shown directly beneath the ACR histogram.
pub(crate) fn draw_readout_strip(
    ui: &mut Ui,
    readout: Option<ScopeReadout>,
    full_size: [usize; 2],
    c: &AcrColors,
) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 18.0), Sense::hover());
    match readout {
        Some(ScopeReadout::Rgb([rr, gg, bb])) => {
            let rv = (rr * 255.0).round().clamp(0.0, 255.0) as u8;
            let gv = (gg * 255.0).round().clamp(0.0, 255.0) as u8;
            let bv = (bb * 255.0).round().clamp(0.0, 255.0) as u8;
            let mut x = r.left() + 2.0;
            for (lbl, val, col) in [
                ("R", rv, Color32::from_rgb(235, 110, 104)),
                ("G", gv, Color32::from_rgb(108, 212, 122)),
                ("B", bv, Color32::from_rgb(112, 162, 248)),
            ] {
                ui.painter().text(pos2(x, r.center().y), Align2::LEFT_CENTER, lbl, theme::semibold(10.5), col);
                ui.painter().text(pos2(x + 12.0, r.center().y), Align2::LEFT_CENTER, format!("{val:<3}"), theme::mono(10.5), c.text);
                x += 52.0;
            }
        }
        Some(ScopeReadout::Lab([l, a, b])) => {
            let text = format!("L {:.0}   a {:.0}   b {:.0}", l, a, b);
            ui.painter().text(pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, text, theme::mono(10.5), c.text);
        }
        Some(_) => {}
        None => {
            let mp = (full_size[0] as f64 * full_size[1] as f64) / 1_000_000.0;
            let info = format!("{} × {} px  ·  {:.1} MP", full_size[0], full_size[1], mp);
            ui.painter().text(pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, info, theme::mono(10.0), c.text_dim);
        }
    }
}

/// Interactive Tone Curve panel (`参数` | `RGB` | `红` | `绿` | `蓝`).
pub(crate) fn draw_curve_panel(
    ui: &mut Ui,
    settings: &CameraRaw,
    channels: Option<[&[u64; 256]; 3]>,
    c: &AcrColors,
) -> Option<(CameraRaw, &'static str, bool)> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum CurveTab {
        Parametric,
        Rgb,
        Red,
        Green,
        Blue,
    }
    let tab_id = Id::unique("acr_curve_tab");
    let mut tab = ui.ctx().data(|d| d.get_temp::<usize>(tab_id)).unwrap_or(0);
    let tabs = [
        (0usize, CurveTab::Parametric, tr("参数", "Param"), c.accent),
        (1usize, CurveTab::Rgb, "RGB", Color32::from_rgb(225, 225, 222)),
        (2usize, CurveTab::Red, tr("红", "R"), Color32::from_rgb(235, 88, 82)),
        (3usize, CurveTab::Green, tr("绿", "G"), Color32::from_rgb(92, 206, 108)),
        (4usize, CurveTab::Blue, tr("蓝", "B"), Color32::from_rgb(94, 146, 245)),
    ];

    let w = ui.available_width();
    let (bar_r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
    ui.painter().rect_filled(bar_r, CornerRadius::same(4), c.bg_well);
    ui.painter().rect_stroke(bar_r, CornerRadius::same(4), Stroke::new(1.0, c.border), StrokeKind::Inside);
    let seg_w = (bar_r.width() - 4.0) / tabs.len() as f32;
    for (idx, _, lbl, dot_col) in tabs {
        let sr = Rect::from_min_size(pos2(bar_r.left() + 2.0 + idx as f32 * seg_w, bar_r.top() + 2.0), vec2(seg_w, bar_r.height() - 4.0));
        let active = tab == idx;
        let resp = ui.interact(sr, tab_id.with(("t", idx)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if active {
            ui.painter().rect_filled(sr, CornerRadius::same(3), c.accent_soft);
            ui.painter().rect_stroke(sr, CornerRadius::same(3), Stroke::new(1.0, c.accent_border), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(sr, CornerRadius::same(3), c.bg_pill);
        }
        if idx >= 1 {
            ui.painter().circle_filled(pos2(sr.left() + 8.0, sr.center().y), 2.5, dot_col);
            ui.painter().text(pos2(sr.center().x + 3.0, sr.center().y), Align2::CENTER_CENTER, lbl, theme::medium(10.5), if active { c.text } else { c.text_dim });
        } else {
            ui.painter().text(sr.center(), Align2::CENTER_CENTER, lbl, theme::medium(10.5), if active { c.text } else { c.text_dim });
        }
        if resp.clicked() {
            tab = idx;
            ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
        }
    }
    ui.add_space(6.0);

    // Square curve graph
    let side = w.min(220.0);
    let (graph_outer, _) = ui.allocate_exact_size(vec2(w, side), Sense::hover());
    let g_rect = Rect::from_center_size(graph_outer.center(), Vec2::splat(side));
    ui.painter().rect_filled(g_rect, CornerRadius::same(4), c.bg_well);
    ui.painter().rect_stroke(g_rect, CornerRadius::same(4), Stroke::new(1.0, c.border_control), StrokeKind::Inside);

    // 4x4 grid + diagonal identity
    for k in 1..4 {
        let f = k as f32 * 0.25;
        ui.painter().line_segment(
            [pos2(g_rect.left() + f * side, g_rect.top()), pos2(g_rect.left() + f * side, g_rect.bottom())],
            Stroke::new(1.0, Color32::from_white_alpha(18)),
        );
        ui.painter().line_segment(
            [pos2(g_rect.left(), g_rect.top() + f * side), pos2(g_rect.right(), g_rect.top() + f * side)],
            Stroke::new(1.0, Color32::from_white_alpha(18)),
        );
    }
    ui.painter().line_segment(
        [g_rect.left_bottom(), g_rect.right_top()],
        Stroke::new(1.0, Color32::from_white_alpha(38)),
    );

    // Muted histogram backdrop inside curve graph
    if let Some([r_b, g_b, b_b]) = channels {
        let peak = interior_96th_peak(r_b, g_b, b_b);
        let bg_bars: Vec<u64> = match tab {
            2 => r_b.to_vec(),
            3 => g_b.to_vec(),
            4 => b_b.to_vec(),
            _ => (0..256).map(|i| (r_b[i] + g_b[i] + b_b[i]) / 3).collect(),
        };
        let cols = 64usize;
        let cw = side / cols as f32;
        for px in 0..cols {
            let bi = (px * 255 / cols).min(255);
            let h = ((bg_bars[bi] as f32 / peak).clamp(0.0, 1.0)) * (side * 0.65);
            if h > 0.5 {
                ui.painter().rect_filled(
                    Rect::from_min_max(pos2(g_rect.left() + px as f32 * cw, g_rect.bottom() - h), pos2(g_rect.left() + (px + 1) as f32 * cw, g_rect.bottom())),
                    CornerRadius::ZERO,
                    Color32::from_white_alpha(20),
                );
            }
        }
    }

    let mut result: Option<(CameraRaw, &'static str, bool)> = None;
    let active_tab = tabs[tab.min(4)].1;

    if active_tab == CurveTab::Parametric {
        // Parametric split dividers + preview curve
        let splits = settings.curve_splits.map(|v| v.clamp(5.0, 95.0) / 100.0);
        for sx in splits {
            let px = g_rect.left() + sx * side;
            ui.painter().line_segment(
                [pos2(px, g_rect.top() + 2.0), pos2(px, g_rect.bottom() - 2.0)],
                Stroke::new(1.0, c.accent.linear_multiply(0.38)),
            );
        }
        let lut = eval_parametric_curve(settings, 65);
        let pts: Vec<Pos2> = lut
            .iter()
            .enumerate()
            .map(|(i, &y)| {
                let fx = i as f32 / 64.0;
                pos2(g_rect.left() + fx * side, g_rect.bottom() - y * side)
            })
            .collect();
        ui.painter().add(Shape::line(pts, Stroke::new(2.0, c.accent)));

        ui.add_space(6.0);
        for (key, lbl, val, set_fn) in [
            ("curve_hi", tr("高光", "Highlights"), settings.curve_highlights, (|s: &mut CameraRaw, v: f32| s.curve_highlights = v) as fn(&mut CameraRaw, f32)),
            ("curve_lt", tr("亮调", "Lights"), settings.curve_lights, |s, v| s.curve_lights = v),
            ("curve_dk", tr("暗调", "Darks"), settings.curve_darks, |s, v| s.curve_darks = v),
            ("curve_sh", tr("阴影", "Shadows"), settings.curve_shadows, |s, v| s.curve_shadows = v),
        ] {
            let (edit, _) = acr_slider_row(ui, key, lbl, val, 0.0, -100.0..=100.0, 1.0, 0, None, c);
            if let Some(ev) = edit {
                let next_v = match ev {
                    SliderEdit::Live(v) | SliderEdit::Commit { after: v, .. } | SliderEdit::Immediate { after: v, .. } => v,
                };
                let mut next = settings.clone();
                set_fn(&mut next, next_v);
                let live = matches!(ev, SliderEdit::Live(_));
                result = Some((next, tr("参数曲线", "Parametric Curve"), live));
            }
        }
    } else {
        let curve_col = tabs[tab.min(4)].3;
        let current_pts: &Vec<[f32; 2]> = match active_tab {
            CurveTab::Red => &settings.point_curve_red,
            CurveTab::Green => &settings.point_curve_green,
            CurveTab::Blue => &settings.point_curve_blue,
            _ => &settings.point_curve,
        };
        let mut pts = if current_pts.len() >= 2 {
            current_pts.clone()
        } else {
            vec![[0.0, 0.0], [255.0, 255.0]]
        };

        // Sample spline and paint curve
        let lut = eval_point_curve(&pts, 65);
        let line_pts: Vec<Pos2> = lut
            .iter()
            .enumerate()
            .map(|(i, &y)| pos2(g_rect.left() + (i as f32 / 64.0) * side, g_rect.bottom() - y * side))
            .collect();
        ui.painter().add(Shape::line(line_pts, Stroke::new(2.0, curve_col)));

        let to_screen = |pt: [f32; 2]| -> Pos2 { pos2(g_rect.left() + (pt[0] / 255.0) * side, g_rect.bottom() - (pt[1] / 255.0) * side) };
        let to_curve = |p: Pos2| -> [f32; 2] {
            [
                (((p.x - g_rect.left()) / side) * 255.0).round().clamp(0.0, 255.0),
                (((g_rect.bottom() - p.y) / side) * 255.0).round().clamp(0.0, 255.0),
            ]
        };

        let drag_idx_id = tab_id.with(("pt_drag", tab));
        let mut dragging_idx: Option<usize> = ui.ctx().data(|d| d.get_temp(drag_idx_id));
        let curve_resp = ui
            .interact(g_rect, tab_id.with(("canvas", tab)), Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::Crosshair);

        let mut changed = false;
        let mut live = false;

        if curve_resp.drag_started()
            && let Some(pos) = curve_resp.interact_pointer_pos()
        {
            if let Some((i, _)) = pts.iter().enumerate().find(|(_, p)| to_screen(**p).distance(pos) <= 9.0) {
                dragging_idx = Some(i);
                ui.ctx().data_mut(|d| d.insert_temp(drag_idx_id, i));
            } else if pts.len() < MAX_CURVE_POINTS {
                let c_pt = to_curve(pos);
                let ok_spacing = pts.iter().all(|p| (p[0] - c_pt[0]).abs() >= 1.5);
                if ok_spacing {
                    pts.push(c_pt);
                    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
                    if let Some(idx) = pts.iter().position(|p| (p[0] - c_pt[0]).abs() < 0.01) {
                        dragging_idx = Some(idx);
                        ui.ctx().data_mut(|d| d.insert_temp(drag_idx_id, idx));
                        changed = true;
                        live = true;
                    }
                }
            }
        } else if curve_resp.dragged()
            && let (Some(idx), Some(pos)) = (dragging_idx, curve_resp.interact_pointer_pos())
        {
            let mut c_pt = to_curve(pos);
            let min_x = if idx == 0 { 0.0 } else { (pts[idx - 1][0] + 1.5).min(255.0) };
            let max_x = if idx + 1 >= pts.len() { 255.0 } else { (pts[idx + 1][0] - 1.5).max(0.0) };
            c_pt[0] = c_pt[0].clamp(min_x, max_x.max(min_x));
            if let Some(slot) = pts.get_mut(idx)
                && (slot[0] != c_pt[0] || slot[1] != c_pt[1])
            {
                *slot = c_pt;
                changed = true;
                live = true;
            }
        } else if curve_resp.drag_stopped() && dragging_idx.is_some() {
            ui.ctx().data_mut(|d| d.remove::<usize>(drag_idx_id));
            changed = true;
            live = false;
        } else if (curve_resp.secondary_clicked() || curve_resp.double_clicked())
            && let Some(pos) = curve_resp.interact_pointer_pos()
            && pts.len() > 2
            && let Some((idx, _)) = pts
                .iter()
                .enumerate()
                .skip(1)
                .take(pts.len().saturating_sub(2))
                .find(|(_, p)| to_screen(**p).distance(pos) <= 10.0)
        {
            pts.remove(idx);
            changed = true;
            live = false;
        } else if curve_resp.clicked()
            && let Some(pos) = curve_resp.interact_pointer_pos()
            && pts.iter().all(|p| to_screen(*p).distance(pos) > 9.0)
            && pts.len() < MAX_CURVE_POINTS
        {
            let c_pt = to_curve(pos);
            if pts.iter().all(|p| (p[0] - c_pt[0]).abs() >= 1.5) {
                pts.push(c_pt);
                pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
                changed = true;
                live = false;
            }
        }

        for (i, pt) in pts.iter().enumerate() {
            let sp = to_screen(*pt);
            let active = dragging_idx == Some(i);
            ui.painter().circle_filled(sp, if active { 4.8 } else { 3.8 }, curve_col);
            ui.painter().circle_stroke(sp, if active { 4.8 } else { 3.8 }, Stroke::new(1.2, c.bg_well));
        }

        if changed {
            let normalized = if pts.len() == 2 && (pts[0][0] - 0.0).abs() < 0.5 && (pts[0][1] - 0.0).abs() < 0.5 && (pts[1][0] - 255.0).abs() < 0.5 && (pts[1][1] - 255.0).abs() < 0.5 {
                Vec::new()
            } else {
                pts
            };
            let mut next = settings.clone();
            match active_tab {
                CurveTab::Red => next.point_curve_red = normalized,
                CurveTab::Green => next.point_curve_green = normalized,
                CurveTab::Blue => next.point_curve_blue = normalized,
                _ => next.point_curve = normalized,
            }
            if next.validate().is_ok() {
                result = Some((next, tr("点曲线", "Point Curve"), live));
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let hover_txt = curve_resp
                .hover_pos()
                .map(|p| {
                    let cp = to_curve(p);
                    format!("{} {:.0}  /  {} {:.0}", tr("输入", "In"), cp[0], tr("输出", "Out"), cp[1])
                })
                .unwrap_or_else(|| tr("单击加点 · 右键/双击删点", "Click to add · Right-click to remove").to_string());
            ui.label(egui::RichText::new(hover_txt).font(theme::medium(10.0)).color(c.text_dim));
        });
    }

    result
}

fn eval_parametric_curve(p: &CameraRaw, n: usize) -> Vec<f32> {
    let s = p.curve_splits.map(|v| v.clamp(5.0, 95.0) / 100.0);
    let centres = [s[0] / 2.0, (s[0] + s[1]) / 2.0, (s[1] + s[2]) / 2.0, (s[2] + 1.0) / 2.0];
    let amounts = [p.curve_shadows, p.curve_darks, p.curve_lights, p.curve_highlights];
    let widths = [s[0].max(0.1), (s[1] - s[0]).max(0.1), (s[2] - s[1]).max(0.1), (1.0 - s[2]).max(0.1)];
    let mut lut: Vec<f32> = (0..n)
        .map(|i| {
            let x = i as f32 / (n - 1).max(1) as f32;
            let mut y = x;
            for k in 0..4 {
                let d = (x - centres[k]) / (widths[k] * 1.2);
                let bump = (-d * d * 2.0).exp() * x * (1.0 - x) * 4.0;
                y += amounts[k] / 100.0 * 0.25 * bump;
            }
            y.clamp(0.0, 1.0)
        })
        .collect();
    for i in 1..n {
        lut[i] = lut[i].max(lut[i - 1]);
    }
    lut
}

fn eval_point_curve(pts: &[[f32; 2]], n: usize) -> Vec<f32> {
    if pts.len() < 2 {
        return (0..n).map(|i| i as f32 / (n - 1).max(1) as f32).collect();
    }
    let m = pts.len();
    let xs: Vec<f32> = pts.iter().map(|p| (p[0] / 255.0).clamp(0.0, 1.0)).collect();
    let ys: Vec<f32> = pts.iter().map(|p| (p[1] / 255.0).clamp(0.0, 1.0)).collect();
    let mut d = vec![0.0f32; m - 1];
    for i in 0..m - 1 {
        let h = (xs[i + 1] - xs[i]).max(1e-5);
        d[i] = (ys[i + 1] - ys[i]) / h;
    }
    let mut tangents = vec![0.0f32; m];
    tangents[0] = d[0];
    tangents[m - 1] = d[m - 2];
    for i in 1..m - 1 {
        if d[i - 1] * d[i] > 0.0 {
            tangents[i] = 2.0 * d[i - 1] * d[i] / (d[i - 1] + d[i]);
        }
    }
    (0..n)
        .map(|i| {
            let x = i as f32 / (n - 1).max(1) as f32;
            if x <= xs[0] {
                return ys[0];
            }
            if x >= xs[m - 1] {
                return ys[m - 1];
            }
            let seg = xs.windows(2).position(|w| x <= w[1]).unwrap_or(m - 2);
            let h = (xs[seg + 1] - xs[seg]).max(1e-5);
            let t = ((x - xs[seg]) / h).clamp(0.0, 1.0);
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            (h00 * ys[seg] + h10 * h * tangents[seg] + h01 * ys[seg + 1] + h11 * h * tangents[seg + 1]).clamp(0.0, 1.0)
        })
        .collect()
}

/// Interactive ACR Color Mixer (`色相` | `饱和度` | `明亮度` | `全部`) over 8 HSL bands.
pub(crate) fn draw_color_mixer_panel(
    ui: &mut Ui,
    settings: &CameraRaw,
    c: &AcrColors,
) -> Option<(CameraRaw, &'static str, bool)> {
    let tab_id = Id::unique("acr_mixer_tab");
    let mut tab = ui.ctx().data(|d| d.get_temp::<usize>(tab_id)).unwrap_or(0);
    let tabs = [
        (0usize, tr("色相", "Hue")),
        (1usize, tr("饱和度", "Sat")),
        (2usize, tr("明亮度", "Lum")),
        (3usize, tr("全部", "All")),
    ];

    let w = ui.available_width();
    let (bar_r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
    ui.painter().rect_filled(bar_r, CornerRadius::same(4), c.bg_well);
    ui.painter().rect_stroke(bar_r, CornerRadius::same(4), Stroke::new(1.0, c.border), StrokeKind::Inside);
    let seg_w = (bar_r.width() - 4.0) / tabs.len() as f32;
    for (idx, lbl) in tabs {
        let sr = Rect::from_min_size(pos2(bar_r.left() + 2.0 + idx as f32 * seg_w, bar_r.top() + 2.0), vec2(seg_w, bar_r.height() - 4.0));
        let active = tab == idx;
        let resp = ui.interact(sr, tab_id.with(("t", idx)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if active {
            ui.painter().rect_filled(sr, CornerRadius::same(3), c.accent_soft);
            ui.painter().rect_stroke(sr, CornerRadius::same(3), Stroke::new(1.0, c.accent), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(sr, CornerRadius::same(3), c.bg_pill);
        }
        ui.painter().text(sr.center(), Align2::CENTER_CENTER, lbl, theme::medium(10.5), if active { c.text } else { c.text_dim });
        if resp.clicked() {
            tab = idx;
            ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
        }
    }
    ui.add_space(6.0);

    let bands: [(&str, &str, f32); 8] = [
        ("r", tr("红色", "Reds"), 0.0),
        ("o", tr("橙色", "Oranges"), 30.0),
        ("y", tr("黄色", "Yellows"), 60.0),
        ("g", tr("绿色", "Greens"), 120.0),
        ("a", tr("浅绿色", "Aquas"), 180.0),
        ("b", tr("蓝色", "Blues"), 240.0),
        ("p", tr("紫色", "Purples"), 270.0),
        ("m", tr("洋红色", "Magentas"), 300.0),
    ];

    let mut result = None;
    let show_modes: &[usize] = match tab {
        0 => &[0],
        1 => &[1],
        2 => &[2],
        _ => &[0, 1, 2],
    };

    for &mode in show_modes {
        if tab == 3 {
            let hdr = match mode {
                0 => tr("色相", "Hue"),
                1 => tr("饱和度", "Saturation"),
                _ => tr("明亮度", "Luminance"),
            };
            ui.label(egui::RichText::new(hdr).font(theme::semibold(11.0)).color(c.text_dim));
            ui.add_space(2.0);
        }
        for (k, (tag, bname, hue)) in bands.iter().enumerate() {
            let val = match mode {
                0 => settings.hsl_hue[k],
                1 => settings.hsl_sat[k],
                _ => settings.hsl_lum[k],
            };
            let stops = hsl_band_stops(mode, *hue);
            let key = format!("hsl_{mode}_{tag}");
            let (edit, _) = acr_slider_row(ui, &key, bname, val, 0.0, -100.0..=100.0, 1.0, 0, Some(&stops), c);
            if let Some(ev) = edit {
                let v = match ev {
                    SliderEdit::Live(x) | SliderEdit::Commit { after: x, .. } | SliderEdit::Immediate { after: x, .. } => x,
                };
                let mut next = settings.clone();
                match mode {
                    0 => next.hsl_hue[k] = v,
                    1 => next.hsl_sat[k] = v,
                    _ => next.hsl_lum[k] = v,
                }
                result = Some((next, tr("混色器", "Color Mixer"), matches!(ev, SliderEdit::Live(_))));
            }
        }
        if tab == 3 {
            ui.add_space(4.0);
        }
    }

    result
}

fn hsl_band_stops(mode: usize, hue: f32) -> [Color32; 3] {
    let col = |h: f32, s: f32, v: f32| Color32::from(egui::ecolor::Hsva::new((h / 360.0).rem_euclid(1.0), s, v, 1.0));
    match mode {
        0 => [col(hue - 30.0, 0.85, 0.95), col(hue, 0.85, 0.95), col(hue + 30.0, 0.85, 0.95)],
        1 => [col(hue, 0.08, 0.82), col(hue, 0.55, 0.92), col(hue, 1.0, 1.0)],
        _ => [col(hue, 0.85, 0.22), col(hue, 0.85, 0.65), col(hue, 0.25, 0.98)],
    }
}

/// Interactive ACR Color Grading Wheels (`全部` | `阴影` | `中间调` | `高光` | `全局`).
pub(crate) fn draw_color_grading_panel(
    ui: &mut Ui,
    settings: &CameraRaw,
    c: &AcrColors,
) -> Option<(CameraRaw, &'static str, bool)> {
    let tab_id = Id::unique("acr_grade_tab");
    let mut tab = ui.ctx().data(|d| d.get_temp::<usize>(tab_id)).unwrap_or(0);
    let tabs = [
        (0usize, tr("全部", "All")),
        (1usize, tr("阴影", "Shad")),
        (2usize, tr("中间调", "Mid")),
        (3usize, tr("高光", "High")),
        (4usize, tr("全局", "Glob")),
    ];

    let w = ui.available_width();
    let (bar_r, _) = ui.allocate_exact_size(vec2(w, 24.0), Sense::hover());
    ui.painter().rect_filled(bar_r, CornerRadius::same(4), c.bg_well);
    ui.painter().rect_stroke(bar_r, CornerRadius::same(4), Stroke::new(1.0, c.border), StrokeKind::Inside);
    let seg_w = (bar_r.width() - 4.0) / tabs.len() as f32;
    for (idx, lbl) in tabs {
        let sr = Rect::from_min_size(pos2(bar_r.left() + 2.0 + idx as f32 * seg_w, bar_r.top() + 2.0), vec2(seg_w, bar_r.height() - 4.0));
        let active = tab == idx;
        let resp = ui.interact(sr, tab_id.with(("t", idx)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if active {
            ui.painter().rect_filled(sr, CornerRadius::same(3), c.accent_soft);
            ui.painter().rect_stroke(sr, CornerRadius::same(3), Stroke::new(1.0, c.accent), StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(sr, CornerRadius::same(3), c.bg_pill);
        }
        ui.painter().text(sr.center(), Align2::CENTER_CENTER, lbl, theme::medium(10.0), if active { c.text } else { c.text_dim });
        if resp.clicked() {
            tab = idx;
            ui.ctx().data_mut(|d| d.insert_temp(tab_id, tab));
        }
    }
    ui.add_space(6.0);

    let mut result: Option<(CameraRaw, &'static str, bool)> = None;

    if tab == 0 {
        // 3 wheels compact overview: Midtones on top-center, Shadows & Highlights below, or crisp 2+1 layout
        let small_dia = ((w - 18.0) * 0.44).clamp(76.0, 96.0);
        ui.vertical_centered(|ui| {
            if let Some((wheel, live)) = draw_one_color_wheel(ui, "mid_small", tr("中间调", "Midtones"), small_dia, settings.grade_midtones, c) {
                let mut next = settings.clone();
                next.grade_midtones = wheel;
                result = Some((next, tr("颜色分级", "Color Grading"), live));
            }
        });
        ui.add_space(4.0);
        ui.columns(2, |cols| {
            if let Some((wheel, live)) = draw_one_color_wheel(&mut cols[0], "shad_small", tr("阴影", "Shadows"), small_dia, settings.grade_shadows, c) {
                let mut next = settings.clone();
                next.grade_shadows = wheel;
                result = Some((next, tr("颜色分级", "Color Grading"), live));
            }
            if let Some((wheel, live)) = draw_one_color_wheel(&mut cols[1], "high_small", tr("高光", "Highlights"), small_dia, settings.grade_highlights, c) {
                let mut next = settings.clone();
                next.grade_highlights = wheel;
                result = Some((next, tr("颜色分级", "Color Grading"), live));
            }
        });
    } else {
        let (key, title, wheel_now) = match tab {
            1 => ("shad_big", tr("阴影", "Shadows"), settings.grade_shadows),
            2 => ("mid_big", tr("中间调", "Midtones"), settings.grade_midtones),
            3 => ("high_big", tr("高光", "Highlights"), settings.grade_highlights),
            _ => ("glob_big", tr("全局", "Global"), settings.grade_global),
        };
        let big_dia = (w - 36.0).clamp(112.0, 154.0);
        ui.vertical_centered(|ui| {
            if let Some((wheel, live)) = draw_one_color_wheel(ui, key, title, big_dia, wheel_now, c) {
                let mut next = settings.clone();
                match tab {
                    1 => next.grade_shadows = wheel,
                    2 => next.grade_midtones = wheel,
                    3 => next.grade_highlights = wheel,
                    _ => next.grade_global = wheel,
                }
                result = Some((next, tr("颜色分级", "Color Grading"), live));
            }
        });
        ui.add_space(4.0);
        let lum_stops = [Color32::from_rgb(20, 20, 20), Color32::from_rgb(128, 128, 128), Color32::from_rgb(242, 242, 240)];
        let (edit, _) = acr_slider_row(ui, &format!("{key}_lum"), tr("明亮度", "Luminance"), wheel_now.lum, 0.0, -100.0..=100.0, 1.0, 0, Some(&lum_stops), c);
        if let Some(ev) = edit {
            let v = match ev {
                SliderEdit::Live(x) | SliderEdit::Commit { after: x, .. } | SliderEdit::Immediate { after: x, .. } => x,
            };
            let mut next = settings.clone();
            match tab {
                1 => next.grade_shadows.lum = v,
                2 => next.grade_midtones.lum = v,
                3 => next.grade_highlights.lum = v,
                _ => next.grade_global.lum = v,
            }
            result = Some((next, tr("颜色分级", "Color Grading"), matches!(ev, SliderEdit::Live(_))));
        }
    }

    ui.add_space(4.0);
    let (b_edit, _) = acr_slider_row(ui, "grade_blend", tr("混合", "Blending"), settings.grade_blending, 50.0, 0.0..=100.0, 1.0, 0, None, c);
    if let Some(ev) = b_edit {
        let v = match ev {
            SliderEdit::Live(x) | SliderEdit::Commit { after: x, .. } | SliderEdit::Immediate { after: x, .. } => x,
        };
        let mut next = settings.clone();
        next.grade_blending = v;
        result = Some((next, tr("混合", "Blending"), matches!(ev, SliderEdit::Live(_))));
    }
    let (bal_edit, _) = acr_slider_row(ui, "grade_bal", tr("平衡", "Balance"), settings.grade_balance, 0.0, -100.0..=100.0, 1.0, 0, None, c);
    if let Some(ev) = bal_edit {
        let v = match ev {
            SliderEdit::Live(x) | SliderEdit::Commit { after: x, .. } | SliderEdit::Immediate { after: x, .. } => x,
        };
        let mut next = settings.clone();
        next.grade_balance = v;
        result = Some((next, tr("平衡", "Balance"), matches!(ev, SliderEdit::Live(_))));
    }

    result
}

fn draw_one_color_wheel(
    ui: &mut Ui,
    key: &str,
    label: &str,
    diameter: f32,
    wheel: GradeWheel,
    c: &AcrColors,
) -> Option<(GradeWheel, bool)> {
    ui.label(egui::RichText::new(label).font(theme::medium(10.5)).color(c.text_dim));
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click_and_drag());
    let resp = resp.on_hover_cursor(egui::CursorIcon::Crosshair);
    let center = rect.center();
    let rad = (diameter * 0.5 - 3.0).max(16.0);

    // Paint smooth HSV disc via polar triangles mesh
    paint_hsv_wheel_mesh(ui.painter(), center, rad);
    ui.painter().circle_stroke(center, rad, Stroke::new(1.2, c.border_control));
    ui.painter().line_segment([pos2(center.x - 4.0, center.y), pos2(center.x + 4.0, center.y)], Stroke::new(1.0, Color32::from_white_alpha(120)));
    ui.painter().line_segment([pos2(center.x, center.y - 4.0), pos2(center.x, center.y + 4.0)], Stroke::new(1.0, Color32::from_white_alpha(120)));

    let mut out = None;
    let id = ui.make_persistent_id(("acr_wheel", key));
    let drag_key = id.with("dragging");

    let map_pos = |pos: Pos2| -> (f32, f32) {
        let v = pos - center;
        let sat = ((v.length() / rad) * 100.0).clamp(0.0, 100.0);
        let hue = v.y.atan2(v.x).to_degrees().rem_euclid(360.0);
        (hue, sat)
    };

    if resp.double_clicked() && (wheel.sat > 0.0 || wheel.hue > 0.0) {
        let mut next = wheel;
        next.hue = 0.0;
        next.sat = 0.0;
        out = Some((next, false));
        ui.ctx().data_mut(|d| d.remove::<bool>(drag_key));
    } else if resp.drag_started() {
        ui.ctx().data_mut(|d| d.insert_temp(drag_key, true));
        if let Some(pos) = resp.interact_pointer_pos() {
            let (h, s) = map_pos(pos);
            let mut next = wheel;
            next.hue = h;
            next.sat = s;
            out = Some((next, true));
        }
    } else if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let (h, s) = map_pos(pos);
            let mut next = wheel;
            next.hue = h;
            next.sat = s;
            out = Some((next, true));
        }
    } else if resp.drag_stopped() {
        let was = ui.ctx().data(|d| d.get_temp::<bool>(drag_key)).unwrap_or(false);
        ui.ctx().data_mut(|d| d.remove::<bool>(drag_key));
        if was {
            let mut next = wheel;
            if let Some(pos) = resp.interact_pointer_pos() {
                let (h, s) = map_pos(pos);
                next.hue = h;
                next.sat = s;
            }
            out = Some((next, false));
        }
    } else if resp.clicked()
        && let Some(pos) = resp.interact_pointer_pos()
    {
        let (h, s) = map_pos(pos);
        let mut next = wheel;
        next.hue = h;
        next.sat = s;
        out = Some((next, false));
    }

    // Current puck position (smooth concentric filled rings free of small-stroke miter artifacts)
    let ang = wheel.hue.to_radians();
    let dist = (wheel.sat / 100.0).clamp(0.0, 1.0) * rad;
    let puck = pos2(center.x + ang.cos() * dist, center.y + ang.sin() * dist);
    if dist > 2.0 {
        ui.painter().line_segment([center, puck], Stroke::new(1.1, Color32::from_white_alpha(180)));
    }
    let puck_col = if wheel.sat < 1.0 {
        Color32::from_gray(143)
    } else {
        Color32::from(egui::ecolor::Hsva::new((wheel.hue / 360.0).rem_euclid(1.0), (wheel.sat / 100.0).clamp(0.0, 1.0), 0.92, 1.0))
    };
    ui.painter().circle_filled(puck, 6.0, Color32::from_black_alpha(110));
    ui.painter().circle_filled(puck, 4.8, Color32::WHITE);
    ui.painter().circle_filled(puck, 3.2, puck_col);

    let caption = format!("H {:.0}°  S {:.0}", wheel.hue, wheel.sat);
    ui.label(egui::RichText::new(caption).font(theme::mono(9.5)).color(c.text_faint));
    out
}

fn paint_hsv_wheel_mesh(painter: &egui::Painter, center: Pos2, rad: f32) {
    const SECTORS: usize = 48;
    const RINGS: usize = 6;
    let mut mesh = Mesh::default();
    let center_v = 0.56f32;
    let center_col = Color32::from_gray((center_v * 255.0) as u8);
    mesh.vertices.push(Vertex { pos: center, uv: pos2(0.5, 0.5), color: center_col });

    for r in 1..=RINGS {
        let frac = r as f32 / RINGS as f32;
        let rr = rad * frac;
        let v = center_v + 0.18 * frac;
        for s in 0..SECTORS {
            let h = s as f32 / SECTORS as f32;
            let ang = h * std::f32::consts::TAU;
            let pos = pos2(center.x + ang.cos() * rr, center.y + ang.sin() * rr);
            let col = Color32::from(egui::ecolor::Hsva::new(h, frac, v, 1.0));
            mesh.vertices.push(Vertex { pos, uv: pos2(0.0, 0.0), color: col });
        }
    }

    // Innermost fan
    for s in 0..SECTORS {
        let next_s = (s + 1) % SECTORS;
        mesh.indices.extend_from_slice(&[0, 1 + s as u32, 1 + next_s as u32]);
    }
    // Outer rings
    for r in 1..RINGS {
        let row0 = 1 + (r - 1) * SECTORS;
        let row1 = 1 + r * SECTORS;
        for s in 0..SECTORS {
            let next_s = (s + 1) % SECTORS;
            let a = (row0 + s) as u32;
            let b = (row0 + next_s) as u32;
            let c = (row1 + s) as u32;
            let d = (row1 + next_s) as u32;
            mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Horizontal filmstrip rendered above the bottom canvas bar whenever multiple RAWs are opened together.
pub(crate) fn draw_filmstrip(ui: &mut Ui, items: &[RawFilmItem], current: usize, c: &AcrColors) -> Option<usize> {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, CornerRadius::ZERO, c.bg_rail);
    ui.painter().line_segment([pos2(rect.left(), rect.top()), pos2(rect.right(), rect.top())], Stroke::new(1.0, c.border));

    let mut picked = None;
    let scroll_rect = rect.shrink2(vec2(8.0, 4.0));
    let mut scroll_ui = ui.new_child(egui::UiBuilder::new().max_rect(scroll_rect));
    egui::ScrollArea::horizontal().id_salt("acr-filmstrip").auto_shrink([false, false]).show(&mut scroll_ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (idx, item) in items.iter().enumerate() {
                let selected = idx == current;
                let (tile_r, resp) = ui.allocate_exact_size(vec2(96.0, 64.0), Sense::click());
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&item.name);

                ui.painter().rect_filled(tile_r, CornerRadius::same(4), c.bg_panel);
                let border_col = if selected {
                    c.accent
                } else if resp.hovered() {
                    c.text_dim
                } else {
                    c.border
                };
                ui.painter().rect_stroke(tile_r, CornerRadius::same(4), Stroke::new(if selected { 1.6 } else { 1.0 }, border_col), StrokeKind::Inside);

                let img_r = Rect::from_min_max(pos2(tile_r.left() + 4.0, tile_r.top() + 4.0), pos2(tile_r.right() - 4.0, tile_r.bottom() - 16.0));
                if let Some(pic) = item.picture {
                    let aspect = (pic.size.x / pic.size.y.max(1.0)).max(0.05);
                    let box_aspect = (img_r.width() / img_r.height().max(1.0)).max(0.05);
                    let draw_size = if aspect > box_aspect {
                        vec2(img_r.width(), img_r.width() / aspect)
                    } else {
                        vec2(img_r.height() * aspect, img_r.height())
                    };
                    let fitted = Rect::from_center_size(img_r.center(), draw_size);
                    ui.painter().image(pic.id, fitted, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                } else {
                    ui.painter().rect_filled(img_r, CornerRadius::same(2), c.bg_well);
                }

                if item.edited {
                    ui.painter().circle_filled(pos2(tile_r.right() - 8.0, tile_r.top() + 8.0), 3.0, c.accent);
                }
                let text_r = Rect::from_min_max(pos2(tile_r.left() + 4.0, tile_r.bottom() - 15.0), pos2(tile_r.right() - 4.0, tile_r.bottom() - 1.0));
                ui.painter().with_clip_rect(text_r).text(
                    pos2(text_r.center().x, text_r.center().y),
                    Align2::CENTER_CENTER,
                    &item.name,
                    theme::medium(9.5),
                    if selected { c.text } else { c.text_dim },
                );

                if resp.clicked() && idx != current {
                    picked = Some(idx);
                }
            }
        });
    });

    picked
}
