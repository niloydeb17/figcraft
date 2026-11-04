//! The home screen: Recents (new file, open, join with a code, the Getting started tips, open and
//! recent files ([`crate::home_recents`]), and templates to start from on a first run), Online (your online files:
//! [`crate::online`]) and Templates (the starter templates as cards with thumbnails rendered from
//! the documents themselves, searched and filtered by kind).

use std::collections::BTreeMap;

use egui::{Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_engine::templates::{Kind, TEMPLATES, Template};
use serde_json::json;

use crate::FigApp;
use crate::a11y;
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
use crate::theme::{ACCENT, BRAND, Tokens, regular, semibold};
use crate::widgets;
pub mod home_recents;
pub mod online;

/// The section the home screen shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Section {
    #[default]
    Recents,
    Online,
    Templates,
}

impl Section {
    /// The sidebar, top to bottom.
    const ALL: [(Section, Icon, &str); 3] = [
        (Section::Recents, Icon::Recents, tr_noop("Recents")),
        (Section::Online, Icon::Online, tr_noop("Online")),
        (Section::Templates, Icon::Community, tr_noop("Templates")),
    ];

    /// The id `ui.set {homeSection}` and `ui.inspect` use.
    pub fn id(self) -> &'static str {
        match self {
            Section::Recents => "recents",
            Section::Online => "online",
            Section::Templates => "templates",
        }
    }

    pub fn parse(id: &str) -> Option<Section> {
        Section::ALL.into_iter().map(|(section, ..)| section).find(|section| section.id() == id)
    }
}

/// The home screen's own state.
#[derive(Default)]
pub struct Home {
    pub section: Section,
    /// Searches known files and starter templates together from the home sidebar.
    pub file_query: String,
    /// The Templates search text and kind filter.
    pub query: String,
    pub kind: Option<Kind>,
    /// The recent files: what is known about each, and the selection.
    pub recents: crate::home_recents::Recents,
    /// The kind of recent file shown (all when none).
    pub files_filter: Option<figcraft_doc::FileKind>,
}

const SIDEBAR: f32 = 240.0;
/// The bar over the main area: the section's name, and the ways to start a file.
const TOPBAR_H: f32 = 49.0;
/// Spacing and size of the cards: as the recent files' (see [`crate::home_recents`]).
use crate::home_recents::{CARD_MAX, GAP as CARD_GAP};
/// Templates offered on a first run.
const FIRST_RUN: usize = 4;

/// A row of the home screen's file lists.
pub(crate) struct FileRow<'a> {
    /// The mark before the name, and its colour.
    pub mark: Option<(Icon, Color32)>,
    pub name: &'a str,
    pub ink: Color32,
    /// What its right end says (a path, where the file is), with its size and colour.
    pub detail: &'a str,
    pub detail_size: f32,
    pub detail_ink: Color32,
}

/// Paint `row` in `r` (highlighted while `resp` is hovered): the name first, the detail in the
/// room left after it, cut at its start; true when either is cut.
pub(crate) fn file_row(ui: &Ui, t: &Tokens, r: Rect, resp: &Response, row: &FileRow) -> bool {
    /// Between the name and the detail.
    const GAP: f32 = 16.0;
    /// A detail with less room than this is left out rather than shown as a bare ellipsis.
    const MIN_DETAIL: f32 = 48.0;
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    if let Some((icon, tint)) = row.mark {
        icons::paint(ui.painter(), Rect::from_min_size(pos2(r.min.x + 8.0, r.center().y - 8.0), vec2(16.0, 16.0)), icon, tint);
    }
    let (name_font, detail_font) = (regular(12.0), regular(row.detail_size));
    let width = |s: &str, font: &egui::FontId| ui.painter().layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE).size().x;
    let room = (r.width() - 40.0).max(0.0);
    let name_w = width(row.name, &name_font);
    let detail_w = width(row.detail, &detail_font);
    // When both don't fit, the name keeps at least 60% of the row and the detail gets the rest.
    let name_max = if name_w + GAP + detail_w <= room { name_w } else { name_w.min((room - GAP - detail_w).max(room * 0.6)) };
    let name = widgets::elide_font(ui, row.name, name_max, name_font.clone());
    let detail_room = room - width(&name, &name_font) - GAP;
    let detail = if row.detail.is_empty() || detail_room < MIN_DETAIL {
        String::new()
    } else {
        widgets::elide_start_font(ui, row.detail, detail_room, detail_font.clone())
    };
    let cut = name != row.name || detail != row.detail;
    ui.painter().text(pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, name, name_font, row.ink);
    ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, detail, detail_font, row.detail_ink);
    cut
}

/// A row of Recents' own lists: laid out in place, named for screen readers, ringed with the
/// keyboard on it; true when chosen.
fn listed(ui: &mut Ui, t: &Tokens, id: Id, height: f32, row: &FileRow) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width().min(640.0), height), Sense::hover());
    let mut resp = ui.interact(r, id, Sense::click());
    a11y::button(&resp, row.name);
    if file_row(ui, t, r, &resp, row) {
        // Cut to fit: in full on hover.
        resp = resp.on_hover_text(format!("{}\n{}", row.name, row.detail));
    }
    focus_ring(ui, t, r, &resp, 5);
    resp.clicked()
}

pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    dropped(app, ui.ctx());
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.panel)).show(ui, |ui| {
        let rect = ui.max_rect();
        let left = Rect::from_min_size(rect.min, vec2(SIDEBAR, rect.height()));
        ui.painter().line_segment([left.right_top(), left.right_bottom()], Stroke::new(1.0, t.border));
        sidebar(app, ui, &t, left);
        let searching = !app.ui.home_view.file_query.trim().is_empty();
        let title =
            if searching { tr("Search results") } else { tr(Section::ALL.iter().find(|(s, ..)| *s == app.ui.home_view.section).map_or("", |x| x.2)) };
        top_bar(app, ui, &t, Rect::from_min_size(pos2(left.max.x, rect.min.y), vec2(rect.width() - SIDEBAR, TOPBAR_H)), title);
        let main = Rect::from_min_max(pos2(left.max.x, rect.min.y + TOPBAR_H), rect.max);
        if !searching && app.ui.home_view.section == Section::Online {
            // Your online files lay themselves out in the area (and scroll their own list).
            let area = Rect::from_min_max(pos2(main.min.x + 32.0, main.min.y + 24.0), main.max - vec2(32.0, 24.0));
            crate::online::section(app, ui, area, &t);
            opened_join(app);
            return;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(main), |ui| {
            egui::ScrollArea::vertical().id_salt("home").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
                if !searching && app.ui.home_view.section != Section::Templates {
                    recents(app, ui, &t);
                    return;
                }
                padded(ui, 24, |ui| {
                    if searching {
                        search_results(app, ui, &t);
                        opened_join(app);
                    } else {
                        templates(app, ui, &t);
                    }
                });
            });
        });
    });
    // The whole window is the drop target, not only the canvas.
    crate::drop_overlay(ui.ctx(), ui.ctx().content_rect(), tr("Drop to open"));
}

/// Files dropped on the home screen: each one the app opens becomes a document of its own (the
/// editor then shows it), behind an "Opening as a new file" card ([`crate::pending_open`]); anything
/// else is turned away with a toast. A file that fails to open keeps the engine's own error.
fn dropped(app: &mut FigApp, ctx: &egui::Context) {
    let files = ctx.input(|i| i.raw.dropped_files.clone());
    for f in files {
        if !crate::opens(f.path()) {
            app.toast(tr("Drop a .figcraft, .fig, .jam, .sketch, .json, .svg or image file."));
            continue;
        }
        crate::pending_open::open_dropped(app, &f);
    }
}

