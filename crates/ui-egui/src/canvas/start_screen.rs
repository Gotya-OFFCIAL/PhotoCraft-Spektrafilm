//! The Home screen: the framework start page with PhotoCraft's recent files, links and drop hint,
//! or PhotoCraft-Spektrafilm's warm charcoal 2-column darkroom start screen when `brand::is_spektra()`.

use craft::behavior::IconId;
use craft::ui::contract::{Bind, Show as _};
use craft::ui::display::Icon;
use craft::ui::primitives::{CheckerArea, IconSpec, Priority, TextureState, ThumbRequest, ThumbSource, Thumbnails, fit_rect, paint_surround};
use craft::ui::runtime::env::{Env, Label};
use craft::ui::shell::{RecentFile, RecentSort, RecentView, StartAction, StartLink, StartScreen, ThumbKey};
use craft::ui::style::{DataColor, SurroundLook};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use super::PhotocraftApp;

/// Recent files listed on the Home screen.
const HOME_RECENT: usize = 11;

/// Left darkroom sidebar width on desktop widths (`area.width() >= 720.0`).
const SIDEBAR_WIDTH: f32 = 212.0;

/// Grid card dimensions for the SpektraLab Home stage.
const SPEKTRA_CARD_W: f32 = 244.0;
const SPEKTRA_CARD_H: f32 = 196.0;
const SPEKTRA_PREVIEW_H: f32 = 132.0;
const SPEKTRA_CARD_GAP: f32 = 16.0;

/// The Home screen in the document area. The recent list is empty on the web, which has no paths to reopen, and with
/// Show Home Recent Files off.
pub(super) fn start_screen(app: &mut PhotocraftApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let show_recent = !cfg!(target_arch = "wasm32") && app.session.prefs().interface.show_home_recent_files;
    let recent = if show_recent { crate::home_recent_ui::recent_files(app, HOME_RECENT) } else { Vec::new() };
    let thumbs = crate::home_recent_ui::thumbnails(&ctx);
    // The source reads the app, so the page binds copies of the view and the sort and they are written back after.
    let (mut view, mut sort) = (app.ui.chrome.home_recent_view, app.ui.chrome.home_recent_sort);
    let actions = if crate::panels::brand::is_spektra() {
        render_spektra_start_screen(app, ui, &recent, &thumbs, &mut view, &mut sort)
    } else {
        let links = links();
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

/// Handcrafted SpektraLab warm darkroom 2-column Home screen.
pub(crate) fn render_spektra_start_screen(
    app: &mut PhotocraftApp,
    ui: &mut Ui,
    recent: &[RecentFile],
    thumbs: &Thumbnails<ThumbKey>,
    view: &mut RecentView,
    sort: &mut RecentSort,
) -> Vec<StartAction> {
    let area = ui.available_rect_before_wrap();
    let ctx = ui.ctx().clone();
    let env = Env::of(ui);
    let t = crate::theme::spektra_tokens(crate::theme::Tokens::get(&ctx));

    paint_surround(ui.painter(), CheckerArea::Rect(area, CornerRadius::ZERO), &SurroundLook::of(&env.tokens).shown(env.canvas_pattern));

    let collator = env.collator();
    let mut files: Vec<RecentFile> = recent.to_vec();
    if *sort == RecentSort::Name {
        files.sort_by(|a, b| collator.compare(&a.name, &b.name).then_with(|| a.path.cmp(&b.path)));
    }

    let has_open_docs = !app.session.documents().is_empty();
    let mut actions = Vec::new();
    let mut clicked_url: Option<&'static str> = None;
    let mut go_home = false;

    let wide = area.width() >= 720.0;
    let stage_rect = if wide {
        let rail_rect = Rect::from_min_size(area.min, vec2(SIDEBAR_WIDTH, area.height()));
        draw_spektra_sidebar(ui, rail_rect, &t, has_open_docs, &mut actions, &mut go_home, &mut clicked_url);
        Rect::from_min_max(pos2(rail_rect.right() + 1.0, area.top()), area.max)
    } else {
        let top_h: f32 = 104.0;
        let top_rect = Rect::from_min_size(area.min, vec2(area.width(), top_h.min(area.height())));
        draw_spektra_compact_top(ui, top_rect, &t, &mut actions);
        Rect::from_min_max(pos2(area.left(), (top_rect.bottom() + 1.0).min(area.bottom())), area.max)
    };

    let mut any_item_hovered = false;
    {
        let source = crate::home_recent_ui::source(app, &ctx);
        let pad_x = if wide { 36.0 } else { 18.0 };
        let pad_y = if wide { 28.0 } else { 16.0 };
        let mut stage_ui = ui.new_child(egui::UiBuilder::new().max_rect(stage_rect));
        egui::ScrollArea::vertical()
            .id_salt("spektra-home-stage-scroll")
            .auto_shrink([false, false])
            .show(&mut stage_ui, |ui| {
                let inner_w = (stage_rect.width() - pad_x * 2.0).max(240.0);
                let content_rect = Rect::from_min_size(
                    pos2(ui.cursor().left() + pad_x, ui.cursor().top() + pad_y),
                    vec2(inner_w, ui.available_height().max(1.0)),
                );
                let mut col_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(content_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                col_ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

                draw_spektra_stage_header(&mut col_ui, inner_w, &t);
                col_ui.add_space(20.0);

                draw_spektra_recent_header(&mut col_ui, inner_w, files.len(), view, sort, &t);
                col_ui.add_space(16.0);

                if files.is_empty() {
                    if draw_spektra_empty_card(&mut col_ui, inner_w, &t, &mut any_item_hovered) {
                        actions.push(StartAction::Open);
                    }
                } else if *view == RecentView::Grid {
                    draw_spektra_recent_grid(
                        &mut col_ui,
                        &ctx,
                        inner_w,
                        &files,
                        thumbs,
                        &source,
                        &t,
                        &mut actions,
                        &mut any_item_hovered,
                    );
                } else {
                    draw_spektra_recent_list(
                        &mut col_ui,
                        &ctx,
                        inner_w,
                        &files,
                        thumbs,
                        &source,
                        &t,
                        &mut actions,
                        &mut any_item_hovered,
                    );
                }

                if !wide {
                    col_ui.add_space(20.0);
                    draw_spektra_compact_links(&mut col_ui, inner_w, &t, &mut clicked_url);
                }
                col_ui.add_space(28.0);
                let used_h = col_ui.min_rect().bottom() - content_rect.top() + pad_y;
                ui.allocate_space(vec2(stage_rect.width(), used_h.max(1.0)));
            });
    }

    if go_home {
        toggle_home(app);
    }
    if let Some(url) = clicked_url {
        crate::links::open(app, &ctx, url);
    }
    if !any_item_hovered
        && ui.rect_contains_pointer(stage_rect)
        && ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary))
    {
        actions.push(StartAction::Open);
    }

    actions
}

fn sidebar_fill(t: &crate::theme::Tokens) -> Color32 {
    if t.dark() { Color32::from_rgb(34, 34, 33) } else { t.chrome }
}

fn primary_hover_color(t: &crate::theme::Tokens) -> Color32 {
    if t.dark() { Color32::from_rgb(235, 185, 117) } else { t.accent_border }
}

fn draw_spektra_sidebar(
    ui: &mut Ui,
    rail_rect: Rect,
    t: &crate::theme::Tokens,
    has_open_docs: bool,
    actions: &mut Vec<StartAction>,
    go_home: &mut bool,
    clicked_url: &mut Option<&'static str>,
) {
    ui.painter().rect_filled(rail_rect, CornerRadius::ZERO, sidebar_fill(t));
    ui.painter().line_segment(
        [pos2(rail_rect.right(), rail_rect.top()), pos2(rail_rect.right(), rail_rect.bottom())],
        Stroke::new(1.0, t.separator),
    );

    let inner = Rect::from_min_max(
        pos2(rail_rect.left() + 20.0, rail_rect.top() + 28.0),
        pos2(rail_rect.right() - 20.0, rail_rect.bottom() - 18.0),
    );
    let mut r_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    r_ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let w = inner.width().max(120.0);

    // Primary CTA: New file
    if draw_cta_button(
        &mut r_ui,
        vec2(w, 36.0),
        tl!("New file"),
        true,
        t,
    )
    .clicked()
    {
        actions.push(StartAction::New);
    }
    r_ui.add_space(8.0);

    // Secondary CTA: Open…
    if draw_cta_button(
        &mut r_ui,
        vec2(w, 34.0),
        tl!("Open…"),
        false,
        t,
    )
    .clicked()
    {
        actions.push(StartAction::Open);
    }

    r_ui.add_space(18.0);
    draw_hairline(&mut r_ui, w, t.separator);
    r_ui.add_space(14.0);

    // Nav: Home (active indicator bar)
    let home_resp = draw_nav_row(&mut r_ui, vec2(w, 34.0), IconId::new("house"), tl!("Home"), true, t, None);
    if has_open_docs && home_resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        *go_home = true;
    }

    r_ui.add_space(4.0);

    // Nav: Learn
    if draw_nav_row(
        &mut r_ui,
        vec2(w, 34.0),
        IconId::new("sparkles"),
        tl!("Learn"),
        false,
        t,
        Some(crate::links::TUTORIALS),
    )
    .clicked()
    {
        *clicked_url = Some(crate::links::TUTORIALS);
    }

    // Push optional darkroom card & bottom links cleanly to the bottom edge.
    let links_block_h = 108.0;
    let avail = r_ui.available_height();
    let show_spec_card = avail >= 210.0;
    if show_spec_card {
        let card_h = 84.0;
        let top_gap = (avail - links_block_h - card_h - 14.0).max(16.0);
        r_ui.add_space(top_gap);
        draw_darkroom_spec_card(&mut r_ui, w, t);
        r_ui.add_space(14.0);
    } else {
        let rem = (avail - links_block_h).max(12.0);
        r_ui.add_space(rem);
    }

    draw_hairline(&mut r_ui, w, t.separator);
    r_ui.add_space(10.0);

    for (label, icon, url) in [
        (tl!("Website"), IconId::new("link"), crate::links::APP_PAGE),
        (tl!("Discord"), IconId::new("message-square"), crate::links::DISCORD),
        (tl!("GitHub"), IconId::new("link-2"), crate::links::GITHUB),
    ] {
        if draw_link_row(&mut r_ui, vec2(w, 28.0), icon, label, url, t).clicked() {
            *clicked_url = Some(url);
        }
        r_ui.add_space(2.0);
    }
}

