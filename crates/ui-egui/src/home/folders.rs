//! Home ▸ All folders: the account's folders as cards (the pictures of their first three files, the
//! folder's name and when it last changed), each opening to its files; a folder's right-click menu
//! (open, star, rename, delete); the Create folder dialog ("+ Folder"), and Move to folder for a file
//! on the account's server (its card's Move file…). Starred folders are listed in the sidebar.
//!
//! The folders are the account's ([`figcraft_engine::account::folders`]): kept on its server, they
//! show on every computer it signs in on. They hold files on the server, one folder to a file.

use egui::{Align2, Color32, CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_engine::account::{Folder, FolderFile};
use serde_json::json;

use crate::FigApp;
use crate::a11y;
use crate::home::home_menus;
use crate::home_recents::{CARD_MAX, GAP, ago, grid_columns};
use crate::i18n::{tr, trf, trn};
use crate::icons::{self, Icon};
use crate::theme::{BRAND, Tokens, regular, semibold};

/// A folder card's height: its row of pictures and the footer under it.
const PICTURES_H: f32 = 96.0;
const FOOTER_H: f32 = 60.0;
const CARD_H: f32 = PICTURES_H + FOOTER_H;

/// A folder's mark, as the sidebar's.
const FOLDER_MARK: Color32 = Color32::from_rgb(0xAF, 0xBC, 0xCF);

/// Create folder, as it's filled in.
#[derive(Clone, Debug, PartialEq)]
pub struct NewFolder {
    pub name: String,
    /// Who it's for: "team" or "private"; what they may do: "edit" or "view".
    pub access: &'static str,
    pub role: &'static str,
    /// 1: name and access; 2: the files to put in it.
    pub step: u8,
    /// The files picked in step 2 (targets), and the one it was opened for (Move to folder's New
    /// folder), which is put in whatever is picked.
    pub picked: Vec<String>,
    pub then_add: Option<FolderFile>,
}

/// What the folders' dialogs are doing.
#[derive(Clone, Debug, Default)]
pub struct Dialogs {
    /// Create folder is open.
    pub create: Option<NewFolder>,
    /// Move to folder is open for this file: the folder picked.
    pub move_file: Option<(FolderFile, Option<String>)>,
    /// Rename folder is open: the folder and the name typed.
    pub rename: Option<(String, String)>,
}

/// When `folder` last changed: it was made, renamed or had a file put in, or one of its files was
/// opened (as the account's recent files know).
fn updated(app: &FigApp, folder: &Folder) -> u64 {
    let opened = folder.files.iter().filter_map(|f| app.session.account.recents.iter().find(|r| r.target == f.target)).map(|r| r.opened);
    opened.fold(folder.updated, u64::max)
}

/// The page: the workspace's mark, name and plan, then the folders as cards (newest change first),
/// or what to do when there are none.
pub fn all_folders(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    workspace_header(app, ui, t);
    let mut folders: Vec<Folder> = app.session.account.folders.clone();
    folders.sort_by_key(|f| std::cmp::Reverse(updated(app, f)));
    if folders.is_empty() {
        ui.add_space(24.0);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
        ui.painter().text(pos2(r.center().x, r.min.y + 30.0), Align2::CENTER_CENTER, tr("No folders yet"), semibold(13.0), t.text);
        ui.painter().text(
            pos2(r.center().x, r.min.y + 54.0),
            Align2::CENTER_CENTER,
            tr("Folders keep your files together on every computer you sign in on."),
            regular(11.0),
            t.text_secondary,
        );
        let b = Rect::from_center_size(pos2(r.center().x, r.min.y + 92.0), vec2(120.0, 32.0));
        if primary_button(ui, t, b, Id::unique("home_folders_empty_new"), tr("New folder")).clicked() {
            open_create(app, None);
        }
        return;
    }
    let (columns, card_w) = grid_columns(ui.available_width());
    let card_w = card_w.min(CARD_MAX);
    let now = figcraft_engine::cmd::comment::now_secs();
    let time = ui.input(|i| i.time);
    for row in folders.chunks(columns.max(1)) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), CARD_H), Sense::hover());
        for (i, folder) in row.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(r.min.x + i as f32 * (card_w + GAP), r.min.y), vec2(card_w, CARD_H));
            let resp = ui.interact(rect, Id::unique(("home_folder", &folder.id)), Sense::click());
            a11y::named_button(&resp, &trn("{name}, folder, {n} files", folder.files.len(), &[("name", &folder.name)]));
            if ui.is_rect_visible(rect) {
                folder_card(app, ui, t, rect, &resp, folder, now, time);
            }
            if resp.clicked() || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                open_folder(app, &folder.id);
            }
            folder_menu(app, &resp, folder);
        }
        ui.add_space(GAP);
    }
}