/// Draw the Home sidebar: app icon and wordmark, file/template search (Escape clears it)
/// and section tabs; choosing a section clears the file query.
fn sidebar(app: &mut FigApp, ui: &mut Ui, t: &Tokens, left: Rect) {
    let x = left.min.x;
    account_header(app, ui, t, Rect::from_min_size(pos2(x + 8.0, left.min.y + 8.0), vec2(SIDEBAR - 52.0, 32.0)));
    // Notifications: FigCraft has no feed of them, and says so.
    let bell = Rect::from_min_size(pos2(left.max.x - 40.0, left.min.y + 8.0), vec2(32.0, 32.0));
    let resp = ui.interact(bell, ui.scope_id().with("home_bell"), Sense::click()).on_hover_text(tr("Notifications"));
    a11y::button(&resp, tr("Notifications"));
    if resp.hovered() {
        ui.painter().rect_filled(bell, CornerRadius::same(5), t.hover);
    }
    focus_ring(ui, t, bell, &resp, 5);
    icons::paint(ui.painter(), Rect::from_center_size(bell.center(), vec2(16.0, 16.0)), Icon::Notifications, t.icon);
    if resp.clicked() {
        app.toast(tr("You're all caught up: there are no notifications."));
    }
    // The search field: a 32 pt rounded box, the lens inside its left end.
    let search = Rect::from_min_size(pos2(x + 8.0, left.min.y + 48.0), vec2(SIDEBAR - 16.0, 32.0));
    ui.painter().rect_filled(search, CornerRadius::same(8), t.field);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(search.min.x + 16.0, search.center().y), vec2(16.0, 16.0)),
        Icon::Search,
        t.icon_secondary,
    );
    let typing =
        Rect::from_min_size(pos2(search.min.x + 28.0, search.min.y + (32.0 - widgets::FIELD_H) / 2.0), vec2(search.width() - 32.0, widgets::FIELD_H));
    ui.scope_builder(egui::UiBuilder::new().max_rect(typing), |ui| {
        let response = widgets::text_input(
            ui,
            Id::unique("home_file_search"),
            &mut app.ui.home_view.file_query,
            typing.width(),
            tr("Search files and templates"),
        );
        a11y::text(&response, tr("Search files and templates"), &app.ui.home_view.file_query);
        if (response.has_focus() || response.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            app.ui.home_view.file_query.clear();
        }
    });
    // The sections, then the workspace with its files.
    let mut y = left.min.y + 88.0;
    for (section, icon, label) in Section::ALL {
        let current = app.ui.home_view.file_query.trim().is_empty() && app.ui.home_view.section == section;
        if nav_row(ui, t, left, y, icon, t.icon, tr(label), current).clicked() {
            app.ui.home_view.section = section;
            app.ui.home_view.file_query.clear();
        }
        y += 32.0;
    }
    divider(ui, t, left, &mut y);
    workspace_row(app, ui, t, left, y);
    y += 36.0;
    if nav_row(ui, t, left, y, Icon::Drafts, t.icon, tr("Drafts"), false).clicked() {
        app.toast(tr("Drafts aren't in FigCraft yet: files live on this computer or on a server."));
    }
    y += 32.0;
    if nav_row(ui, t, left, y, Icon::ViewGrid, t.icon, tr("All folders"), false).clicked() {
        app.open_dialog();
    }
    y += 32.0;
    if nav_row(ui, t, left, y, Icon::Resources, t.icon, tr("Resources"), false).clicked() {
        app.ui.home_view.section = Section::Templates;
        app.ui.home_view.file_query.clear();
    }
    y += 32.0;
    if nav_row(ui, t, left, y, Icon::Trash, t.icon, tr("Trash"), false).clicked() {
        app.toast(tr("There's no trash in FigCraft yet."));
    }
    y += 32.0;
    // The plan card: a bordered button with two lines and a chevron.
    let card = Rect::from_min_size(pos2(x + 12.0, y + 8.0), vec2(SIDEBAR - 24.0, 58.0));
    let resp = ui.interact(card, ui.scope_id().with("home_plan"), Sense::click());
    a11y::button(&resp, tr("See what's included"));
    ui.painter().rect_filled(card, CornerRadius::same(13), if resp.hovered() { t.hover } else { t.panel });
    ui.painter().rect_stroke(card, CornerRadius::same(13), Stroke::new(1.0, t.border), StrokeKind::Inside);
    focus_ring(ui, t, card, &resp, 13);
    ui.painter().text(pos2(card.min.x + 12.0, card.min.y + 20.0), Align2::LEFT_CENTER, tr("See what's included"), regular(11.0), t.text);
    ui.painter().text(pos2(card.min.x + 12.0, card.min.y + 38.0), Align2::LEFT_CENTER, tr("Your plan and usage"), regular(11.0), t.text_secondary);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(card.max.x - 20.0, card.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, t.icon);
    if resp.clicked() {
        app.toast(tr("Everything in FigCraft is included: it's free and open source."));
    }
    y += 74.0;
    divider(ui, t, left, &mut y);
    // Starred: its heading, then what is in it.
    ui.painter().text(pos2(x + 24.0, y + 16.0), Align2::LEFT_CENTER, tr("Starred"), semibold(11.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(x + 8.0, y + 16.0), vec2(16.0, 16.0)), Icon::Chevron, t.icon_secondary);
    y += 32.0;
    if nav_row(ui, t, left, y, Icon::Folder, Color32::from_rgb(0xAF, 0xBC, 0xCF), tr("Team folder"), false).clicked() {
        app.toast(tr("Starring isn't in FigCraft yet."));
    }
    // The upgrade button at the foot.
    let upgrade = Rect::from_min_size(pos2(x + 16.0, left.max.y - 58.0), vec2(SIDEBAR - 32.0, 32.0));
    let resp = ui.interact(upgrade, ui.scope_id().with("home_upgrade"), Sense::click());
    a11y::button(&resp, tr("Upgrade"));
    ui.painter().rect_filled(upgrade, CornerRadius::same(5), if resp.hovered() { ACCENT } else { BRAND });
    focus_ring(ui, t, upgrade, &resp, 5);
    let label_w = ui.painter().layout_no_wrap(tr("Upgrade").to_string(), regular(11.0), Color32::WHITE).size().x;
    let start = upgrade.center().x - (24.0 + label_w) / 2.0;
    icons::paint(ui.painter(), Rect::from_center_size(pos2(start + 12.0, upgrade.center().y), vec2(16.0, 16.0)), Icon::Upgrade, Color32::WHITE);
    ui.painter().text(pos2(start + 24.0, upgrade.center().y), Align2::LEFT_CENTER, tr("Upgrade"), regular(11.0), Color32::WHITE);
    if resp.clicked() {
        app.toast(tr("Everything in FigCraft is included: it's free and open source."));
    }
}

/// A line across the sidebar with 8 pt over and under it.
fn divider(ui: &Ui, t: &Tokens, left: Rect, y: &mut f32) {
    ui.painter().line_segment([pos2(left.min.x, *y + 8.0), pos2(left.max.x, *y + 8.0)], Stroke::new(1.0, t.border));
    *y += 17.0;
}

/// A 32 pt sidebar row: the row itself 28 pt and inset 8 pt, a 24 pt slot for the icon (tinted
/// `tint`) and the label; lit when `current`.
fn nav_row(ui: &mut Ui, t: &Tokens, left: Rect, y: f32, icon: Icon, tint: Color32, label: &str, current: bool) -> Response {
    let r = Rect::from_min_size(pos2(left.min.x + 8.0, y + 2.0), vec2(SIDEBAR - 16.0, 28.0));
    let resp = ui.interact(r, ui.scope_id().with(("home_nav", label)), Sense::click());
    a11y::tab(&resp, label, current);
    if current || resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), if current { t.selected_row } else { t.hover });
    }
    focus_ring(ui, t, r, &resp, 5);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(16.0, 16.0)), icon, tint);
    ui.painter().text(pos2(r.min.x + 36.0, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.text);
    resp
}

