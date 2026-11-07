//! Home's dark menus, laid out after the UI3 kit's file browser: the account menu under your name
//! (who you are, the theme, settings, the desktop app, the account in use, adding one, logging out)
//! and a recent file's right-click menu (open it, copy its link, share, duplicate, its version
//! history, rename, remove it from the list). What FigCraft has no feature for (favourites,
//! folders, the trash, community profiles) says so when chosen instead of doing nothing.

use egui::{Align2, Color32, CornerRadius, Margin, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::FigApp;
use crate::a11y;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::{ACCENT, Tokens, regular, semibold};

/// The account menu's width, as the kit's.
pub const ACCOUNT_MENU_W: f32 = 246.0;
/// A one-line row, and a row with a name over an address.
const ROW_H: f32 = 24.0;
const TWO_LINE_H: f32 = 40.0;
/// Where a row's highlight starts and ends (in from the menu's sides), its mark's 24 pt box and
/// its label.
const INSET: f32 = 8.0;
const MARK_X: f32 = 32.0;
const LABEL_X: f32 = 60.0;
/// The card menu's rows start their labels here (it has no marks).
const PLAIN_LABEL_X: f32 = 16.0;
/// The Change theme submenu's width.
const THEME_MENU_W: f32 = 160.0;

/// Where FigCraft's desktop app is downloaded from.
pub const DESKTOP_APP_URL: &str = "https://github.com/storytold/figcraft/releases";

/// A Home menu's look: the dark menu fill with 13 pt corners, no outline and a soft shadow, the
/// rows edge to edge with `margin` over and under them. Its submenus (Change theme's) take it too.
fn dark(t: &Tokens, top: i8, bottom: i8) -> impl Fn(&mut egui::Style) + Send + Sync + 'static {
    let fill = t.menu;
    move |s: &mut egui::Style| {
        egui::containers::menu::menu_style(s);
        s.wrap_mode = Some(egui::TextWrapMode::Extend);
        s.visuals.window_fill = fill;
        s.visuals.window_stroke = Stroke::NONE;
        s.visuals.menu_corner_radius = CornerRadius::same(13);
        s.visuals.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(60) };
        s.spacing.menu_margin = Margin { left: 0, right: 0, top, bottom };
        s.spacing.item_spacing = vec2(0.0, 0.0);
    }
}

/// A row's area, its hover (or keyboard) highlight drawn; whether it is lit.
fn row_rect(ui: &mut Ui, t: &Tokens, h: f32, label: &str, enabled: bool) -> (Rect, Response, bool) {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, h), if enabled { Sense::click() } else { Sense::hover() });
    a11y::menu_item(&resp, label, "", None);
    if !enabled {
        a11y::disabled(&resp);
    }
    a11y::own_focus(&resp);
    let lit = enabled && (resp.hovered() || resp.has_focus());
    if lit {
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(r.min.x + INSET, r.min.y), pos2(r.max.x - INSET, r.max.y)),
            CornerRadius::same(5),
            t.menu_row_hover,
        );
    }
    (r, resp, lit)
}