/// The page's heading: the workspace's 40 pt mark, its name and its plan.
fn workspace_header(app: &FigApp, ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    let (name, _, plan) = crate::home::workspace_names(app);
    let mark = Rect::from_min_size(pos2(r.min.x, r.min.y + 8.0), vec2(40.0, 40.0));
    ui.painter().rect_filled(mark, CornerRadius::same(10), crate::home::WORKSPACE_MARK);
    let initial: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    ui.painter().text(mark.center(), Align2::CENTER_CENTER, initial, semibold(20.0), Color32::WHITE);
    let end = ui.painter().text(pos2(mark.max.x + 16.0, mark.center().y), Align2::LEFT_CENTER, &name, semibold(20.0), t.text).max.x;
    let chip_w = ui.painter().layout_no_wrap(plan.to_string(), regular(13.0), BRAND).size().x + 16.0;
    let chip = Rect::from_min_size(pos2(end + 12.0, mark.center().y - 12.0), vec2(chip_w, 24.0));
    ui.painter().rect_filled(chip, CornerRadius::same(5), BRAND.gamma_multiply(0.1));
    ui.painter().text(chip.center(), Align2::CENTER_CENTER, plan, regular(13.0), BRAND);
}

/// A folder's card: the pictures of its first three files in a row (an empty tile for each it
/// hasn't), the folder mark, its name and when it changed; a star on it when starred.
#[allow(clippy::too_many_arguments)]
fn folder_card(app: &FigApp, ui: &Ui, t: &Tokens, rect: Rect, resp: &Response, folder: &Folder, now: u64, time: f64) {
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(13), if resp.hovered() { t.hover } else { t.panel });
    p.rect_stroke(rect, CornerRadius::same(13), Stroke::new(1.0, if resp.has_focus() { BRAND } else { t.border }), StrokeKind::Inside);
    let inner = Rect::from_min_max(pos2(rect.min.x + 16.0, rect.min.y + 16.0), pos2(rect.max.x - 16.0, rect.min.y + PICTURES_H));
    let tile_w = (inner.width() - 2.0 * 12.0) / 3.0;
    for i in 0..3 {
        let tile = Rect::from_min_size(pos2(inner.min.x + i as f32 * (tile_w + 12.0), inner.min.y), vec2(tile_w, inner.height() - 8.0));
        let picture = folder.files.get(i).and_then(|f| app.ui.home_view.recents.cloud_picture(&f.target, time));
        match picture {
            Some((page, Some(texture), _)) => {
                p.rect_filled(tile, CornerRadius::same(6), page.unwrap_or(t.canvas));
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                let size = texture.size_vec2();
                let scale = (tile.width() / size.x).min(tile.height() / size.y);
                let shown = Rect::from_center_size(tile.center(), size * scale);
                p.image(texture.id(), shown, uv, Color32::WHITE);
            }
            _ => {
                p.rect_filled(tile, CornerRadius::same(6), t.canvas);
            }
        }
        p.rect_stroke(tile, CornerRadius::same(6), Stroke::new(1.0, t.border), StrokeKind::Inside);
    }
    let y = rect.max.y - FOOTER_H / 2.0;
    icons::paint(p, Rect::from_center_size(pos2(rect.min.x + 28.0, y), vec2(20.0, 20.0)), Icon::Folder, FOLDER_MARK);
    let text_x = rect.min.x + 48.0;
    let name_w = rect.max.x - 16.0 - text_x - if folder.starred { 20.0 } else { 0.0 };
    let name = crate::widgets::elide_font(ui, &folder.name, name_w, semibold(11.0));
    let end = p.text(pos2(text_x, y - 8.0), Align2::LEFT_CENTER, name, semibold(11.0), t.text).max.x;
    if folder.starred {
        icons::paint(p, Rect::from_center_size(pos2(end + 10.0, y - 8.0), vec2(11.0, 11.0)), Icon::Star, t.icon_secondary);
    }
    let when = trf("Updated {ago}", &[("ago", &ago(now, updated(app, folder)))]);
    p.text(pos2(text_x, y + 9.0), Align2::LEFT_CENTER, when, regular(11.0), t.text_secondary);
}