/// The workspace: its round mark, its name, a chevron, and the plan beside them; choosing it shows
/// your online files. The plan is the one the onboarding recorded (nothing is billed).
fn workspace_row(app: &mut FigApp, ui: &mut Ui, t: &Tokens, left: Rect, y: f32) {
    let (name, plan) = match &app.session.account.account {
        Some(a) => {
            let who = if a.name.trim().is_empty() { a.email.split('@').next().unwrap_or_default() } else { a.name.trim() };
            let plan = match a.profile.plan.as_str() {
                "professional" => tr("Pro"),
                "organization" => tr("Org"),
                _ => tr("Free"),
            };
            (trf("{name}'s team", &[("name", &who)]), plan)
        }
        None => (tr("Your workspace").to_string(), tr("Free")),
    };
    let row = Rect::from_min_size(pos2(left.min.x + 8.0, y + 2.0), vec2(SIDEBAR - 16.0, 32.0));
    // The button runs from the mark to the chevron; the plan chip follows it.
    let chip_w = ui.painter().layout_no_wrap(plan.to_string(), regular(11.0), t.text).size().x + 12.0;
    let button = Rect::from_min_size(row.min, vec2(row.width() - chip_w - 4.0, 32.0));
    let resp = ui.interact(button, ui.scope_id().with("home_workspace"), Sense::click());
    a11y::button(&resp, &name);
    if resp.hovered() {
        ui.painter().rect_filled(button, CornerRadius::same(4), t.hover);
    }
    focus_ring(ui, t, button, &resp, 4);
    let mark = pos2(button.min.x + 16.0, button.center().y);
    ui.painter().circle_filled(mark, 8.0, Color32::from_rgb(0xFF, 0x24, 0xBD));
    let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    ui.painter().text(mark, Align2::CENTER_CENTER, initial, semibold(10.0), Color32::WHITE);
    let shown = widgets::elide_font(ui, &name, button.width() - 40.0 - 24.0, semibold(13.0));
    ui.painter().text(pos2(button.min.x + 36.0, button.center().y), Align2::LEFT_CENTER, shown, semibold(13.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(button.max.x - 12.0, button.center().y), vec2(16.0, 16.0)), Icon::Chevron, t.icon);
    if resp.clicked() {
        app.ui.home_view.section = Section::Online;
        app.ui.home_view.file_query.clear();
    }
    let chip = Rect::from_min_size(pos2(button.max.x + 4.0, row.center().y - 8.0), vec2(chip_w, 16.0));
    ui.painter().rect_filled(chip, CornerRadius::same(4), t.selected_row_child);
    ui.painter().text(chip.center(), Align2::CENTER_CENTER, plan, regular(11.0), ACCENT);
    ui.interact(chip, ui.scope_id().with("home_plan_chip"), Sense::hover())
        .on_hover_text(tr("Everything in FigCraft is included: it's free and open source."));
}

/// The sidebar's first row: who is signed in (their picture, or their initials until it is in, and
/// their name, with a menu), else the app's own mark and name.
fn account_header(app: &mut FigApp, ui: &mut Ui, t: &Tokens, r: Rect) {
    let Some(account) = app.session.account.account.as_ref() else {
        crate::theme::paint_app_icon(ui, Rect::from_min_size(pos2(r.min.x + 4.0, r.min.y + 4.0), vec2(24.0, 24.0)));
        crate::theme::paint_wordmark(ui.painter(), pos2(r.min.x + 36.0, r.center().y), 15.0, t);
        return;
    };
    let (name, email) = (
        if account.name.trim().is_empty() {
            account.email.split('@').next().unwrap_or_default().to_string()
        } else {
            account.name.trim().to_string()
        },
        account.email.clone(),
    );
    let shown = widgets::elide_font(ui, &name, r.width() - 44.0 - 20.0, semibold(13.0));
    let name_w = ui.painter().layout_no_wrap(shown.clone(), semibold(13.0), t.text).size().x;
    let button = Rect::from_min_size(r.min, vec2((4.0 + 24.0 + 8.0 + name_w + 4.0 + 16.0 + 4.0).min(r.width()), 32.0));
    let resp = ui.interact(button, ui.scope_id().with("home_account"), Sense::click()).on_hover_text(&email);
    a11y::button(&resp, &trf("Account menu for {name}", &[("name", &name)]));
    if resp.hovered() {
        ui.painter().rect_filled(button, CornerRadius::same(4), t.hover);
    }
    focus_ring(ui, t, button, &resp, 4);
    let center = pos2(button.min.x + 16.0, button.center().y);
    match &app.ui.account_picture {
        Some(picture) => crate::account_picture::paint(ui.painter(), center, 12.0, picture, t.border),
        None => crate::live::avatar(ui.painter(), center, 12.0, &name, ACCENT),
    }
    ui.painter().text(pos2(button.min.x + 36.0, button.center().y), Align2::LEFT_CENTER, shown, semibold(13.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(button.max.x - 12.0, button.center().y), vec2(16.0, 16.0)), Icon::Chevron, t.icon);
    crate::right::menu_popup(&resp).show(|ui| {
        if crate::right::menu_item(ui, &email, "") {
            ui.close();
        }
        ui.separator();
        if crate::right::menu_item(ui, tr("Preferences…"), "") {
            app.ui.preferences = true;
            ui.close();
        }
        if crate::right::menu_item(ui, tr("Keyboard shortcuts"), "") {
            app.ui.shortcuts_panel = true;
            ui.close();
        }
        if crate::right::menu_item(ui, tr("About FigCraft"), "") {
            app.ui.about = true;
            ui.close();
        }
    });
}

