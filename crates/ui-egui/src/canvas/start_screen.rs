//! The Home screen: the framework start page with PhotoCraft's recent files, links and drop hint, and the Home rule
//! the options bar's button, the rail's Home link and `window.home` share.

use craft::behavior::IconId;
use craft::ui::contract::{Bind, Show as _};
use craft::ui::runtime::env::Label;
use craft::ui::shell::{StartAction, StartLink, StartScreen};

use super::PhotocraftApp;

/// Recent files listed on the Home screen.
const HOME_RECENT: usize = 11;

/// The Home screen in the document area. The recent list is empty on the web, which has no paths to reopen, and with
/// Show Home Recent Files off.
pub(super) fn start_screen(app: &mut PhotocraftApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let show_recent = !cfg!(target_arch = "wasm32") && app.session.prefs().interface.show_home_recent_files;
    let recent = if show_recent { crate::home_recent_ui::recent_files(app, HOME_RECENT) } else { Vec::new() };
    let thumbs = crate::home_recent_ui::thumbnails(&ctx);
    // The source reads the app, so the page binds copies of the view and the sort and they are written back after.
    let (mut view, mut sort) = (app.ui.chrome.home_recent_view, app.ui.chrome.home_recent_sort);
    let links = links();
    let actions = if crate::panels::brand::is_spektra() {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            render_spektra_banner(ui);
            let source = crate::home_recent_ui::source(app, &ctx);
            StartScreen::new(Bind::bound(&mut view), Bind::bound(&mut sort), &recent)
                .thumbnails(&thumbs, &source)
                .links(&links[..])
                .max_recent(HOME_RECENT)
                .drop_hint(Label::from(tl!("Drop an image or PSD anywhere to open it.")))
                .show(ui)
                .actions
        })
        .inner
    } else {
        let source = crate::home_recent_ui::source(app, &ctx);
        StartScreen::new(Bind::bound(&mut view), Bind::bound(&mut sort), &recent)
            .thumbnails(&thumbs, &source)
            .links(&links[..])
            .max_recent(HOME_RECENT)
            .drop_hint(Label::from(tl!("Drop an image or PSD anywhere to open it.")))
            .show(ui)
            .actions
    };
    app.ui.chrome.home_recent_view = view;
    app.ui.chrome.home_recent_sort = sort;
    for action in actions {
        // The recent list's own actions and Open; New is the New Document dialog's, Show in Folder the platform's.
        if let StartAction::RevealRecent(path) = &action {
            reveal(app, path);
            continue;
        }
        if crate::home_recent_ui::act(app, &action) {
            continue;
        }
        if action == StartAction::New {
            crate::new_doc_ui::open(app);
        }
    }
}

/// Brand header strip shown at the top of the Home page under `PhotoCraft-Spektrafilm`.
fn render_spektra_banner(ui: &mut egui::Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let avail_w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, 46.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, t.chrome);
    ui.painter().line_segment([egui::pos2(rect.left(), rect.bottom()), egui::pos2(rect.right(), rect.bottom())], egui::Stroke::new(1.0, t.separator));
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(20.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    row.spacing_mut().item_spacing.x = 10.0;
    crate::panels::brand::mark().show(&mut row);
    row.label(egui::RichText::new(crate::panels::app_name()).font(crate::theme::semibold(15.0)).color(t.text));
    let badge_fg = if t.dark() { t.accent } else { t.accent_text };
    let badge_bg = t.accent_soft;
    let badge_text = "81 波长光谱暗房";
    let galley = row.painter().layout_no_wrap(badge_text.into(), crate::theme::semibold(10.5), badge_fg);
    let (b_rect, _) = row.allocate_exact_size(egui::vec2(galley.size().x + 14.0, 20.0), egui::Sense::hover());
    row.painter().rect_filled(b_rect, egui::CornerRadius::same(99), badge_bg);
    row.painter().rect_stroke(b_rect, egui::CornerRadius::same(99), egui::Stroke::new(1.0, t.accent_border), egui::StrokeKind::Inside);
    row.painter().text(b_rect.center(), egui::Align2::CENTER_CENTER, badge_text, crate::theme::semibold(10.5), badge_fg);
    row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.label(egui::RichText::new("胶片物理显影 · 放大机印相 · 内容边缘吸附").font(crate::theme::medium(11.5)).color(t.text_dim));
    });
}

/// Shows a recent file selected in the system file manager; a failure goes to the status line, as the other open
/// errors do. The start screen offers Show in Folder only where the platform can reveal a file.
fn reveal(app: &mut PhotocraftApp, path: &std::path::Path) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(e) = craft::platform::open::reveal(path) {
        app.ui.status = e;
        app.ui.status_error = true;
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, path);
}

/// The links of the Home rail: Home, which leaves Home for the open documents, then Learn and the community pages.
fn links() -> [StartLink; 5] {
    [
        StartLink::command(tl!("Home"), Some(IconId::new("home")), "window.home"),
        StartLink::url(tl!("Learn"), Some(IconId::new("sparkles")), crate::links::TUTORIALS),
        StartLink::url(tl!("Website"), Some(IconId::new("link")), crate::links::APP_PAGE),
        StartLink::url(tl!("Discord"), Some(IconId::new("globe")), crate::links::DISCORD),
        StartLink::url(tl!("GitHub"), Some(IconId::new("globe")), crate::links::GITHUB),
    ]
}

/// Shows Home over the open documents, or leaves it for them: the rule of the options bar's Home button, the Home
/// rail's Home link and `window.home`. With no document open there is nothing behind Home, so it does nothing.
pub(crate) fn toggle_home(app: &mut PhotocraftApp) {
    let documents = app.session.documents().len();
    if documents == 0 {
        return;
    }
    let auto = app.session.prefs().general.auto_show_home_screen;
    let up = app.ui.chrome.shows_home(documents, auto);
    app.ui.chrome.home = if up { None } else { Some(documents) };
}