/// What a folder's menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FolderItem {
    Open,
    Star,
    Rename,
    Delete,
}

/// A folder's right-click menu: open it, star or unstar it, rename it, delete it.
fn folder_menu(app: &mut FigApp, resp: &Response, folder: &Folder) {
    let star = if folder.starred { tr("Remove from starred") } else { tr("Add to starred") };
    let items = [(tr("Open"), FolderItem::Open), (star, FolderItem::Star), (tr("Rename"), FolderItem::Rename), (tr("Delete"), FolderItem::Delete)];
    let Some(item) = home_menus::dark_context_menu(resp, &items) else { return };
    match item {
        FolderItem::Open => open_folder(app, &folder.id),
        FolderItem::Star => {
            app.run_user("folder.star", json!({"id": folder.id}));
        }
        FolderItem::Rename => app.ui.home_view.folder_dialogs.rename = Some((folder.id.clone(), folder.name.clone())),
        FolderItem::Delete => {
            if app.run_user("folder.delete", json!({"id": folder.id})).is_some() {
                app.toast(trf("Deleted {name}: its files are still in your recent files.", &[("name", &folder.name)]));
                if app.ui.home_view.folder.as_deref() == Some(folder.id.as_str()) {
                    app.ui.home_view.folder = None;
                }
            }
        }
    }
}

/// Show folder `id`'s files.
pub fn open_folder(app: &mut FigApp, id: &str) {
    app.ui.home_view.section = crate::home::Section::Folders;
    app.ui.home_view.folder = Some(id.to_string());
    app.ui.home_view.file_query.clear();
}

/// Open Create folder (with a file to put in the folder once it's made).
pub fn open_create(app: &mut FigApp, then_add: Option<FolderFile>) {
    let picked = then_add.iter().map(|f| f.target.clone()).collect();
    app.ui.home_view.folder_dialogs.create =
        Some(NewFolder { name: tr("Untitled").to_string(), access: "team", role: "edit", step: 1, picked, then_add });
}

/// Open Move to folder for `file` (a file on the account's server).
pub fn open_move(app: &mut FigApp, file: FolderFile) {
    let current = app.session.account.folder_of(&file.target).map(|f| f.id.clone());
    app.ui.home_view.folder_dialogs.move_file = Some((file, current));
}

/// A filled blue button (`label` centred), for the dialogs and "+ Folder".
pub fn primary_button(ui: &Ui, t: &Tokens, r: Rect, id: Id, label: &str) -> Response {
    let resp = ui.interact(r, id, Sense::click());
    a11y::button(&resp, label);
    ui.painter().rect_filled(r, CornerRadius::same(5), if resp.hovered() { crate::theme::ACCENT } else { BRAND });
    if resp.has_focus() {
        ui.painter().rect_stroke(r.expand(2.0), CornerRadius::same(7), Stroke::new(1.0, t.focus), StrokeKind::Outside);
    }
    ui.painter().text(r.center(), Align2::CENTER_CENTER, label, semibold(11.0), Color32::WHITE);
    resp
}

/// The folders' dialogs, when one is open: Create folder, Move to folder, Rename folder.
pub fn dialogs(app: &mut FigApp, ctx: &egui::Context) {
    create_dialog(app, ctx);
    move_dialog(app, ctx);
    rename_dialog(app, ctx);
}

/// The Create folder dialog's width, and its fields' height.
const DIALOG_W: f32 = 480.0;
const FIELD_H: f32 = 32.0;