/// A row with a mark in the 24 pt box before its label (none: the label lines up all the same),
/// and a chevron at its end for a submenu.
fn item(ui: &mut Ui, t: &Tokens, mark: Option<Icon>, label: &str, chevron: bool) -> Response {
    let (r, resp, _) = row_rect(ui, t, ROW_H, label, true);
    let p = ui.painter();
    if let Some(icon) = mark {
        icons::paint(p, Rect::from_min_size(pos2(r.min.x + MARK_X + 4.0, r.center().y - 8.0), vec2(16.0, 16.0)), icon, t.menu_text);
    }
    p.text(pos2(r.min.x + LABEL_X, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.menu_text);
    if chevron {
        icons::paint(p, Rect::from_center_size(pos2(r.max.x - INSET - 12.0, r.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, t.menu_text);
    }
    resp
}

/// A plain row (the card menu), its label at the left; dimmed and inert when not `enabled`.
fn plain(ui: &mut Ui, t: &Tokens, label: &str, enabled: bool) -> Response {
    let (r, resp, _) = row_rect(ui, t, ROW_H, label, enabled);
    let col = if enabled { t.menu_text } else { t.menu_dim };
    ui.painter().text(pos2(r.min.x + PLAIN_LABEL_X, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), col);
    resp
}

/// The line between groups of rows: 8 pt under the last group's rows, 7 pt over the next one's.
fn separator(ui: &mut Ui) {
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.0, Color32::from_white_alpha(25)));
    ui.add_space(7.0);
}

/// A round picture `r` in radius: the account's own picture when it is yours, else the initial on
/// the account colour.
fn avatar(app: &FigApp, p: &egui::Painter, center: egui::Pos2, r: f32, name: &str) {
    match &app.ui.account_picture {
        Some(picture) => crate::account_picture::paint(p, center, r, picture, Color32::TRANSPARENT),
        None => crate::live::avatar(p, center, r, name, ACCENT),
    }
}

/// What the account menu offers (one chosen, or none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AccountChoice {
    Theme(&'static str),
    Settings,
    DesktopApp,
    CommunityProfile,
    AddAccount,
    LogOut,
}

/// The account menu under the account's name (`anchor`): its picture, name and address; Change
/// theme (light, dark or the system's), Settings (Preferences), Get desktop app; Create a community
/// profile; the account in use under Switch account (FigCraft signs in one account at a time:
/// Add account signs this one out to sign another in); Log out.
pub fn account_menu(app: &mut FigApp, anchor: &Response, name: &str, email: &str) {
    let t = Tokens::get(&anchor.ctx);
    let mut chosen = None;
    let shown = egui::Popup::menu(anchor).style(dark(&t, 0, 8)).width(ACCOUNT_MENU_W).gap(4.0).align(egui::RectAlign::BOTTOM_START).show(|ui| {
        ui.set_width(ACCOUNT_MENU_W);
        // Who: the 40 pt picture, the name and the address, centred.
        let (head, _) = ui.allocate_exact_size(vec2(ACCOUNT_MENU_W, 112.0), Sense::hover());
        avatar(app, ui.painter(), pos2(head.center().x, head.min.y + 36.0), 20.0, name);
        ui.painter().text(pos2(head.center().x, head.min.y + 72.0), Align2::CENTER_CENTER, name, semibold(11.0), t.menu_text);
        ui.painter().text(pos2(head.center().x, head.min.y + 88.0), Align2::CENTER_CENTER, email, regular(11.0), t.menu_dim);
        separator(ui);
        let theme = item(ui, &t, Some(Icon::Panels), tr("Change theme"), true);
        if let Some(mode) = theme_submenu(ui, &t, &theme, &app.session.prefs.theme.clone()) {
            chosen = Some(AccountChoice::Theme(mode));
        }
        if item(ui, &t, Some(Icon::McpSettings), tr("Settings"), false).clicked() {
            chosen = Some(AccountChoice::Settings);
        }
        if item(ui, &t, Some(Icon::Download), tr("Get desktop app"), false).clicked() {
            chosen = Some(AccountChoice::DesktopApp);
        }
        separator(ui);
        if two_line(ui, &t, tr("Create a community profile"), email, false, |p, c| avatar(app, p, c, 8.0, name)).clicked() {
            chosen = Some(AccountChoice::CommunityProfile);
        }
        separator(ui);
        let (head, _) = ui.allocate_exact_size(vec2(ACCOUNT_MENU_W, ROW_H), Sense::hover());
        ui.painter().text(pos2(head.min.x + MARK_X, head.center().y), Align2::LEFT_CENTER, tr("Switch account"), regular(11.0), t.menu_dim);
        // The one account signed in here, ticked: choosing it keeps it.
        if two_line(ui, &t, name, email, true, |p, c| avatar(app, p, c, 8.0, name)).clicked() {
            ui.close();
        }
        separator(ui);
        if item(ui, &t, Some(Icon::Plus), tr("Add account"), false).clicked() {
            chosen = Some(AccountChoice::AddAccount);
        }
        if item(ui, &t, Some(Icon::SignOut), tr("Log out"), false).clicked() {
            chosen = Some(AccountChoice::LogOut);
        }
        if chosen.is_some() {
            ui.close();
        }
    });
    if shown.is_none() {
        return;
    }
    match chosen {
        Some(AccountChoice::Theme(mode)) => {
            app.run(mode, json!({}));
            app.services.save_prefs(&app.session.prefs);
        }
        Some(AccountChoice::Settings) => app.ui.preferences = true,
        Some(AccountChoice::DesktopApp) => app.services.open_url(DESKTOP_APP_URL),
        Some(AccountChoice::CommunityProfile) => app.toast(tr("Community profiles aren't in FigCraft yet.")),
        Some(AccountChoice::AddAccount | AccountChoice::LogOut) => app.services.sign_out(),
        None => {}
    }
}

/// A row with a 16 pt picture (`mark` paints it at the centre it is given), a name and an address
/// under it; a tick before the picture when `ticked`.
fn two_line(ui: &mut Ui, t: &Tokens, title: &str, sub: &str, ticked: bool, mark: impl FnOnce(&egui::Painter, egui::Pos2)) -> Response {
    let (r, resp, _) = row_rect(ui, t, TWO_LINE_H, title, true);
    let p = ui.painter();
    if ticked {
        icons::paint(p, Rect::from_center_size(pos2(r.min.x + INSET + 12.0, r.center().y), vec2(16.0, 16.0)), Icon::Check, t.menu_text);
    }
    mark(p, pos2(r.min.x + MARK_X + 12.0, r.center().y));
    let text_w = r.max.x - INSET - 8.0 - (r.min.x + LABEL_X);
    let clip = p.with_clip_rect(Rect::from_min_max(pos2(r.min.x + LABEL_X, r.min.y), pos2(r.min.x + LABEL_X + text_w, r.max.y)));
    clip.text(pos2(r.min.x + LABEL_X, r.center().y - 8.0), Align2::LEFT_CENTER, title, regular(11.0), t.menu_text);
    clip.text(pos2(r.min.x + LABEL_X, r.center().y + 8.0), Align2::LEFT_CENTER, sub, regular(11.0), t.menu_dim);
    resp
}

/// Change theme's submenu beside its row: Light, Dark and Follow system, the current one ticked.
/// The theme command chosen.
fn theme_submenu(ui: &mut Ui, t: &Tokens, row: &Response, current: &str) -> Option<&'static str> {
    let mut picked = None;
    egui::containers::menu::SubMenu::new().show(ui, row, |ui| {
        ui.set_width(THEME_MENU_W);
        ui.add_space(8.0);
        for (mode, label, cmd) in
            [("light", tr("Light"), "view.themeLight"), ("dark", tr("Dark"), "view.themeDark"), ("system", tr("Follow system"), "view.themeSystem")]
        {
            let (r, resp, _) = row_rect(ui, t, ROW_H, label, true);
            if current == mode {
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(r.min.x + INSET + 12.0, r.center().y), vec2(16.0, 16.0)),
                    Icon::Check,
                    t.menu_text,
                );
            }
            ui.painter().text(pos2(r.min.x + MARK_X, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.menu_text);
            if resp.clicked() {
                picked = Some(cmd);
                ui.close();
            }
        }
    });
    picked
}