/// The bar over the main area: the section's name on the left; on the right the ways to start a
/// file as pills (their marks alone when the window is narrow), each saying what it does on hover.
fn top_bar(app: &mut FigApp, ui: &mut Ui, t: &Tokens, bar: Rect, title: &str) {
    let title_end = ui.painter().text(pos2(bar.min.x + 32.0, bar.center().y), Align2::LEFT_CENTER, title, regular(13.0), t.text).max.x;
    let pills = [
        (Icon::PenNib, tr("New design file"), tr("Start from a blank canvas"), BRAND),
        (Icon::Board, tr("New board"), tr("Sticky notes, shapes and connectors"), Color32::from_rgb(0x97, 0x47, 0xFF)),
        (Icon::Page, tr("Open file"), tr("Open .figcraft, .svg or images"), Color32::from_rgb(0x2C, 0x2C, 0x2C)),
    ];
    let text_w = |label: &str| ui.painter().layout_no_wrap(label.to_string(), regular(11.0), t.text).size().x;
    // 16 pt each side, the 16 pt mark, 8 pt, the label.
    let full: Vec<f32> = pills.iter().map(|(_, label, ..)| 16.0 + 16.0 + 8.0 + text_w(label).ceil() + 16.0).collect();
    let spread = |w: &[f32]| w.iter().sum::<f32>() + 8.0 * (w.len() as f32 - 1.0);
    let marks_only = bar.max.x - 32.0 - spread(&full) < title_end + 24.0;
    let widths: Vec<f32> = if marks_only { vec![32.0; pills.len()] } else { full };
    let mut x = bar.max.x - 32.0 - spread(&widths);
    let mut chosen = None;
    for (i, ((icon, label, sub, mark), w)) in pills.into_iter().zip(widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, bar.min.y + 8.0), vec2(w, 32.0));
        x += w + 8.0;
        let resp = ui.interact(r, ui.scope_id().with(("home_start", i)), Sense::click()).on_hover_text(if marks_only {
            format!("{label}\n{sub}")
        } else {
            sub.to_string()
        });
        a11y::button(&resp, label);
        ui.painter().rect_filled(r, CornerRadius::same(16), if resp.hovered() { t.pressed } else { t.field });
        ui.painter().rect_stroke(r, CornerRadius::same(16), Stroke::new(1.0, t.border), StrokeKind::Inside);
        focus_ring(ui, t, r, &resp, 16);
        let tile = Rect::from_min_size(pos2(if marks_only { r.center().x - 8.0 } else { r.min.x + 16.0 }, r.center().y - 8.0), vec2(16.0, 16.0));
        ui.painter().rect_filled(tile, CornerRadius::same(2), mark);
        icons::paint(ui.painter(), tile.shrink(2.0), icon, Color32::WHITE);
        if !marks_only {
            ui.painter().text(pos2(tile.max.x + 8.0, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.text);
        }
        if resp.clicked() {
            chosen = Some(i);
        }
    }
    match chosen {
        Some(0 | 1) => {
            app.run(if chosen == Some(1) { "file.newBoard" } else { "file.new" }, json!({}));
            app.ui.home = false;
        }
        Some(_) => app.open_dialog(),
        None => {}
    }
}

/// `content` in the main area's margin: 32 pt at the sides and `top` over it.
fn padded(ui: &mut Ui, top: i8, content: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE.inner_margin(egui::Margin { left: 32, right: 32, top, bottom: 24 }).show(ui, content);
}

fn file_matches(name: &str, location: &str, query: &str) -> bool {
    name.to_lowercase().contains(query) || location.to_lowercase().contains(query)
}

/// One search over known files and starter templates. It never sends a query or owner token to a
/// server; online files are opened through their existing owner-token command only on click.
fn search_results(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    let query = app.ui.home_view.file_query.trim().to_lowercase();
    let mut count = crate::home_recents::section(app, ui, t, &query);

    let online: Vec<_> = app.session.prefs.online_files.iter().filter(|f| file_matches(&f.name, &f.server, &query)).cloned().collect();
    let mut open_online = None;
    if !online.is_empty() {
        subheading(ui, t, tr("Online files"));
        for f in &online {
            count += 1;
            let host = figcraft_engine::online::server_host(&f.server);
            let icon = crate::online::mark(f.shared.is_some());
            let row = FileRow {
                mark: Some((icon, if f.shared.is_some() { t.success } else { t.icon_secondary })),
                name: &f.name,
                ink: t.text,
                detail: host,
                detail_size: 11.0,
                detail_ink: t.text_secondary,
            };
            if listed(ui, t, ui.scope_id().with(("search_online", &f.server, &f.file)), 36.0, &row) {
                open_online = Some(f.clone());
            }
        }
    }
    let templates: Vec<&Template> = TEMPLATES.iter().filter(|template| matches(template, &query, None)).collect();
    if !templates.is_empty() {
        subheading(ui, t, tr("Templates"));
        template_grid(app, ui, t, &templates);
    }
    if count == 0 && templates.is_empty() {
        ui.label(egui::RichText::new(tr("No files or templates match your search")).font(semibold(13.0)).color(t.text));
        ui.add_space(6.0);
        widgets::label(ui, tr("Try another search, or browse templates."), t.text_secondary);
        ui.add_space(16.0);
        if widgets::button(ui, tr("Show all templates"), false, true).clicked() {
            app.ui.home_view.file_query.clear();
            app.ui.home_view.section = Section::Templates;
        }
    }
    if let Some(file) = open_online {
        crate::online::open(app, &file);
    }
}

/// A ring around a card or row that has the keyboard.
fn focus_ring(ui: &Ui, t: &Tokens, rect: Rect, resp: &Response, radius: u8) {
    if resp.has_focus() {
        ui.painter().rect_stroke(rect, CornerRadius::same(radius), Stroke::new(2.0, t.focus), StrokeKind::Outside);
    }
}