/// Create folder, in two steps as the kit's: first its name, who has access (the workspace or only
/// you) and what they can do; then the files on the account's server to put in it. Folders aren't
/// shared with other accounts yet: the access chosen is kept with the folder.
fn create_dialog(app: &mut FigApp, ctx: &egui::Context) {
    let Some(mut d) = app.ui.home_view.folder_dialogs.create.clone() else { return };
    let t = Tokens::get(ctx);
    let team = crate::home::workspace_names(app).0;
    let files: Vec<(String, String, String)> =
        app.session.account.recents.iter().map(|r| (r.target.clone(), r.name.clone(), r.kind.clone())).collect();
    let (mut close, mut create) = (false, false);
    let frame = egui::Frame::NONE.fill(t.panel).corner_radius(CornerRadius::same(13)).shadow(egui::Shadow {
        offset: [0, 8],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(50),
    });
    let shown = egui::Modal::new(Id::unique("home_create_folder")).frame(frame).backdrop_color(Color32::from_black_alpha(60)).show(ctx, |ui| {
        ui.set_width(DIALOG_W);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // The header: the title, the close button, a rule.
        let (head, _) = ui.allocate_exact_size(vec2(DIALOG_W, 48.0), Sense::hover());
        ui.painter().text(pos2(head.min.x + 16.0, head.center().y), Align2::LEFT_CENTER, tr("Create new folder"), semibold(13.0), t.text);
        let x = Rect::from_center_size(pos2(head.max.x - 28.0, head.center().y), vec2(24.0, 24.0));
        let xr = ui.interact(x, Id::unique("home_create_folder_close"), Sense::click()).on_hover_text(tr("Close"));
        a11y::button(&xr, tr("Close"));
        if xr.hovered() {
            ui.painter().rect_filled(x, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(x.center(), vec2(16.0, 16.0)), Icon::Close, t.icon);
        close |= xr.clicked();
        ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, t.border));
        egui::Frame::NONE.inner_margin(egui::Margin::same(16)).show(ui, |ui| {
            ui.set_width(DIALOG_W - 32.0);
            if d.step == 1 {
                create_step_one(ui, &t, &mut d, &team);
            } else {
                create_step_two(ui, &t, &mut d, &files);
            }
        });
        // The footer: a rule, the step, and the buttons at the right.
        let (foot, _) = ui.allocate_exact_size(vec2(DIALOG_W, 56.0), Sense::hover());
        ui.painter().hline(foot.x_range(), foot.min.y, Stroke::new(1.0, t.border));
        let step = if d.step == 1 { tr("Step 1 of 2") } else { tr("Step 2 of 2") };
        ui.painter().text(pos2(foot.min.x + 16.0, foot.center().y), Align2::LEFT_CENTER, step, regular(12.0), t.text_secondary);
        let main = if d.step == 1 { tr("Continue") } else { tr("Create") };
        let main_w = ui.painter().layout_no_wrap(main.to_string(), regular(12.0), Color32::WHITE).size().x + 24.0;
        let main_r = Rect::from_min_size(pos2(foot.max.x - 16.0 - main_w, foot.center().y - 16.0), vec2(main_w, 32.0));
        let main_resp = primary_button(ui, &t, main_r, Id::unique("home_create_folder_main"), main);
        let other = if d.step == 1 { tr("Cancel") } else { tr("Back") };
        let other_w = ui.painter().layout_no_wrap(other.to_string(), regular(12.0), t.text).size().x + 24.0;
        let other_r = Rect::from_min_size(pos2(main_r.min.x - 8.0 - other_w, main_r.min.y), vec2(other_w, 32.0));
        let other_resp = ui.interact(other_r, Id::unique("home_create_folder_other"), Sense::click());
        a11y::button(&other_resp, other);
        if other_resp.hovered() {
            ui.painter().rect_filled(other_r, CornerRadius::same(5), t.hover);
        }
        ui.painter().text(other_r.center(), Align2::CENTER_CENTER, other, regular(12.0), t.text);
        if main_resp.clicked() {
            if d.step == 1 { d.step = 2 } else { create = true }
        }
        if other_resp.clicked() {
            if d.step == 1 { close = true } else { d.step = 1 }
        }
    });
    if create {
        app.ui.home_view.folder_dialogs.create = None;
        let Some(made) = app.run_user("folder.create", json!({"name": d.name, "access": d.access, "role": d.role})) else { return };
        let Some(id) = made.get("id").and_then(serde_json::Value::as_str).map(str::to_string) else { return };
        // The files picked go in, the first picked first.
        for target in d.picked.iter().rev() {
            let (name, kind) = files
                .iter()
                .find(|(t, ..)| t == target)
                .map(|(_, n, k)| (n.clone(), k.clone()))
                .or_else(|| d.then_add.as_ref().filter(|f| &f.target == target).map(|f| (f.name.clone(), f.kind.clone())))
                .unwrap_or_else(|| (tr("Untitled").to_string(), "design".into()));
            app.run_user("folder.addFile", json!({"id": id, "target": target, "name": name, "kind": kind}));
        }
        match d.then_add {
            Some(file) if d.picked.contains(&file.target) => {
                app.toast(trf("Moved {file} to {folder}", &[("file", &file.name), ("folder", &made["name"].as_str().unwrap_or_default())]));
            }
            _ => open_folder(app, &id),
        }
    } else if close || shown.should_close() {
        app.ui.home_view.folder_dialogs.create = None;
    } else {
        app.ui.home_view.folder_dialogs.create = Some(d);
    }
}