fn draw_spektra_compact_top(
    ui: &mut Ui,
    top_rect: Rect,
    t: &crate::theme::Tokens,
    actions: &mut Vec<StartAction>,
) {
    ui.painter().rect_filled(top_rect, CornerRadius::ZERO, sidebar_fill(t));
    ui.painter().line_segment(
        [pos2(top_rect.left(), top_rect.bottom()), pos2(top_rect.right(), top_rect.bottom())],
        Stroke::new(1.0, t.separator),
    );
    let inner = top_rect.shrink2(vec2(18.0, 12.0));
    let mut c_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    c_ui.vertical(|ui| {
        ui.horizontal(|ui| {
            let badge_r = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::hover()).0;
            ui.painter().rect_filled(badge_r, CornerRadius::same(4), t.primary_bg);
            ui.painter().text(badge_r.center(), Align2::CENTER_CENTER, "01", crate::theme::semibold(10.5), t.primary_text);
            ui.add_space(8.0);
            ui.label(egui::RichText::new(crate::panels::app_name()).font(crate::theme::semibold(16.0)).color(t.text));
        });
        ui.add_space(10.0);
        let btn_w = ((inner.width() - 10.0) * 0.5).clamp(90.0, 170.0);
        ui.horizontal(|ui| {
            if draw_cta_button(ui, vec2(btn_w, 32.0), tl!("New file"), true, t).clicked() {
                actions.push(StartAction::New);
            }
            ui.add_space(10.0);
            if draw_cta_button(ui, vec2(btn_w, 32.0), tl!("Open…"), false, t).clicked() {
                actions.push(StartAction::Open);
            }
        });
    });
}

fn draw_spektra_compact_links(
    ui: &mut Ui,
    inner_w: f32,
    t: &crate::theme::Tokens,
    clicked_url: &mut Option<&'static str>,
) {
    draw_hairline(ui, inner_w, t.separator);
    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        for (label, icon, url) in [
            (tl!("Learn"), IconId::new("sparkles"), crate::links::TUTORIALS),
            (tl!("Website"), IconId::new("link"), crate::links::APP_PAGE),
            (tl!("Discord"), IconId::new("message-square"), crate::links::DISCORD),
            (tl!("GitHub"), IconId::new("link-2"), crate::links::GITHUB),
        ] {
            if draw_link_row(ui, vec2(96.0, 28.0), icon, label, url, t).clicked() {
                *clicked_url = Some(url);
            }
        }
    });
}