fn subheading(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add_space(28.0);
    ui.label(egui::RichText::new(text).font(semibold(12.0)).color(t.text_secondary));
    ui.add_space(12.0);
}

fn recents(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    padded(ui, 0, |ui| recents_body(app, ui, t));
}

fn recents_body(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    let none_kept = app.session.account.recents.is_empty() && crate::home_recents::listed(&app.session.prefs.recent_files).next().is_none();
    if none_kept && app.session.docs.is_empty() {
        // A first run: nothing to reopen yet, so offer something to start from.
        crate::home_recents::first_run_row(app, ui, t);
        if template_header(ui, t, tr("Start from a template"), tr("See all templates")) {
            app.ui.home_view.section = Section::Templates;
        }
        let shown: Vec<&Template> = TEMPLATES.iter().take(FIRST_RUN).collect();
        template_grid(app, ui, t, &shown);
        return;
    }
    crate::home_recents::section(app, ui, t, "");
}

/// The line over the template cards of a first run: what they are, and a button for all of them;
/// true when that is clicked.
fn template_header(ui: &mut Ui, t: &Tokens, label: &str, action: &str) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().text(pos2(row.min.x, row.center().y), Align2::LEFT_CENTER, label, semibold(12.0), t.text_secondary);
    let w = ui.painter().layout_no_wrap(action.to_string(), regular(11.0), t.text).size().x.ceil() + 24.0;
    let button = Rect::from_min_size(pos2(row.max.x - w, row.center().y - 12.0), vec2(w, 24.0));
    let resp = ui.interact(button, ui.scope_id().with(("home_template_header", action)), Sense::click());
    a11y::button(&resp, action);
    ui.painter().rect_filled(button, CornerRadius::same(5), if resp.hovered() { t.hover } else { t.panel });
    ui.painter().rect_stroke(button, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
    focus_ring(ui, t, button, &resp, 5);
    ui.painter().text(button.center(), Align2::CENTER_CENTER, action, regular(11.0), t.text);
    resp.clicked()
}

/// An open file for the Recently viewed list: the tab that shows it.
pub(crate) struct OpenFile {
    /// The tab's place in `Session::docs`, and its `DocState::uid`.
    pub index: usize,
    pub uid: u64,
    pub name: String,
    pub kind: figcraft_doc::FileKind,
    /// Where it is on disk, if it is.
    pub path: Option<String>,
}

/// What names the file a tab shows: its path; else the file on the server it is kept in or
/// joined; else only the tab itself. Tabs with one key are one file.
fn open_key(app: &FigApp, d: &figcraft_engine::DocState) -> String {
    if let Some(path) = d.path.as_deref().filter(|p| !p.is_empty()) {
        return format!("path:{path}");
    }
    if let Some(live) = app.session.live.iter().find(|l| l.doc_uid == Some(d.uid)) {
        if let Some(p) = live.published.as_ref() {
            return format!("server:{}/{}", p.server, p.file);
        }
        if let Some(info) = live.info.as_ref().filter(|i| !i.link.is_empty()) {
            return format!("link:{}", info.link);
        }
    }
    format!("tab:{}", d.uid)
}

/// The open files, each once: tabs that show one file are merged into the one you are in (else
/// the one edited most).
pub(crate) fn open_list(app: &FigApp) -> Vec<OpenFile> {
    let mut list: Vec<(String, OpenFile)> = Vec::new();
    for (index, d) in app.session.docs.iter().enumerate() {
        let key = open_key(app, d);
        let file = OpenFile { index, uid: d.uid, name: d.doc.name.clone(), kind: d.doc.kind, path: d.path.clone().filter(|p| !p.is_empty()) };
        match list.iter_mut().find(|(k, _)| *k == key) {
            Some((_, kept)) => {
                let better = index == app.session.active
                    || (kept.index != app.session.active && d.revision > app.session.docs.get(kept.index).map_or(0, |k| k.revision));
                if better {
                    *kept = file;
                }
            }
            None => list.push((key, file)),
        }
    }
    list.into_iter().map(|(_, f)| f).collect()
}

/// The pictures of the open files `uids` as they are now, by tab: drawn again when a file changes
/// (one new picture a frame, so the screen keeps moving), `None` for one with nothing to show.
pub(crate) fn tab_pictures(ctx: &egui::Context, docs: &[figcraft_engine::DocState], uids: &[u64]) -> BTreeMap<u64, Option<Thumb>> {
    let dark = ctx.global_style().visuals.dark_mode;
    let mut cache: OpenThumbs = ctx.data(|d| d.get_temp(Id::unique("open_file_thumbs"))).unwrap_or_default();
    // A file closed is forgotten.
    cache.0.retain(|uid, _| uids.contains(uid));
    let mut rendered = false;
    for d in docs.iter().filter(|d| uids.contains(&d.uid)) {
        let fresh = cache.0.get(&d.uid).is_some_and(|(revision, was_dark, _)| *revision == d.revision && *was_dark == dark);
        if fresh {
            continue;
        }
        if rendered {
            ctx.request_repaint();
        } else {
            cache.0.insert(d.uid, (d.revision, dark, thumbnail_of(ctx, &d.doc, &format!("open-file-{}", d.uid))));
            rendered = true;
        }
    }
    let pictures = cache.0.iter().map(|(uid, (_, _, thumb))| (*uid, thumb.clone())).collect();
    ctx.data_mut(|d| d.insert_temp(Id::unique("open_file_thumbs"), cache));
    pictures
}