/// Step 1: the name (the folder mark beside it), who has access, what they can do.
fn create_step_one(ui: &mut Ui, t: &Tokens, d: &mut NewFolder, team: &str) {
    let w = ui.available_width();
    // The kit's 32 pt tile and field, 8 pt apart; the text 8 pt in, centred down the field
    // (typing or not).
    let (row, _) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::hover());
    let tile = Rect::from_min_size(row.min, vec2(FIELD_H, FIELD_H));
    ui.painter().rect_filled(tile, CornerRadius::same(5), t.field);
    icons::paint(ui.painter(), Rect::from_center_size(tile.center(), vec2(16.0, 16.0)), Icon::Folder, FOLDER_MARK);
    let field = Rect::from_min_max(pos2(tile.max.x + 8.0, row.min.y), row.max);
    ui.painter().rect_filled(field, CornerRadius::same(5), t.field);
    // Without a frame the edit has no margin of its own: it gets the field less 8 pt each side.
    let text = field.shrink2(vec2(8.0, 0.0));
    let edit = ui.put(
        text,
        egui::TextEdit::singleline(&mut d.name)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .min_size(text.size())
            .desired_width(text.width())
            .align(Align2::LEFT_CENTER)
            .font(regular(12.0))
            .id(Id::unique("home_create_folder_name")),
    );
    if ui.memory(|m| m.focused().is_none()) {
        edit.request_focus();
    }
    if edit.has_focus() {
        ui.painter().rect_stroke(field, CornerRadius::same(5), Stroke::new(1.0, BRAND), StrokeKind::Inside);
    }
    if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        d.step = 2;
    }
    ui.add_space(16.0);
    label(ui, tr("Who has access"), t.text);
    ui.add_space(8.0);
    // The dropdown: the workspace (its mark) or only you.
    // The kit's 32 pt box: the mark 8 pt in, the name after it, the chevron 8 pt from the end.
    let (r, resp) = ui.allocate_exact_size(vec2(w, FIELD_H), Sense::click());
    a11y::button(&resp, tr("Who has access"));
    ui.painter().rect_filled(r, CornerRadius::same(5), if resp.hovered() { t.hover } else { t.panel });
    ui.painter().rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
    let (icon, chosen) = if d.access == "team" { (Icon::Team, team.to_string()) } else { (Icon::Lock, tr("Only you").to_string()) };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(16.0, 16.0)), icon, t.icon);
    ui.painter().text(pos2(r.min.x + 32.0, r.center().y), Align2::LEFT_CENTER, chosen, regular(12.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.max.x - 16.0, r.center().y), vec2(12.0, 12.0)), Icon::Chevron, t.icon);
    crate::right::menu_popup(&resp).show(|ui| {
        if crate::right::menu_check(ui, d.access == "team", team, "") {
            d.access = "team";
            ui.close();
        }
        if crate::right::menu_check(ui, d.access == "private", tr("Only you"), "") {
            d.access = "private";
            ui.close();
        }
    });
    ui.add_space(8.0);
    let note = if d.access == "team" {
        tr("Shared folders aren't in FigCraft yet: until they are, only you see this folder.")
    } else {
        tr("Only you can see this folder.")
    };
    label(ui, note, t.text_secondary);
    if d.access == "team" {
        ui.add_space(16.0);
        label(ui, tr("What they can do"), t.text);
        ui.add_space(4.0);
        for (role, text) in [("view", tr("View and comment only")), ("edit", tr("Edit files, add people, and change their permissions"))] {
            if radio(ui, t, d.role == role, text) {
                d.role = role;
            }
        }
    }
}