fn draw_cta_button(
    ui: &mut Ui,
    size: Vec2,
    label: &str,
    primary: bool,
    t: &crate::theme::Tokens,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hovered = resp.hovered();
    if primary {
        let fill = if hovered { primary_hover_color(t) } else { t.primary_bg };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        let text = if label.starts_with('+') || label.starts_with('＋') {
            label.to_owned()
        } else {
            format!("+ {label}")
        };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, crate::theme::semibold(13.0), t.primary_text);
    } else {
        let fill = if hovered { t.hover } else { t.card };
        let stroke = if hovered { t.accent_border } else { t.field_border };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, stroke), StrokeKind::Inside);
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, crate::theme::semibold(13.0), t.text);
    }
    resp
}

fn draw_nav_row(
    ui: &mut Ui,
    size: Vec2,
    icon: IconId,
    label: &str,
    active: bool,
    t: &crate::theme::Tokens,
    tooltip: Option<&str>,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let mut resp = resp;
    if let Some(tip) = tooltip {
        resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip);
    }
    let hovered = resp.hovered();
    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(6), t.accent_soft);
        let bar = Rect::from_min_size(pos2(rect.left() + 4.0, rect.center().y - 8.0), vec2(2.5, 16.0));
        ui.painter().rect_filled(bar, CornerRadius::same(1), t.accent);
    } else if hovered {
        ui.painter().rect_filled(rect, CornerRadius::same(6), t.hover);
    }
    let fg = if active || hovered { t.text } else { t.text_dim };
    let icon_col = if active { t.accent } else { fg };
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(16.0, 16.0));
    paint_svg_icon(ui, icon_rect, icon, icon_col);
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        if active { crate::theme::semibold(13.0) } else { crate::theme::medium(13.0) },
        fg,
    );
    resp
}

fn draw_link_row(
    ui: &mut Ui,
    size: Vec2,
    icon: IconId,
    label: &str,
    url: &str,
    t: &crate::theme::Tokens,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(url);
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(5), t.hover);
    }
    let fg = if resp.hovered() { t.text } else { t.text_dim };
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(14.0, 14.0));
    paint_svg_icon(ui, icon_rect, icon, fg);
    ui.painter().text(pos2(rect.left() + 34.0, rect.center().y), Align2::LEFT_CENTER, label, crate::theme::medium(12.0), fg);
    resp
}

fn draw_darkroom_spec_card(ui: &mut Ui, width: f32, t: &crate::theme::Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 84.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(8), t.field);
    ui.painter().rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, t.separator), StrokeKind::Inside);

    ui.painter().circle_filled(pos2(rect.left() + 15.0, rect.top() + 19.0), 3.5, t.accent);
    ui.painter().text(
        pos2(rect.left() + 24.0, rect.top() + 19.0),
        Align2::LEFT_CENTER,
        "81 波长光谱暗房",
        crate::theme::semibold(11.0),
        if t.dark() { t.accent } else { t.accent_text },
    );
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 43.0),
        Align2::LEFT_CENTER,
        "22 款胶片 · 6 款暗房相纸",
        egui::FontId::proportional(10.5),
        t.text_dim,
    );
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 63.0),
        Align2::LEFT_CENTER,
        "TH-KG3 印相 · 内容边缘吸附",
        egui::FontId::proportional(10.5),
        t.text_faint,
    );
}

fn draw_spektra_stage_header(ui: &mut Ui, inner_w: f32, t: &crate::theme::Tokens) {
    let subtitle = "81-Wavelength Spectral Film & Darkroom Studio · 光谱胶片暗房工作站";
    let accent_fg = if t.dark() { t.accent } else { t.accent_text };

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 6.0);
        let (badge_rect, _) = ui.allocate_exact_size(vec2(28.0, 22.0), Sense::hover());
        ui.painter().rect_filled(badge_rect, CornerRadius::same(4), t.primary_bg);
        ui.painter().text(badge_rect.center(), Align2::CENTER_CENTER, "01", crate::theme::semibold(11.5), t.primary_text);

        let title_text = crate::panels::app_name();
        let title_galley = ui.painter().layout_no_wrap(title_text.to_owned(), crate::theme::semibold(21.0), t.text);
        let (title_r, _) = ui.allocate_exact_size(vec2(title_galley.size().x, 24.0), Sense::hover());
        ui.painter().galley(pos2(title_r.left(), title_r.center().y - title_galley.size().y * 0.5), title_galley, t.text);

        let sub_galley = ui.painter().layout_no_wrap(subtitle.to_owned(), crate::theme::medium(11.5), accent_fg);
        let pill_w = (sub_galley.size().x + 18.0).min(inner_w);
        let (sub_r, _) = ui.allocate_exact_size(vec2(pill_w, 22.0), Sense::hover());
        ui.painter().rect_filled(sub_r, CornerRadius::same(99), t.accent_soft);
        ui.painter().rect_stroke(sub_r, CornerRadius::same(99), Stroke::new(1.0, t.accent_border), StrokeKind::Inside);
        let text_galley = elide_text(ui, subtitle, crate::theme::medium(11.0), accent_fg, pill_w - 14.0);
        ui.painter().galley(
            pos2(sub_r.center().x - text_galley.size().x * 0.5, sub_r.center().y - text_galley.size().y * 0.5),
            text_galley,
            accent_fg,
        );
    });

    ui.add_space(16.0);
    draw_hairline(ui, inner_w, t.separator);
}