/// A picture on its page colour, as large as fits `area`.
fn paint_thumb(p: &egui::Painter, area: Rect, top: CornerRadius, thumb: &Thumb) {
    p.rect_filled(area, top, thumb.page);
    let Some(texture) = &thumb.texture else { return };
    let size = texture.size_vec2();
    let inner = area.shrink(area.height() * 0.05);
    let fit = (inner.width() / size.x.max(1.0)).min(inner.height() / size.y.max(1.0));
    let image = Rect::from_center_size(inner.center(), size * fit);
    p.image(texture.id(), image, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// The file joined from the home screen (a code, or one of your online files) opened in a tab of
/// its own: show it. (A join that was left is forgotten.)
fn opened_join(app: &mut FigApp) {
    match app.ui.live.home_join.map(|id| app.session.live.get(id).map(figcraft_engine::live::Live::syncing)) {
        Some(Some(true)) => {
            app.ui.live.home_join = None;
            app.ui.home = false;
        }
        Some(None) => app.ui.live.home_join = None,
        _ => {}
    }
}

fn matches(template: &Template, query: &str, kind: Option<Kind>) -> bool {
    let q = query.trim().to_lowercase();
    let has = |s: &str| s.to_lowercase().contains(&q);
    kind.is_none_or(|k| k == template.kind)
        && (q.is_empty()
            || [template.name, template.description, template.kind.label()].into_iter().any(|s| has(s) || has(tr(s)))
            || has(template.size))
}

/// Draw template search and kind filter, show no-match guidance and a "Show all templates"
/// button that clears the query and kind, and lay matching templates out in a grid.
fn templates(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    ui.horizontal(|ui| {
        let search = widgets::text_input(ui, Id::unique("home_template_search"), &mut app.ui.home_view.query, 260.0, tr("Search templates"));
        a11y::text(&search, tr("Search templates"), &app.ui.home_view.query);
        let labels: Vec<&str> = std::iter::once(tr("All")).chain(Kind::ALL.iter().map(|k| tr(k.label()))).collect();
        let active = app.ui.home_view.kind.and_then(|k| Kind::ALL.iter().position(|x| *x == k)).map_or(0, |i| i + 1);
        ui.scope(|ui| {
            ui.set_max_width((labels.len() as f32 * 124.0).min(ui.available_width()));
            if let Some(i) = widgets::segmented_text(ui, &labels, active) {
                app.ui.home_view.kind = i.checked_sub(1).and_then(|i| Kind::ALL.get(i).copied());
            }
        });
    });
    ui.add_space(24.0);
    let (query, kind) = (app.ui.home_view.query.clone(), app.ui.home_view.kind);
    let shown: Vec<&Template> = TEMPLATES.iter().filter(|x| matches(x, &query, kind)).collect();
    if shown.is_empty() {
        ui.add_space(24.0);
        ui.label(egui::RichText::new(tr("No templates match your search")).font(semibold(13.0)).color(t.text));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("Try another word, or look at every kind.")).font(regular(12.0)).color(t.text_secondary));
        ui.add_space(14.0);
        if widgets::button(ui, tr("Show all templates"), false, true).clicked() {
            app.ui.home_view.query.clear();
            app.ui.home_view.kind = None;
        }
        return;
    }
    template_grid(app, ui, t, &shown);
}

/// Template cards in as many columns as fit (the recent files' cards: a 16:9 picture over a 60 pt
/// footer with a mark, the name and what it is); choosing one opens a new document made from it.
fn template_grid(app: &mut FigApp, ui: &mut Ui, t: &Tokens, list: &[&Template]) {
    let (columns, card_w) = crate::home_recents::grid_columns(ui.available_width());
    let (picture_h, card_h) = crate::home_recents::card_heights(card_w);
    let dark = ui.visuals().dark_mode;
    let mut thumbs: Thumbs = ui.data(|d| d.get_temp(Id::unique("template_thumbs"))).unwrap_or_default();
    let mut chosen = None;
    let mut rendered = false;
    for row in list.chunks(columns) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), card_h), Sense::hover());
        for (i, template) in row.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(r.min.x + i as f32 * (card_w + CARD_GAP), r.min.y), vec2(card_w, card_h));
            let key = (template.id, dark);
            if !thumbs.0.contains_key(&key) && ui.is_rect_visible(rect) {
                if rendered {
                    // One a frame keeps the screen moving; the rest follow.
                    ui.ctx().request_repaint();
                } else {
                    thumbs.0.insert(key, thumbnail(ui.ctx(), template, app.session.new_page_background()));
                    rendered = true;
                }
            }
            if template_card(ui, t, rect, picture_h, template, thumbs.0.get(&key).and_then(Option::as_ref)) {
                chosen = Some(*template);
            }
        }
        ui.add_space(CARD_GAP);
    }
    ui.data_mut(|d| d.insert_temp(Id::unique("template_thumbs"), thumbs));
    if let Some(template) = chosen
        && app.run("file.newFromTemplate", json!({"template": template.id, "name": tr(template.name)})).is_some()
    {
        app.ui.home = false;
    }
}