/// Step 2: the files on the account's server, each with a box to put it in the folder.
fn create_step_two(ui: &mut Ui, t: &Tokens, d: &mut NewFolder, files: &[(String, String, String)]) {
    label(ui, &trf("Add files to {name}", &[("name", &d.name)]), t.text);
    ui.add_space(4.0);
    label(ui, tr("Pick files to put in the folder now, or add them later with Move file…"), t.text_secondary);
    ui.add_space(8.0);
    if files.is_empty() {
        label(ui, tr("No files on your account's server yet."), t.text_secondary);
        return;
    }
    egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
        for (target, name, kind) in files {
            let on = d.picked.contains(target);
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
            a11y::menu_item(&resp, name, "", Some(on));
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
            }
            let bx = Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(14.0, 14.0));
            ui.painter().rect_filled(bx, CornerRadius::same(3), if on { BRAND } else { t.panel });
            ui.painter().rect_stroke(bx, CornerRadius::same(3), Stroke::new(1.0, if on { BRAND } else { t.border }), StrokeKind::Inside);
            if on {
                icons::paint(ui.painter(), bx.shrink(1.0), Icon::Check, Color32::WHITE);
            }
            let mark = if kind == "board" { Icon::Board } else { Icon::PenNib };
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 40.0, r.center().y), vec2(14.0, 14.0)), mark, t.icon_secondary);
            ui.painter().text(pos2(r.min.x + 56.0, r.center().y), Align2::LEFT_CENTER, name, regular(12.0), t.text);
            if resp.clicked() {
                if on {
                    d.picked.retain(|p| p != target);
                } else {
                    d.picked.push(target.clone());
                }
            }
        }
    });
}

/// A line of text the dialog's width (wrapping).
fn label(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(egui::RichText::new(text).font(regular(12.0)).color(color)).wrap());
}

/// A radio button row: the kit's 16 pt circle (blue with a white dot when chosen) and its label.
/// Whether it was clicked.
fn radio(ui: &mut Ui, t: &Tokens, on: bool, text: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    a11y::toggle(&resp, text, on);
    let c = pos2(r.min.x + 8.0, r.center().y);
    if on {
        ui.painter().circle_filled(c, 8.0, BRAND);
        ui.painter().circle_filled(c, 3.0, Color32::WHITE);
    } else {
        ui.painter().circle_filled(c, 8.0, t.field);
        ui.painter().circle_stroke(c, 7.5, Stroke::new(1.0, if resp.hovered() { t.icon_secondary } else { t.border }));
    }
    ui.painter().text(pos2(r.min.x + 24.0, r.center().y), Align2::LEFT_CENTER, text, regular(12.0), t.text);
    resp.clicked()
}