fn draw_spektra_recent_header(
    ui: &mut Ui,
    inner_w: f32,
    count: usize,
    view: &mut RecentView,
    sort: &mut RecentSort,
    t: &crate::theme::Tokens,
) {
    let (row_rect, _) = ui.allocate_exact_size(vec2(inner_w, 30.0), Sense::hover());
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row_ui.spacing_mut().item_spacing.x = 8.0;

    row_ui.label(egui::RichText::new("最近使用").font(crate::theme::semibold(16.0)).color(t.text));
    if count > 0 {
        let count_text = format!("{count} 项");
        let cg = row_ui.painter().layout_no_wrap(count_text.clone(), crate::theme::mono(10.5), t.text_dim);
        let (cr, _) = row_ui.allocate_exact_size(vec2(cg.size().x + 14.0, 20.0), Sense::hover());
        row_ui.painter().rect_filled(cr, CornerRadius::same(99), t.card);
        row_ui.painter().rect_stroke(cr, CornerRadius::same(99), Stroke::new(1.0, t.card_border), StrokeKind::Inside);
        row_ui.painter().text(cr.center(), Align2::CENTER_CENTER, count_text, crate::theme::mono(10.5), t.text_dim);
    }

    row_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        // Sort segmented pill
        let sort_w = 136.0;
        let (sort_r, _) = ui.allocate_exact_size(vec2(sort_w, 26.0), Sense::hover());
        ui.painter().rect_filled(sort_r, CornerRadius::same(6), t.field);
        ui.painter().rect_stroke(sort_r, CornerRadius::same(6), Stroke::new(1.0, t.field_border), StrokeKind::Inside);
        // Left slot: 最近打开, Right slot: 按名称 (regardless of RTL child layout above)
        let seg_w = (sort_w - 4.0) * 0.5;
        for (idx, (val, label)) in [(RecentSort::Opened, "最近打开"), (RecentSort::Name, "按名称")].into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(sort_r.left() + 2.0 + idx as f32 * seg_w, sort_r.top() + 2.0), vec2(seg_w, 22.0));
            let active = *sort == val || (val == RecentSort::Opened && *sort == RecentSort::Created);
            let resp = ui
                .interact(r, ui.scope_id().with(("spektra-sort", idx)), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, active, label));
            if active {
                ui.painter().rect_filled(r, CornerRadius::same(4), t.accent_soft);
                ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.accent_border), StrokeKind::Inside);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
            }
            let fg = if active { if t.dark() { t.accent } else { t.accent_text } } else if resp.hovered() { t.text } else { t.text_dim };
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, crate::theme::medium(11.0), fg);
            if resp.clicked() {
                *sort = val;
            }
        }

        // View segmented toggle (Grid / List)
        let (view_r, _) = ui.allocate_exact_size(vec2(62.0, 26.0), Sense::hover());
        ui.painter().rect_filled(view_r, CornerRadius::same(6), t.field);
        ui.painter().rect_stroke(view_r, CornerRadius::same(6), Stroke::new(1.0, t.field_border), StrokeKind::Inside);
        let grid_r = Rect::from_min_size(view_r.min + vec2(2.0, 2.0), vec2(28.0, 22.0));
        let list_r = Rect::from_min_size(pos2(view_r.left() + 32.0, view_r.top() + 2.0), vec2(28.0, 22.0));

        let g_resp = ui
            .interact(grid_r, ui.scope_id().with("spektra-view-grid"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("网格视图");
        g_resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, *view == RecentView::Grid, tl!("Grid view")));

        let l_resp = ui
            .interact(list_r, ui.scope_id().with("spektra-view-list"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("列表视图");
        l_resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, *view == RecentView::List, tl!("List view")));

        for (r, active, hovered, icon) in [
            (grid_r, *view == RecentView::Grid, g_resp.hovered(), IconId::new("grid-2x2")),
            (list_r, *view == RecentView::List, l_resp.hovered(), IconId::new("list")),
        ] {
            if active {
                ui.painter().rect_filled(r, CornerRadius::same(4), t.accent_soft);
                ui.painter().rect_stroke(r, CornerRadius::same(4), Stroke::new(1.0, t.accent_border), StrokeKind::Inside);
            } else if hovered {
                ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
            }
            let tint = if active { if t.dark() { t.accent } else { t.accent_text } } else if hovered { t.text } else { t.text_dim };
            paint_svg_icon(ui, r.shrink(3.0), icon, tint);
        }

        if g_resp.clicked() {
            *view = RecentView::Grid;
        }
        if l_resp.clicked() {
            *view = RecentView::List;
        }
    });
}

fn draw_spektra_recent_grid(
    ui: &mut Ui,
    ctx: &egui::Context,
    inner_w: f32,
    files: &[RecentFile],
    thumbs: &Thumbnails<ThumbKey>,
    source: &dyn ThumbSource<ThumbKey>,
    t: &crate::theme::Tokens,
    actions: &mut Vec<StartAction>,
    any_hovered: &mut bool,
) {
    let cols = ((inner_w + SPEKTRA_CARD_GAP) / (SPEKTRA_CARD_W + SPEKTRA_CARD_GAP)).floor().max(1.0) as usize;
    let total_cards = files.len() + 1; // +1 for inline open card
    let rows = total_cards.div_ceil(cols);

    for row in 0..rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPEKTRA_CARD_GAP;
            for col in 0..cols {
                let idx = row * cols + col;
                if idx < files.len() {
                    draw_spektra_grid_card(ui, ctx, &files[idx], thumbs, source, t, actions, any_hovered);
                } else if idx == files.len() {
                    if draw_spektra_drop_card(ui, t, any_hovered) {
                        actions.push(StartAction::Open);
                    }
                }
            }
        });
        if row + 1 < rows {
            ui.add_space(SPEKTRA_CARD_GAP);
        }
    }
}