/// A recent file's menu items, as the kit lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardItem {
    ShowInFolder,
    Open,
    OpenInNewTab,
    Favorite,
    CopyLink,
    Share,
    Duplicate,
    VersionHistory,
    Rename,
    Move,
    Trash,
    Remove,
}

/// What can be done to a file from its menu.
pub struct CardMenu<'a> {
    /// The platform's name for showing a file in its folder.
    pub reveal: &'a str,
    /// The file is on this computer's disk (Show in folder can show it).
    pub on_disk: bool,
    /// The file can leave the recent files (one open in a tab stays until it is closed).
    pub removable: bool,
}

/// A recent file's right-click menu on `resp` (its card or row): the item chosen.
pub fn card_menu(resp: &Response, menu: &CardMenu<'_>) -> Option<CardItem> {
    let t = Tokens::get(&resp.ctx);
    let mut chosen = None;
    egui::Popup::context_menu(resp).style(dark(&t, 8, 8)).width(150.0).show(|ui| {
        let mut row = |ui: &mut Ui, label: &str, enabled: bool, what: CardItem| {
            if plain(ui, &t, label, enabled).clicked() {
                chosen = Some(what);
            }
        };
        row(ui, menu.reveal, menu.on_disk, CardItem::ShowInFolder);
        row(ui, tr("Open"), true, CardItem::Open);
        row(ui, tr("Open in new tab"), true, CardItem::OpenInNewTab);
        separator(ui);
        row(ui, tr("Add to your favorites"), true, CardItem::Favorite);
        separator(ui);
        row(ui, tr("Copy link"), true, CardItem::CopyLink);
        row(ui, tr("Share"), true, CardItem::Share);
        row(ui, tr("Duplicate"), true, CardItem::Duplicate);
        separator(ui);
        row(ui, tr("Show version history"), true, CardItem::VersionHistory);
        row(ui, tr("Rename"), true, CardItem::Rename);
        row(ui, tr("Move file…"), true, CardItem::Move);
        row(ui, tr("Move to trash"), true, CardItem::Trash);
        row(ui, tr("Remove from recent"), menu.removable, CardItem::Remove);
        if chosen.is_some() {
            ui.close();
        }
    });
    chosen
}

