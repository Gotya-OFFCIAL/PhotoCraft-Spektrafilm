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
        let enabled = app.session.active().is_some();
        ui.add_enabled_ui(enabled, |ui| {
            let amber = Color32::from_rgb(225, 169, 95);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            let resp = resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("打开 SpektraFilm 81 波长光谱胶片与暗房印相工作台");
            let bg = if resp.hovered() {
                Color32::from_rgba_unmultiplied(225, 169, 95, 42)
            } else {
                Color32::from_rgba_unmultiplied(225, 169, 95, 22)
            };
            ui.painter().rect_filled(rect, CornerRadius::same(6), bg);
            ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, amber), StrokeKind::Inside);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "✦ SpektraFilm 光谱胶片暗房",
                theme::semibold(12.0),
                amber,
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