fn draw_spektra_grid_card(
    ui: &mut Ui,
    ctx: &egui::Context,
    file: &RecentFile,
    thumbs: &Thumbnails<ThumbKey>,
    source: &dyn ThumbSource<ThumbKey>,
    t: &crate::theme::Tokens,
    actions: &mut Vec<StartAction>,
    any_hovered: &mut bool,
) {
    let (card_rect, resp) = ui.allocate_exact_size(vec2(SPEKTRA_CARD_W, SPEKTRA_CARD_H), Sense::click());
    let hovered = ui.rect_contains_pointer(card_rect);
    if hovered {
        *any_hovered = true;
    }

    ui.painter().rect_filled(card_rect, CornerRadius::same(8), t.card);
    ui.painter().rect_stroke(
        card_rect,
        CornerRadius::same(8),
        Stroke::new(1.0, if hovered { t.accent } else { t.card_border }),
        StrokeKind::Inside,
    );

    // Preview well
    let preview_rect = Rect::from_min_size(card_rect.min + vec2(1.0, 1.0), vec2(SPEKTRA_CARD_W - 2.0, SPEKTRA_PREVIEW_H));
    ui.painter().rect_filled(
        preview_rect,
        CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 },
        t.field,
    );
    ui.painter().line_segment(
        [pos2(preview_rect.left(), preview_rect.bottom()), pos2(preview_rect.right(), preview_rect.bottom())],
        Stroke::new(1.0, t.separator),
    );

    let key = ThumbKey::Recent(file.path.clone());
    let req = ThumbRequest::for_cell(280.0, ctx.pixels_per_point());
    let thumb_state = thumbs.state(ctx, source, &key, &req, Priority::VISIBLE);
    let (ext_label, is_raw) = file_badge(&file.path);

    match thumb_state {
        TextureState::Ready(tex) | TextureState::Loading { stand_in: Some(tex), .. } => {
            let bounds = preview_rect.shrink(6.0);
            let dest = fit_rect(bounds, tex.size_vec2());
            ui.painter().image(tex.id, dest, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            ui.painter().rect_stroke(dest, CornerRadius::same(2), Stroke::new(1.0, Color32::from_white_alpha(14)), StrokeKind::Inside);
        }
        _ => {
            draw_film_frame_placeholder(ui.painter(), preview_rect, &ext_label, is_raw, t);
        }
    }

    // Format badge pill at top-left
    draw_format_badge(ui.painter(), pos2(preview_rect.left() + 8.0, preview_rect.top() + 8.0), &ext_label, is_raw, t);

    // Hover 'x' close button at top-right
    let mut remove_clicked = false;
    if hovered {
        let close_rect = Rect::from_center_size(pos2(card_rect.right() - 16.0, card_rect.top() + 16.0), vec2(22.0, 22.0));
        let x_resp = ui
            .interact(close_rect, resp.id.with("spektra-remove"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("从最近列表中移除");
        x_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &format!("从最近列表中移除 {}", file.name)));
        ui.painter().circle_filled(
            close_rect.center(),
            10.5,
            if x_resp.hovered() { Color32::from_black_alpha(235) } else { Color32::from_black_alpha(185) },
        );
        ui.painter().circle_stroke(close_rect.center(), 10.5, Stroke::new(1.0, Color32::from_white_alpha(32)));
        paint_svg_icon(ui, close_rect.shrink(4.0), IconId::new("x"), if x_resp.hovered() { Color32::WHITE } else { t.text_dim });
        if x_resp.clicked() {
            remove_clicked = true;
            actions.push(StartAction::RemoveRecent(file.path.clone()));
        }
    }

    // Bottom info footer
    let info_top = preview_rect.bottom();
    let text_x = card_rect.left() + 12.0;
    let text_max_w = (SPEKTRA_CARD_W - 24.0).max(20.0);

    let name_galley = elide_text(ui, &file.name, crate::theme::semibold(13.0), t.text, text_max_w);
    ui.painter().galley(pos2(text_x, info_top + 11.0), name_galley, t.text);

    let folder_str = file
        .path
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "本地文件".to_owned());
    let folder_galley = elide_text(ui, &folder_str, egui::FontId::proportional(11.0), t.text_faint, text_max_w);
    ui.painter().galley(pos2(text_x, info_top + 35.0), folder_galley, t.text_faint);

    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &file.name));
    let full_tip = file.path.display().to_string();
    let resp = resp.on_hover_text(full_tip).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() && !remove_clicked {
        actions.push(StartAction::OpenRecent(file.path.clone()));
    }
    attach_recent_context_menu(&resp, &file.path, actions);
}

fn draw_spektra_drop_card(ui: &mut Ui, t: &crate::theme::Tokens, any_hovered: &mut bool) -> bool {
    let (card_rect, resp) = ui.allocate_exact_size(vec2(SPEKTRA_CARD_W, SPEKTRA_CARD_H), Sense::click());
    let hovered = ui.rect_contains_pointer(card_rect);
    if hovered {
        *any_hovered = true;
    }
    let fill = if hovered { t.hover } else { Color32::from_white_alpha(4) };
    let border = if hovered { t.accent_border } else { t.separator };
    ui.painter().rect_filled(card_rect, CornerRadius::same(8), fill);
    ui.painter().rect_stroke(card_rect, CornerRadius::same(8), Stroke::new(1.0, border), StrokeKind::Inside);

    let icon_plate = Rect::from_center_size(pos2(card_rect.center().x, card_rect.center().y - 18.0), vec2(44.0, 36.0));
    ui.painter().rect_filled(icon_plate, CornerRadius::same(6), t.field);
    ui.painter().rect_stroke(icon_plate, CornerRadius::same(6), Stroke::new(1.0, if hovered { t.accent_border } else { t.card_border }), StrokeKind::Inside);
    paint_svg_icon(ui, icon_plate.shrink(7.0), IconId::new("image"), if hovered { t.accent } else { t.text_dim });

    let label = "点击或拖入图片 / RAW / PSD 打开";
    let gal = elide_text(
        ui,
        label,
        crate::theme::medium(12.0),
        if hovered { t.text } else { t.text_dim },
        SPEKTRA_CARD_W - 28.0,
    );
    ui.painter().galley(
        pos2(card_rect.center().x - gal.size().x * 0.5, card_rect.center().y + 14.0),
        gal,
        if hovered { t.text } else { t.text_dim },
    );

    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Open…")));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tl!("Open…")).clicked()
}