/// Move to folder: the account's folders to pick from (the file's own ticked), New folder…, Cancel
/// and Move.
fn move_dialog(app: &mut FigApp, ctx: &egui::Context) {
    let Some((file, mut picked)) = app.ui.home_view.folder_dialogs.move_file.clone() else { return };
    let t = Tokens::get(ctx);
    let folders: Vec<(String, String)> = app.session.account.folders.iter().map(|f| (f.id.clone(), f.name.clone())).collect();
    let current = app.session.account.folder_of(&file.target).map(|f| f.id.clone());
    let (mut open, mut moved, mut cancel, mut new_folder, mut take_out) = (true, false, false, false, false);
    let title = trf("Move {name}", &[("name", &file.name)]);
    crate::dialogs::window(&title, ctx).id(Id::unique("home_move_file")).open(&mut open).show(ctx, |ui| {
        ui.set_width(320.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        if folders.is_empty() {
            crate::widgets::label(ui, tr("No folders yet: make one to move the file into."), t.text_secondary);
        }
        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
            for (id, name) in &folders {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                let on = picked.as_deref() == Some(id.as_str());
                a11y::menu_item(&resp, name, "", Some(on));
                if on {
                    ui.painter().rect_filled(r, CornerRadius::same(5), BRAND.gamma_multiply(0.12));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
                }
                icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 16.0, r.center().y), vec2(16.0, 16.0)), Icon::Folder, FOLDER_MARK);
                ui.painter().text(pos2(r.min.x + 36.0, r.center().y), Align2::LEFT_CENTER, name, regular(11.0), t.text);
                if current.as_deref() == Some(id.as_str()) {
                    ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, tr("Current"), regular(11.0), t.text_secondary);
                }
                if resp.clicked() {
                    picked = Some(id.clone());
                }
                if resp.double_clicked() {
                    moved = true;
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            new_folder = crate::widgets::button(ui, tr("New folder…"), false, true).clicked();
            if current.is_some() {
                take_out = crate::widgets::button(ui, tr("Take out of folder"), false, true).clicked();
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            cancel = crate::widgets::button(ui, tr("Cancel"), false, true).clicked();
            let can = picked.is_some() && picked != current;
            moved |= crate::widgets::button(ui, tr("Move"), true, can).clicked() && can;
        });
    });
    app.ui.home_view.folder_dialogs.move_file = None;
    if new_folder {
        open_create(app, Some(file));
    } else if take_out {
        app.run_user("folder.removeFile", json!({"target": file.target}));
        app.toast(trf("Took {file} out of its folder", &[("file", &file.name)]));
    } else if moved && let Some(id) = picked.clone().filter(|p| Some(p) != current.as_ref()) {
        if app.run_user("folder.addFile", json!({"id": id, "target": file.target, "name": file.name, "kind": file.kind})).is_some() {
            let folder = folders.iter().find(|(f, _)| *f == id).map(|(_, n)| n.clone()).unwrap_or_default();
            app.toast(trf("Moved {file} to {folder}", &[("file", &file.name), ("folder", &folder)]));
        }
    } else if !(cancel || !open) {
        app.ui.home_view.folder_dialogs.move_file = Some((file, picked));
    }
}

/// Rename folder: its name, Cancel and Rename.
fn rename_dialog(app: &mut FigApp, ctx: &egui::Context) {
    let Some((id, mut name)) = app.ui.home_view.folder_dialogs.rename.clone() else { return };
    let (mut open, mut done, mut cancel) = (true, false, false);
    crate::dialogs::window(tr("Rename folder"), ctx).id(Id::unique("home_rename_folder")).open(&mut open).show(ctx, |ui| {
        ui.set_width(300.0);
        let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0).id(Id::unique("home_rename_folder_name")));
        if ui.memory(|m| m.focused().is_none()) {
            field.request_focus();
        }
        done |= field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            cancel = crate::widgets::button(ui, tr("Cancel"), false, true).clicked();
            done |= crate::widgets::button(ui, tr("Rename"), true, true).clicked();
        });
    });
    app.ui.home_view.folder_dialogs.rename = None;
    if done {
        app.run_user("folder.rename", json!({"id": id, "name": name}));
    } else if !(cancel || !open) {
        app.ui.home_view.folder_dialogs.rename = Some((id, name));
    }
}

/// The folder page's heading: "All folders" (back to them) › the folder's name; its files follow
/// ([`crate::home_recents::section_in`]).
pub fn folder_heading(app: &mut FigApp, ui: &mut Ui, t: &Tokens, folder: &Folder) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    let back_w = ui.painter().layout_no_wrap(tr("All folders").to_string(), regular(13.0), t.text).size().x + 8.0;
    let back = Rect::from_min_size(pos2(r.min.x - 4.0, r.min.y + 8.0), vec2(back_w, 28.0));
    let resp = ui.interact(back, Id::unique("home_folder_back"), Sense::click());
    a11y::button(&resp, tr("All folders"));
    if resp.hovered() {
        ui.painter().rect_filled(back, CornerRadius::same(5), t.hover);
    }
    ui.painter().text(pos2(back.min.x + 4.0, back.center().y), Align2::LEFT_CENTER, tr("All folders"), regular(13.0), t.text_secondary);
    icons::paint(
        ui.painter(),
        Rect::from_center_size(pos2(back.max.x + 10.0, back.center().y), vec2(14.0, 14.0)),
        Icon::ChevronRight,
        t.icon_secondary,
    );
    let end = ui.painter().text(pos2(back.max.x + 22.0, back.center().y), Align2::LEFT_CENTER, &folder.name, semibold(13.0), t.text).max.x;
    if folder.starred {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(end + 10.0, back.center().y), vec2(11.0, 11.0)), Icon::Star, t.icon_secondary);
    }
    if resp.clicked() {
        app.ui.home_view.folder = None;
    }
}