fn template_card(ui: &mut Ui, t: &Tokens, rect: Rect, picture_h: f32, template: &Template, thumb: Option<&Thumb>) -> bool {
    let resp = ui.interact(rect, ui.scope_id().with(("template_card", template.id)), Sense::click());
    let (name, description) = (tr(template.name), tr(template.description));
    a11y::button(&resp, &format!("{name}: {description}"));
    let resp = resp.on_hover_text(format!("{name}\n{description}"));
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(13), t.panel);
    let area = Rect::from_min_size(rect.min, vec2(rect.width(), picture_h)).shrink(1.0);
    let top = CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 };
    match thumb {
        Some(thumb) => paint_thumb(p, area, top, thumb),
        None => {
            p.rect_filled(area, top, t.field);
            icons::paint(p, Rect::from_center_size(area.center(), vec2(20.0, 20.0)), Icon::Frame, t.icon_secondary);
        }
    }
    p.line_segment([pos2(rect.min.x, area.max.y + 0.5), pos2(rect.max.x, area.max.y + 0.5)], Stroke::new(1.0, t.border));
    // The footer: 16 pt in, a 16 pt design-file mark, 12 pt, then the name and what it is.
    let footer = area.max.y + 1.0;
    let mark = Rect::from_center_size(pos2(rect.min.x + 24.0, footer + 30.0), vec2(16.0, 16.0));
    p.rect_filled(mark, CornerRadius::same(4), t.accent);
    icons::paint(p, mark.shrink(3.0), Icon::PenNib, t.accent_on);
    let (x, w) = (rect.min.x + 44.0, rect.width() - 60.0);
    p.text(pos2(x, footer + 20.0), Align2::LEFT_CENTER, widgets::elide_font(ui, name, w, semibold(11.0)), semibold(11.0), t.text);
    let tag = format!("{} · {}", tr(template.kind.label()), template.size);
    p.text(pos2(x, footer + 38.0), Align2::LEFT_CENTER, widgets::elide_font(ui, &tag, w, regular(11.0)), regular(11.0), t.text_secondary);
    p.rect_stroke(rect, CornerRadius::same(13), Stroke::new(1.0, if resp.hovered() { t.border_strong } else { t.border }), StrokeKind::Inside);
    focus_ring(ui, t, rect, &resp, 13);
    resp.clicked()
}

/// A template's picture: its first page's frames, and the page colour they sit on.
#[derive(Clone)]
pub(crate) struct Thumb {
    /// What is on the page (`None`: nothing: an empty file is its page colour alone).
    pub texture: Option<egui::TextureHandle>,
    pub page: Color32,
}

/// Pictures by template and theme (the page colour follows the theme); `None`: nothing to show.
#[derive(Clone, Default)]
struct Thumbs(BTreeMap<(&'static str, bool), Option<Thumb>>);

/// Build `template` and render the top of its first page to fit a card.
fn thumbnail(ctx: &egui::Context, template: &Template, background: figcraft_color::Color) -> Option<Thumb> {
    let doc = template.build(background).ok()?;
    thumbnail_of(ctx, &doc, &format!("template-thumb-{}", template.id))
}

/// Render the top of `doc`'s first page to fit a card, on that page's colour. A file with nothing
/// on its page is that colour alone; `None` only for a file with no page.
fn thumbnail_of(ctx: &egui::Context, doc: &figcraft_doc::Document, name: &str) -> Option<Thumb> {
    let page = doc.first_page()?;
    let background = match doc.node(page).map(|n| &n.kind) {
        Some(figcraft_doc::NodeKind::Page { background, .. }) => *background,
        _ => return None,
    };
    let empty = Thumb { texture: None, page: crate::theme::c32(background) };
    let frames = doc.node(page)?.children.clone();
    let Some(bounds) = frames.iter().filter_map(|id| figcraft_render::export_bounds(doc, *id)).reduce(|a, b| a.union(b)) else {
        return Some(empty);
    };
    if !(bounds.width() > 0.0 && bounds.height() > 0.0) {
        return Some(empty);
    }
    // A page much taller than a card shows its top, as wide as the card.
    let (w, h) = (f64::from(CARD_MAX - 20.0), f64::from((CARD_MAX * 9.0 / 16.0).round() - 20.0));
    let area = figcraft_geom::Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y1.min(bounds.y0 + bounds.width() * h / w * 1.5));
    // As sharp as the card on this screen: small files are drawn larger, not left at one pixel
    // to a point and stretched.
    let scale = ((w / area.width()).min(h / area.height()) * f64::from(ctx.pixels_per_point())).min(8.0);
    let Some(pix) = figcraft_render::render_area(doc, &frames, area, scale) else { return Some(empty) };
    let image = crate::render_worker::screen_image(&pix, doc.color_profile);
    let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR);
    Some(Thumb { texture: Some(texture), page: empty.page })
}

/// The open files' pictures by tab: the revision and theme each was drawn for.
#[derive(Clone, Default)]
struct OpenThumbs(BTreeMap<u64, (u64, bool, Option<Thumb>)>);

/// Tests: the templates whose pictures are cached for the light or dark theme.
#[cfg(test)]
pub(crate) fn cached_thumbnails(ctx: &egui::Context, dark: bool) -> Vec<&'static str> {
    let thumbs: Thumbs = ctx.data(|d| d.get_temp(Id::unique("template_thumbs"))).unwrap_or_default();
    thumbs.0.iter().filter(|((_, d), thumb)| *d == dark && thumb.is_some()).map(|((id, _), _)| *id).collect()
}

/// Tests: how many open files have something drawn on their picture.
#[cfg(test)]
pub(crate) fn open_pictures(ctx: &egui::Context) -> usize {
    let thumbs: OpenThumbs = ctx.data(|d| d.get_temp(Id::unique("open_file_thumbs"))).unwrap_or_default();
    thumbs.0.values().filter(|(_, _, thumb)| thumb.as_ref().is_some_and(|t| t.texture.is_some())).count()
}