fn draw_spektra_recent_list(
    ui: &mut Ui,
    ctx: &egui::Context,
    inner_w: f32,
    files: &[RecentFile],
    thumbs: &Thumbnails<ThumbKey>,
    source: &dyn ThumbSource<ThumbKey>,
    t: &crate::theme::Tokens,
    actions: &mut Vec<StartAction>,
    any_hovered: &mut bool,
) {
    for (idx, file) in files.iter().enumerate() {
        let (row_rect, resp) = ui.allocate_exact_size(vec2(inner_w, 44.0), Sense::click());
        let hovered = ui.rect_contains_pointer(row_rect);
        if hovered {
            *any_hovered = true;
            ui.painter().rect_filled(row_rect, CornerRadius::same(6), t.hover);
        }

        let thumb_box = Rect::from_center_size(pos2(row_rect.left() + 28.0, row_rect.center().y), vec2(40.0, 30.0));
        ui.painter().rect_filled(thumb_box, CornerRadius::same(4), t.field);
        ui.painter().rect_stroke(thumb_box, CornerRadius::same(4), Stroke::new(1.0, t.card_border), StrokeKind::Inside);

        let key = ThumbKey::Recent(file.path.clone());
        let req = ThumbRequest::for_cell(80.0, ctx.pixels_per_point());
        let (ext_label, is_raw) = file_badge(&file.path);
        match thumbs.state(ctx, source, &key, &req, Priority::VISIBLE) {
            TextureState::Ready(tex) | TextureState::Loading { stand_in: Some(tex), .. } => {
                let dest = fit_rect(thumb_box.shrink(2.0), tex.size_vec2());
                ui.painter().image(tex.id, dest, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            _ => {
                ui.painter().text(
                    thumb_box.center(),
                    Align2::CENTER_CENTER,
                    &ext_label,
                    crate::theme::mono(8.5),
                    if is_raw { t.accent } else { t.text_dim },
                );
            }
        }

        // Format badge pill beside thumbnail
        let pill_r = draw_format_badge(ui.painter(), pos2(thumb_box.right() + 10.0, row_rect.center().y - 8.5), &ext_label, is_raw, t);

        let close_rect = Rect::from_center_size(pos2(row_rect.right() - 18.0, row_rect.center().y), vec2(22.0, 22.0));
        let mut remove_clicked = false;
        let right_limit = if hovered {
            let x_resp = ui
                .interact(close_rect, resp.id.with("spektra-list-remove"), Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("从最近列表中移除");
            x_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &format!("从最近列表中移除 {}", file.name)));
            if x_resp.hovered() {
                ui.painter().rect_filled(close_rect, CornerRadius::same(4), t.pressed);
            }
            paint_svg_icon(ui, close_rect.shrink(4.0), IconId::new("x"), if x_resp.hovered() { t.text } else { t.text_dim });
            if x_resp.clicked() {
                remove_clicked = true;
                actions.push(StartAction::RemoveRecent(file.path.clone()));
            }
            close_rect.left() - 10.0
        } else {
            row_rect.right() - 12.0
        };

        let name_x = pill_r.right() + 10.0;
        let name_max_w = ((right_limit - name_x) * 0.55).max(80.0);
        let name_galley = elide_text(ui, &file.name, crate::theme::semibold(13.0), t.text, name_max_w);
        let name_w = name_galley.size().x;
        ui.painter().galley(pos2(name_x, row_rect.center().y - name_galley.size().y * 0.5), name_galley, t.text);

        let folder_max = (right_limit - (name_x + name_w + 16.0)).max(0.0);
        if folder_max > 28.0 {
            let folder = file.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            if !folder.is_empty() {
                let fg = elide_text(ui, &folder, egui::FontId::proportional(11.5), t.text_faint, folder_max);
                ui.painter().galley(pos2(right_limit - fg.size().x, row_rect.center().y - fg.size().y * 0.5), fg, t.text_faint);
            }
        }

        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &file.name));
        let resp = resp.on_hover_text(file.path.display().to_string()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() && !remove_clicked {
            actions.push(StartAction::OpenRecent(file.path.clone()));
        }
        attach_recent_context_menu(&resp, &file.path, actions);

        if idx + 1 < files.len() {
            ui.add_space(2.0);
        }
    }
}

fn draw_spektra_empty_card(
    ui: &mut Ui,
    inner_w: f32,
    t: &crate::theme::Tokens,
    any_hovered: &mut bool,
) -> bool {
    let card_w = inner_w.min(560.0);
    let card_h = 210.0;
    let pad_x = ((inner_w - card_w) * 0.5).max(0.0);
    ui.add_space(36.0);
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(pad_x);
        let (rect, resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::click());
        let hovered = ui.rect_contains_pointer(rect);
        if hovered {
            *any_hovered = true;
        }
        ui.painter().rect_filled(rect, CornerRadius::same(10), if hovered { t.card } else { Color32::from_white_alpha(4) });
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(10),
            Stroke::new(1.0, if hovered { t.accent_border } else { t.separator }),
            StrokeKind::Inside,
        );

        let plate = Rect::from_center_size(pos2(rect.center().x, rect.center().y - 28.0), vec2(56.0, 44.0));
        ui.painter().rect_filled(plate, CornerRadius::same(8), t.field);
        ui.painter().rect_stroke(plate, CornerRadius::same(8), Stroke::new(1.0, if hovered { t.accent_border } else { t.card_border }), StrokeKind::Inside);
        paint_svg_icon(ui, plate.shrink(10.0), IconId::new("image"), if hovered { t.accent } else { t.text_dim });

        ui.painter().text(
            pos2(rect.center().x, rect.center().y + 12.0),
            Align2::CENTER_CENTER,
            "拖入任意图片、RAW 照片或 PSD 文件",
            crate::theme::semibold(14.0),
            if hovered { t.text } else { t.text_dim },
        );
        ui.painter().text(
            pos2(rect.center().x, rect.center().y + 36.0),
            Align2::CENTER_CENTER,
            "支持 NEF · RW2 · ARW · CR2 · DNG · PSD · PNG · JPG，点击直接浏览打开",
            egui::FontId::proportional(11.5),
            t.text_faint,
        );

        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tl!("Open…")));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tl!("Open…")).clicked() {
            clicked = true;
        }
    });
    clicked
}

fn draw_film_frame_placeholder(painter: &egui::Painter, preview_rect: Rect, ext_label: &str, is_raw: bool, t: &crate::theme::Tokens) {
    let slide = Rect::from_center_size(preview_rect.center(), vec2(84.0, 56.0));
    painter.rect_filled(slide, CornerRadius::same(4), if t.dark() { Color32::from_rgb(24, 24, 23) } else { t.card });
    painter.rect_stroke(
        slide,
        CornerRadius::same(4),
        Stroke::new(1.0, if is_raw { t.accent_border } else { t.separator }),
        StrokeKind::Inside,
    );

    // Subtle 35mm film sprocket holes along top and bottom borders
    for sx in [10.0, 28.0, 46.0, 64.0] {
        let top_hole = Rect::from_min_size(pos2(slide.left() + sx, slide.top() + 4.0), vec2(8.0, 3.0));
        let bot_hole = Rect::from_min_size(pos2(slide.left() + sx, slide.bottom() - 7.0), vec2(8.0, 3.0));
        painter.rect_filled(top_hole, CornerRadius::same(1), t.separator);
        painter.rect_filled(bot_hole, CornerRadius::same(1), t.separator);
    }

    let badge_title = if is_raw { format!("RAW · {ext_label}") } else { ext_label.to_owned() };
    painter.text(
        pos2(slide.center().x, slide.center().y - 4.0),
        Align2::CENTER_CENTER,
        badge_title,
        crate::theme::semibold(11.5),
        if is_raw { if t.dark() { t.accent } else { t.accent_text } } else { t.text_dim },
    );
    painter.text(
        pos2(slide.center().x, slide.center().y + 11.0),
        Align2::CENTER_CENTER,
        "无缩略图预览",
        egui::FontId::proportional(9.5),
        t.text_faint,
    );
}