/// A dark right-click menu on `resp` of plain rows (`items`: label and what it stands for): the one
/// chosen.
pub fn dark_context_menu<T: Copy>(resp: &Response, items: &[(&str, T)]) -> Option<T> {
    let t = Tokens::get(&resp.ctx);
    let mut chosen = None;
    egui::Popup::context_menu(resp).style(dark(&t, 8, 8)).width(150.0).show(|ui| {
        for (label, what) in items {
            if plain(ui, &t, label, true).clicked() {
                chosen = Some(*what);
            }
        }
        if chosen.is_some() {
            ui.close();
        }
    });
    chosen
}

/// What happens in the editor once a file chosen on Home is open there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Then {
    Share,
    Duplicate,
    VersionHistory,
    Rename,
}

impl Then {
    /// The menu item that asks for it.
    pub fn of(item: CardItem) -> Option<Then> {
        match item {
            CardItem::Share => Some(Then::Share),
            CardItem::Duplicate => Some(Then::Duplicate),
            CardItem::VersionHistory => Some(Then::VersionHistory),
            CardItem::Rename => Some(Then::Rename),
            _ => None,
        }
    }
}

/// Do `then` on the file in the editor (it was just opened or switched to).
pub fn run_then(app: &mut FigApp, then: Then) {
    app.ui.home = false;
    match then {
        Then::Share => app.ui.open_share = true,
        Then::Duplicate => {
            app.run_user("file.duplicate", json!({}));
        }
        Then::VersionHistory => {
            app.run_user("file.versionHistory", json!({}));
        }
        Then::Rename => app.ui.rename_file = true,
    }
}

/// Join `session` (just started from Home) has its file in a tab: show that tab and do what was
/// asked for it, if anything was.
pub fn joined(app: &mut FigApp, session: u64) {
    let Some((asked, then)) = app.ui.home_view.then else { return };
    if asked != session {
        return;
    }
    app.ui.home_view.then = None;
    let uid = app.session.live.get(session).and_then(|l| l.doc_uid);
    if let Some(index) = uid.and_then(|uid| app.session.docs.iter().position(|d| d.uid == uid)) {
        app.run("doc.switch", json!({"index": index}));
    }
    run_then(app, then);
}
