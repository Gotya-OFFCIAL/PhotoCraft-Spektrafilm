//! The Adjustments panel: a grid of one-click adjustment layers.

use craft::imaging::ui::adjust::AdjustmentsPanel;
use craft::ui::contract::Show as _;
use craft::ui::display::Divider;
use egui::{Align2, Color32, CornerRadius, Sense, Stroke, StrokeKind, vec2};
use serde_json::json;

use crate::{PhotocraftApp, theme};

/// The Adjustments panel body: every adjustment kind, in the framework's order. A pick adds that
/// adjustment layer; the command brings Properties forward to edit it. The pick is the pass's own
/// click, so a rerun pass of the frame adds no second layer.
pub(crate) fn panel(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    if crate::panels::brand::is_spektra() {
        let enabled = app.session.is_enabled(crate::spektra_ui::COMMAND);
        let t = theme::Tokens::get(ui.ctx());
        let (fg, bg_normal, bg_hover, border) = if !enabled {
            (t.text_faint, t.field, t.field, t.separator)
        } else if t.dark() {
            let a = t.accent;
            (
                a,
                Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 22),
                Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 46),
                Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 180),
            )
        } else {
            (t.accent_text, t.accent_soft, t.hover, t.accent_border)
        };
        ui.add_enabled_ui(enabled, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            let resp = if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
                .on_hover_text("打开 SpektraFilm 81 波长光谱胶片与暗房印相工作台")
                .on_disabled_hover_text("请先选中像素图层或智能对象图层以启用光谱胶片暗房");
            let bg = if enabled && resp.hovered() { bg_hover } else { bg_normal };
            ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
            ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, border), StrokeKind::Inside);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "✦ SpektraFilm 光谱胶片暗房",
                theme::semibold(12.0),
                fg,
            );
            if resp.clicked()
                && let Err(e) = crate::menus::invoke(app, ui.ctx(), "filter.spektra.filmSim", json!({}))
            {
                app.ui.status = e;
            }
        });
        ui.add_space(6.0);
        Divider::horizontal().show(ui);
        ui.add_space(6.0);
    }
    let Some(kind) = AdjustmentsPanel::new(tl!("Adjustments")).show(ui).picked else { return };
    let _ = app.run(&format!("layer.newAdjustmentLayer.{}", kind.key()), json!({}));
}