fn draw_format_badge(painter: &egui::Painter, min: Pos2, ext_label: &str, is_raw: bool, t: &crate::theme::Tokens) -> Rect {
    let font = crate::theme::mono(9.0);
    let gal = painter.layout_no_wrap(ext_label.to_owned(), font.clone(), Color32::WHITE);
    let badge_w = (gal.size().x + 12.0).clamp(28.0, 56.0);
    let rect = Rect::from_min_size(min, vec2(badge_w, 17.0));
    let (fill, stroke, fg) = if is_raw {
        (
            Color32::from_rgba_unmultiplied(24, 20, 16, 224),
            t.accent_border,
            if t.dark() { t.accent } else { t.accent_text },
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(24, 24, 23, 215),
            t.separator,
            t.text_dim,
        )
    };
    painter.rect_filled(rect, CornerRadius::same(4), fill);
    painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, ext_label, font, fg);
    rect
}

fn attach_recent_context_menu(resp: &Response, path: &std::path::Path, actions: &mut Vec<StartAction>) {
    resp.context_menu(|ui| {
        if ui.button("打开").clicked() {
            actions.push(StartAction::OpenRecent(path.to_path_buf()));
            ui.close();
        }
        if craft::platform::open::CAN_REVEAL && ui.button("在文件夹中显示").clicked() {
            actions.push(StartAction::RevealRecent(path.to_path_buf()));
            ui.close();
        }
        ui.separator();
        if ui.button(tl!("Remove from Recent")).clicked() {
            actions.push(StartAction::RemoveRecent(path.to_path_buf()));
            ui.close();
        }
        if ui.button(tl!("Clear Recent Files")).clicked() {
            actions.push(StartAction::ClearRecent);
            ui.close();
        }
    });
}

fn paint_svg_icon(ui: &mut Ui, rect: Rect, id: IconId, color: Color32) {
    let mut icon_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    let _ = Icon::new(IconSpec::svg(id).data_color(DataColor::new(color))).show(&mut icon_ui);
}

fn draw_hairline(ui: &mut Ui, width: f32, color: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, color));
}

fn elide_text(ui: &Ui, text: &str, font: egui::FontId, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_w.max(8.0));
    ui.painter().layout_job(job)
}

fn file_badge(path: &std::path::Path) -> (String, bool) {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let is_raw = !ext.is_empty() && photocraft_io::raw::EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e));
    let label = if ext.is_empty() { "FILE".to_owned() } else { ext.to_ascii_uppercase() };
    (label, is_raw)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::framework::BuildApp;
    use egui_kittest::kittest::Queryable;
    use serde_json::{Value, json};

    fn deny_file(id: &str, _: &Value) -> photocraft_engine::Result<()> {
        if id.starts_with("file.") && !matches!(id, "file.new" | "file.close" | "file.closeAll" | "file.closeOthers") {
            Err(photocraft_engine::EngineError::Other(format!("automation command `{id}` is disabled")))
        } else {
            Ok(())
        }
    }

    fn dispatch_spektra_actions(app: &mut PhotocraftApp, actions: Vec<StartAction>) {
        for action in actions {
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

    fn spektra_preview_app_theme() -> crate::theme::AppTheme {
        if let Some(def) = crate::theme::spektra_theme_def() {
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
        }
    }

    #[test]
    fn spektra_home_screen_renders_two_columns_switches_view_and_enforces_recent_gate() {
        let services = crate::Services {
            automation_authorize: Some(deny_file),
            automation_command: Some(Box::new(|id, p| deny_file(id, p).map_err(|e| e.to_string()))),
            ..Default::default()
        };
        let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
        app.background_jobs = false;
        app.push_recent(r"G:\Photos\kodak_portra_400.jpg");
        app.push_recent(r"G:\Photos\P1340690.RW2");
        app.push_recent(r"G:\Photos\DSC_9042.NEF");

        let simulate_automation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sim_flag = std::sync::Arc::clone(&simulate_automation);
        PhotocraftApp::setup_context(&app.ctx().clone(), spektra_preview_app_theme());
        let mut h = egui_kittest::Harness::builder()
            .allow_missing_glyphs()
            .with_size(vec2(1440.0, 900.0))
            .build_app(
                move |ui, app| {
                    if sim_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        app.automation_input = true;
                    }
                    let ctx = ui.ctx().clone();
                    let recent = crate::home_recent_ui::recent_files(app, HOME_RECENT);
                    let thumbs = crate::home_recent_ui::thumbnails(&ctx);
                    let mut view = app.ui.chrome.home_recent_view;
                    let mut sort = app.ui.chrome.home_recent_sort;
                    let actions = render_spektra_start_screen(app, ui, &recent, &thumbs, &mut view, &mut sort);
                    app.ui.chrome.home_recent_view = view;
                    app.ui.chrome.home_recent_sort = sort;
                    dispatch_spektra_actions(app, actions);
                },
                app,
            );
        h.run_steps(4);
        assert_eq!(h.state().ui.chrome.home_recent_view, RecentView::Grid);
        assert_eq!(h.state().ui.chrome.home_recent_sort, RecentSort::Opened);

        // Toggle List view, then Grid view, then Name sort.
        h.get_by_label(tl!("List view")).click();
        h.run_steps(2);
        assert_eq!(h.state().ui.chrome.home_recent_view, RecentView::List);

        h.get_by_label("按名称").click();
        h.run_steps(2);
        assert_eq!(h.state().ui.chrome.home_recent_sort, RecentSort::Name);

        h.get_by_label(tl!("Grid view")).click();
        h.run_steps(2);
        assert_eq!(h.state().ui.chrome.home_recent_view, RecentView::Grid);

        // Synthetic input on a recent file card is gated as `file.openRecent.*`.
        h.get_by_label("DSC_9042.NEF").click();
        simulate_automation.store(true, std::sync::atomic::Ordering::Relaxed);
        h.step();
        simulate_automation.store(false, std::sync::atomic::Ordering::Relaxed);
        h.run_steps(2);
        assert!(h.state().session.documents().is_empty());
        assert!(h.state().ui.status.contains("file.openRecent."), "unexpected status: {}", h.state().ui.status);
    }

    #[test]
    #[ignore = "Manual visual snapshot generator (writes PNGs to F:\\spektra-ui-preview)"]
    fn dump_spektra_ui_previews() {
        let preview_dir = std::path::PathBuf::from(r"F:\spektra-ui-preview");
        let fixtures_dir = preview_dir.join("fixtures");
        std::fs::create_dir_all(&fixtures_dir).unwrap();
        let mut snap_results = egui_kittest::SnapshotResults::new();

        // 1. Sample colorful PNG thumbnail fixture
        let png_path = fixtures_dir.join("kodak_portra_400.png");
        {
            let mut rgba = Vec::with_capacity(320 * 213 * 4);
            for y in 0..213u32 {
                for x in 0..320u32 {
                    let r = ((x * 220 / 320) + 30).min(255) as u8;
                    let g = ((y * 165 / 213) + 55).min(255) as u8;
                    let b = (190u32.saturating_sub(x * 110 / 320)) as u8;
                    rgba.extend_from_slice(&[r, g, b, 255]);
                }
            }
            let img = photocraft_codecs::Image::from_raw(320, 213, photocraft_codecs::ChannelLayout::Rgba, photocraft_codecs::SampleType::U8, rgba).unwrap();
            let png = photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &photocraft_codecs::EncodeOptions::default()).unwrap();
            std::fs::write(&png_path, png).unwrap();
        }

        // 2. Sample synthetic Canon CR3 with embedded JPEG preview fixture
        let cr3_path = fixtures_dir.join("EOS_R5_Sample.CR3");
        {
            let mut rgb = Vec::with_capacity(320 * 213 * 3);
            for y in 0..213u32 {
                for x in 0..320u32 {
                    let r = (225u32.saturating_sub(y * 120 / 213)) as u8;
                    let g = ((x * 130 / 320) + 70).min(255) as u8;
                    let b = ((y * 180 / 213) + 45).min(255) as u8;
                    rgb.extend_from_slice(&[r, g, b]);
                }
            }
            let img = photocraft_codecs::Image::from_raw(320, 213, photocraft_codecs::ChannelLayout::Rgb, photocraft_codecs::SampleType::U8, rgb).unwrap();
            let jpeg = photocraft_codecs::encode(&img, photocraft_codecs::Format::Jpeg, &photocraft_codecs::EncodeOptions::default()).unwrap();
            std::fs::write(&cr3_path, photocraft_raw::testgen::cr3_preview(&jpeg, 1)).unwrap();
        }

        // Render Spektra Home Screen preview
        {
            let theme = spektra_preview_app_theme();
            let theme_for_home = theme.clone();
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), crate::Services::default());
            app.session.edit_prefs(|p| p.interface.language = "zh-hans".into());
            app.ui.theme = theme.clone();
            app.push_recent(r"G:\Photos\DSC_9042.NEF");
            app.push_recent(r"G:\Photos\P1340690.RW2");
            app.push_recent(&cr3_path.to_string_lossy());
            app.push_recent(&png_path.to_string_lossy());

            let ctx = app.ctx().clone();
            PhotocraftApp::setup_context(&ctx, theme);
            crate::i18n::sync_context(&ctx, "zh-hans");

            let mut h = egui_kittest::Harness::builder()
                .allow_missing_glyphs()
                .with_size(vec2(1440.0, 900.0))
                .build_app(
                    move |ui, app| {
                        let ctx = ui.ctx().clone();
                        app.ui.theme = theme_for_home.clone();
                        crate::i18n::sync_context(&ctx, "zh-hans");
                        let recent = crate::home_recent_ui::recent_files(app, HOME_RECENT);
                        let thumbs = crate::home_recent_ui::thumbnails(&ctx);
                        let mut view = app.ui.chrome.home_recent_view;
                        let mut sort = app.ui.chrome.home_recent_sort;
                        let actions = render_spektra_start_screen(app, ui, &recent, &thumbs, &mut view, &mut sort);
                        app.ui.chrome.home_recent_view = view;
                        app.ui.chrome.home_recent_sort = sort;
                        dispatch_spektra_actions(app, actions);
                    },
                    app,
                );
            for _ in 0..8 {
                h.step();
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            h.snapshot_options(
                "spektra_home",
                &egui_kittest::SnapshotOptions::default().output_path(&preview_dir),
            );
            snap_results.extend(h.take_snapshot_results());
        }

        // Render SpektraDialog preview
        {
            let theme = spektra_preview_app_theme();
            let theme_for_dialog = theme.clone();
            let services = crate::Services {
                import: Some(Box::new(|name, bytes, opts| {
                    photocraft_io::import_with_options(name, bytes, opts)
                        .map(|r| (r.document, r.warnings))
                        .map_err(|e| e.to_string())
                })),
                ..Default::default()
            };
            let mut app = PhotocraftApp::new(photocraft_engine::Session::new(), services);
            app.session.edit_prefs(|p| p.interface.language = "zh-hans".into());
            app.ui.theme = theme.clone();
            if app.open_path(&png_path.to_string_lossy()).is_err() {
                app.run("file.new", json!({"width": 160, "height": 100})).unwrap();
            }
            let _ = crate::dialogs::open_command_dialog(&mut app, crate::spektra_ui::COMMAND, "Film Simulation…").unwrap();

            let ctx = app.ctx().clone();
            PhotocraftApp::setup_context(&ctx, theme);
            crate::i18n::sync_context(&ctx, "zh-hans");

            let mut h = egui_kittest::Harness::builder()
                .allow_missing_glyphs()
                .with_size(vec2(1280.0, 800.0))
                .build_app(
                    move |ui, app| {
                        let ctx = ui.ctx().clone();
                        app.ui.theme = theme_for_dialog.clone();
                        crate::i18n::sync_context(&ctx, "zh-hans");
                        crate::spektra_ui::feed(app, &ctx);
                        crate::dialogs::run(app, ui);
                    },
                    app,
                );
            for _ in 0..12 {
                h.step();
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
            h.snapshot_options(
                "spektra_dialog",
                &egui_kittest::SnapshotOptions::default().output_path(&preview_dir),
            );
            snap_results.extend(h.take_snapshot_results());
        }

        let _ = std::fs::remove_dir_all(&fixtures_dir);
    }
}
