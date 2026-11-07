//! The Share dialog, as the incumbent's (the UI3 kit's "Share this file"): two cards in the middle
//! of the window.
//!
//! The first card: the title, Copy link and Close; a field for email addresses with Invite; then
//! who has access: anyone with the link (and what they may do: can edit, can comment, can view or
//! no access), you (the owner), and each person invited by email (what they may do, or remove
//! them). The second card: More (export, joining with a code, the local network, another server,
//! leaving or removing the file, what its server says about itself) and, signed in, Move to folder.
//!
//! A file of yours that isn't on the server yet goes there first, then the action asked for
//! happens ([`Pending`]): copying its link, inviting people, or giving the link an access. Everything
//! acts through the `collab.*` commands (`collab.access`, `collab.linkAccess`, `collab.invite`,
//! `collab.setAccess`, `collab.share`).

use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Stroke, Ui, pos2, vec2};
use figcraft_engine::account::FolderFile;
use figcraft_engine::live::{Access, Role, Status, Via};
use serde_json::{Value, json};

use crate::FigApp;
use crate::a11y;
use crate::i18n::{tr, trf};
use crate::icons::{self, Icon};
use crate::right::{menu_item, menu_item_with, menu_popup};
use crate::theme::{BRAND, Tokens, regular, semibold};

/// The dialog's width and its two cards' corner radius (the kit's).
const W: f32 = 480.0;
const RADIUS: u8 = 13;
/// A row of who has access, and of the second card.
const ROW_H: f32 = 36.0;
/// The body's inset from the card's sides.
const PAD: f32 = 16.0;
/// How far below the window's top the dialog sits, and its header's height.
const TOP: f32 = 96.0;
const HEAD_H: f32 = 40.0;

/// What the Share dialog keeps between frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShareDialog {
    /// The dialog shows.
    pub open: bool,
    /// What is typed in the invite field.
    pub emails: String,
    /// Who has access to the open file as the server last said (`collab.access`), for the file
    /// with this uid.
    pub access: Option<(u64, Value)>,
    /// The file isn't on the server yet: it is on its way, then this happens.
    pub pending: Option<Pending>,
    /// More ▸ About this server: what its server says shows in the second card.
    pub about: bool,
    /// Where the open file's server serves its browser build (`collab.webAddress`), for the server
    /// asked: `Some(None)` when it names none (Copy link then gives the app's own link).
    pub web: Option<(String, Option<String>)>,
    /// A dialog More opens, on the frame after this one closes (a modal keeps the keyboard for
    /// the frame it shows, so the other dialog's field couldn't take it).
    pub then: Option<Then>,
}

/// What More opens once the Share dialog has closed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Then {
    Publish,
    Join,
}

/// What waits for the open file to be on the server.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    CopyLink,
    Invite(Vec<String>),
}

/// The levels anyone with the link may have, and an invited person, as the menus list them.
const LEVELS: [(Access, &str); 3] = [(Access::Edit, "can edit"), (Access::Comment, "can comment"), (Access::View, "can view")];

fn level_text(a: Option<Access>) -> &'static str {
    match a {
        Some(Access::Edit) => tr("can edit"),
        Some(Access::Comment) => tr("can comment"),
        Some(Access::View) => tr("can view"),
        None => tr("no access"),
    }
}

/// Open the Share dialog (the Share button, Home's Share…).
pub fn open(app: &mut FigApp) {
    let d = &mut app.ui.live.share;
    d.open = true;
    d.about = false;
    d.access = None;
}

/// The open file's uid.
fn uid(app: &FigApp) -> Option<u64> {
    app.session.docs.get(app.session.active).map(|d| d.uid)
}

/// The open file is yours on a server.
fn owner(app: &FigApp) -> bool {
    figcraft_engine::live::owns_open_file(&app.session)
}

/// Ask the server who has access (once per file while the dialog is open, and after each change).
fn refresh(app: &mut FigApp, ctx: &egui::Context) {
    let Some(uid) = uid(app) else { return };
    app.run_background(
        ctx,
        "collab.access",
        json!({}),
        Box::new(move |app, _, v| {
            if uid_now(app) == Some(uid) {
                app.ui.live.share.access = Some((uid, v));
            }
        }),
    );
}

fn uid_now(app: &FigApp) -> Option<u64> {
    uid(app)
}

/// Each frame: what waited for the file to be on the server happens once it is; the dialog when
/// it is open.
pub fn show(app: &mut FigApp, ctx: &egui::Context) {
    pending(app, ctx);
    if !app.ui.live.share.open {
        match app.ui.live.share.then.take() {
            Some(Then::Publish) => super::open_publish(app),
            Some(Then::Join) => super::open_join(app, ""),
            None => {}
        }
    }
    // A file opened from a Dev Mode link shows in Dev Mode once it is there.
    if let Some(l) = app.session.live_mut().filter(|l| l.dev && l.syncing()) {
        l.dev = false;
        app.ui.right_tab = crate::RightTab::Inspect;
    }
    // Move to folder's dialog, over the editor (Home draws its own).
    crate::home::folders::dialogs(app, ctx);
    if !app.ui.live.share.open {
        return;
    }
    if !app.has_files() || app.ui.home {
        app.ui.live.share.open = false;
        return;
    }
    ask_web(app, ctx);
    let shown = uid(app);
    if owner(app) && app.ui.live.share.access.as_ref().is_none_or(|(u, _)| Some(*u) != shown) && !app.session.waiting_for("collab.access") {
        app.ui.live.share.access = shown.map(|u| (u, Value::Null));
        refresh(app, ctx);
    }
    let t = Tokens::get(ctx);
    let mut close = false;
    // Near the top of the window, the canvas behind it as it was (the kit's).
    let id = Id::unique("share_dialog");
    let area = egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0.0, TOP));
    let shown = egui::Modal::new(id).area(area).frame(egui::Frame::NONE).backdrop_color(Color32::TRANSPARENT).show(ctx, |ui| {
        ui.set_width(W);
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        card(ui, &t, |ui| close |= first_card(app, ui, &t));
        ui.add_space(8.0);
        card(ui, &t, |ui| second_card(app, ui, &t));
    });
    // Escape or a click outside closes it, but not while a menu of it is open.
    let menu_open = egui::Popup::is_any_open(ctx);
    if close || (shown.should_close() && !menu_open) {
        app.ui.live.share.open = false;
    }
}

/// A white card with the kit's corners and shadow.
fn card(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui)) {
    let frame = egui::Frame::NONE.fill(t.panel).corner_radius(CornerRadius::same(RADIUS)).shadow(egui::Shadow {
        offset: [0, 8],
        blur: 32,
        spread: 0,
        color: Color32::from_black_alpha(50),
    });
    frame.show(ui, |ui| {
        ui.set_width(W);
        add(ui);
    });
}

/// The header, the invite field and who has access. Whether Close was clicked.
fn first_card(app: &mut FigApp, ui: &mut Ui, t: &Tokens) -> bool {
    let (head, _) = ui.allocate_exact_size(vec2(W, HEAD_H), Sense::hover());
    ui.painter().text(pos2(head.min.x + PAD, head.center().y), Align2::LEFT_CENTER, tr("Share this file"), semibold(11.0), t.text);
    let x = Rect::from_center_size(pos2(head.max.x - 20.0, head.center().y), vec2(24.0, 24.0));
    let xr = ui.interact(x, Id::unique("share_dialog_close"), Sense::click()).on_hover_text(tr("Close"));
    a11y::button(&xr, tr("Close"));
    if xr.hovered() {
        ui.painter().rect_filled(x, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(x.center(), vec2(16.0, 16.0)), Icon::Close, t.icon);
    copy_link_button(app, ui, pos2(x.min.x - 6.0, head.center().y));
    ui.painter().hline(head.x_range(), head.max.y, Stroke::new(1.0, t.border));
    egui::Frame::NONE.inner_margin(egui::Margin { left: PAD as i8, right: PAD as i8, top: 8, bottom: 8 }).show(ui, |ui| {
        ui.set_width(W - 2.0 * PAD);
        if let Some((line, color)) = status_line(app, t) {
            ui.add_space(4.0);
            ui.add(egui::Label::new(egui::RichText::new(line).font(regular(11.0)).color(color)).wrap());
            ui.add_space(4.0);
        }
        invite_row(app, ui, t);
        who_has_access(app, ui, t);
    });
    xr.clicked()
}

/// What the file is doing, when the dialog has something to say about it: on its way to the
/// server, reconnecting, failed, or sharing switched off.
fn status_line(app: &FigApp, t: &Tokens) -> Option<(String, Color32)> {
    let live = app.session.live();
    match &live.status {
        Status::Connecting if live.quiet => None,
        Status::Connecting => Some((tr("Putting the file online…").to_string(), t.text_secondary)),
        Status::Reconnecting { attempt } => {
            Some((trf("Connection lost, reconnecting (attempt {attempt})…", &[("attempt", attempt)]), t.text_secondary))
        }
        Status::Failed { reason } => Some((super::failure_text(reason).into_owned(), t.danger)),
        Status::Off if !can_share_online(app) => Some((tr(OFFLINE_NOTE).to_string(), t.text_secondary)),
        _ => None,
    }
}

const OFFLINE_NOTE: &str =
    crate::i18n::tr_noop("Online sharing is off: no server is set in Preferences. More has the local network and joining a file.");

/// Sharing online is possible here: a server is set (the page's own, in the browser).
fn can_share_online(app: &FigApp) -> bool {
    !app.session.prefs.publish_server.trim().is_empty() && app.session.live.available()
}

/// The open file isn't in a session yet, and could go to the server.
fn not_yet_online(app: &FigApp) -> bool {
    let live = app.session.live();
    matches!(live.status, Status::Off | Status::Failed { .. }) && can_share_online(app)
}

/// Put the open file on the server (then `then` happens).
fn go_online(app: &mut FigApp, then: Option<Pending>, access: Option<Access>) {
    let mut params = json!({"via": "online"});
    if let Some(a) = access {
        params["access"] = json!(a.id());
    }
    if app.run("collab.share", params).is_some() {
        app.ui.live.share.pending = then;
    }
}

/// What waited for the open file to be on the server, once it is (an upload that failed drops
/// it: the dialog says why).
fn pending(app: &mut FigApp, ctx: &egui::Context) {
    let Some(p) = app.ui.live.share.pending.clone() else { return };
    let live = app.session.live();
    if matches!(live.status, Status::Failed { .. } | Status::Off) {
        app.ui.live.share.pending = None;
        return;
    }
    if !owner(app) || live.share.as_ref().is_none_or(|s| s.links.is_none()) {
        return;
    }
    // The link is given as the server's browser address, once the server has said where that is.
    if p == Pending::CopyLink && app.services.file_link("figcraft://join").is_none() && web_now(app).is_none() {
        ask_web(app, ctx);
        if app.session.waiting_for("collab.webAddress") {
            return;
        }
    }
    app.ui.live.share.pending = None;
    match p {
        Pending::CopyLink => copy_link(app),
        Pending::Invite(emails) => invite(app, ctx, emails),
    }
}

/// The link Copy link gives, with what the toast says: anyone's link while it gives access, the
/// session's own on the local network, else the one you joined with.
fn link(app: &FigApp) -> Option<String> {
    let live = app.session.live();
    if live.status != Status::Live {
        return None;
    }
    if let Some(l) = live.share.as_ref().and_then(|s| s.link()).filter(|l| !l.is_empty()) {
        return Some(l.to_string());
    }
    if owner(app) && live.share.is_some() {
        // Yours, with no access for anyone with a link: there's no link to give.
        return None;
    }
    // Published to a server (File ▸ Publish to server…): its edit link.
    if let Some(p) = live.published.as_ref().filter(|p| !p.edit.is_empty()) {
        return Some(p.edit.clone());
    }
    if let Some(i) = live.info.as_ref().filter(|i| i.role == Role::Host && !i.link.is_empty()) {
        return Some(i.link.clone());
    }
    // A file you joined: the link it was opened with (the browser's address names it).
    uid(app).and_then(|u| app.session.account.recent_for_tab(u)).map(|r| r.target.clone()).filter(|t| t.contains("://"))
}

/// The address to give for join link `link`, the same in the desktop app and the browser: the
/// file's address in its server's browser build (`https://…/#file=…`, which opens in any browser,
/// as the incumbent's links do). In the browser that is the page's own address; in the desktop app
/// the server says where its browser build is. A server that names none, and the local network,
/// give the link itself (`figcraft://…`, which opens the app).
pub fn shareable(app: &FigApp, link: &str) -> String {
    if let Some(l) = app.services.file_link(link) {
        return l;
    }
    match web_now(app) {
        Some(Some(web)) if link.starts_with("figcraft://join?server=") => figcraft_engine::team::file_link(&web, link),
        _ => link.to_string(),
    }
}

/// The server the open file is on (or would go to), as `collab.webAddress` is asked about.
fn server_now(app: &FigApp) -> Option<String> {
    let live = app.session.live();
    let server = live.server_file().map(|(s, _)| s).or_else(|| live.share.as_ref().filter(|s| s.via == Via::Online).map(|s| s.server.clone()));
    server.or_else(|| Some(app.session.prefs.publish_server.trim().to_string())).filter(|s| !s.is_empty())
}

/// What the open file's server said about its browser build: `None` while it isn't known yet.
fn web_now(app: &FigApp) -> Option<Option<String>> {
    let server = server_now(app)?;
    app.ui.live.share.web.as_ref().filter(|(at, _)| *at == server).map(|(_, w)| w.clone())
}

/// Ask the open file's server where its browser build is, once per server.
fn ask_web(app: &mut FigApp, ctx: &egui::Context) {
    let Some(server) = server_now(app) else { return };
    if app.services.file_link("figcraft://join").is_some() || web_now(app).is_some() || app.session.waiting_for("collab.webAddress") {
        return;
    }
    let asked = server.clone();
    app.run_background(
        ctx,
        "collab.webAddress",
        json!({"server": server}),
        Box::new(move |app, _, v| {
            app.ui.live.share.web = Some((asked, v["web"].as_str().map(str::to_string)));
        }),
    );
}

/// The join link that opens the file in Dev Mode (the Inspect tab): the link with `dev=1`.
pub fn dev_link(link: &str) -> String {
    format!("{link}{}dev=1", if link.contains('?') { "&" } else { "?" })
}

fn copy_link(app: &mut FigApp) {
    if let Some(l) = link(app) {
        let l = shareable(app, &l);
        app.clipboard_set_text(&l);
        app.toast(tr("Link copied"));
    }
}

/// "Copy link" with its link icon, ending at `right`.
fn copy_link_button(app: &mut FigApp, ui: &mut Ui, right: egui::Pos2) {
    let has = link(app).is_some();
    let goes_online = !has && not_yet_online(app) && app.ui.live.share.pending.is_none();
    if !has && !goes_online {
        return;
    }
    let galley = ui.painter().layout_no_wrap(tr("Copy link").to_string(), regular(11.0), BRAND);
    let w = galley.size().x + 26.0;
    let r = Rect::from_min_max(pos2(right.x - w - 8.0, right.y - 12.0), pos2(right.x, right.y + 12.0));
    let resp =
        ui.interact(r, Id::unique("share_dialog_copy_link"), Sense::click()).on_hover_text(tr("Anyone with the link gets in as the file allows"));
    a11y::button(&resp, tr("Copy link"));
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), Tokens::get(ui.ctx()).hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 11.0, r.center().y), vec2(14.0, 14.0)), Icon::Link, BRAND);
    ui.painter().galley(pos2(r.min.x + 22.0, r.center().y - galley.size().y / 2.0), galley, BRAND);
    if resp.clicked() {
        if has {
            copy_link(app);
        } else {
            go_online(app, Some(Pending::CopyLink), None);
        }
    }
}

/// The addresses typed in the invite field (commas, spaces or semicolons between them).
pub fn typed_emails(s: &str) -> Vec<String> {
    s.split([',', ';', ' ', '\n', '\t']).map(str::trim).filter(|e| !e.is_empty()).map(str::to_string).collect()
}

/// The field for email addresses and Invite. Only the file's owner invites; a file not yet on the
/// server goes there first.
fn invite_row(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    let may = owner(app) || not_yet_online(app);
    let busy = app.session.waiting_for("collab.invite") || app.ui.live.share.pending.is_some();
    let (row, _) = ui.allocate_exact_size(vec2(W - 2.0 * PAD, 50.0), Sense::hover());
    let label = if busy { tr("Inviting…") } else { tr("Invite") };
    let bw = (ui.painter().layout_no_wrap(label.to_string(), semibold(11.0), Color32::WHITE).size().x + 24.0).max(54.0);
    let button = Rect::from_min_size(pos2(row.max.x - bw, row.min.y + 9.0), vec2(bw, 32.0));
    let field = Rect::from_min_max(pos2(row.min.x, row.min.y + 8.0), pos2(button.min.x - 8.0, row.min.y + 42.0));
    ui.painter().rect_filled(field, CornerRadius::same(5), t.field);
    let text = field.shrink2(vec2(10.0, 0.0));
    let mut typed = std::mem::take(&mut app.ui.live.share.emails);
    let hint = if may { tr("Add comma separated emails to invite") } else { tr("Only the file's owner can invite people") };
    let edit = ui.add_enabled_ui(may && !busy, |ui| {
        ui.put(
            text,
            egui::TextEdit::singleline(&mut typed)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .min_size(text.size())
                .desired_width(text.width())
                .align(Align2::LEFT_CENTER)
                .font(regular(11.0))
                .hint_text(egui::RichText::new(hint).color(t.text_tertiary))
                .char_limit(2000)
                .id(Id::unique("share_dialog_emails")),
        )
    });
    let edit = edit.inner;
    a11y::text(&edit, tr("Emails to invite"), &typed);
    let emails = typed_emails(&typed);
    let ready = may && !busy && !emails.is_empty();
    let resp = ui.interact(button, Id::unique("share_dialog_invite"), if ready { Sense::click() } else { Sense::hover() });
    a11y::button(&resp, label);
    let fill = if ready { if resp.hovered() { crate::theme::ACCENT } else { BRAND } } else { t.hover };
    ui.painter().rect_filled(button, CornerRadius::same(5), fill);
    ui.painter().text(button.center(), Align2::CENTER_CENTER, label, semibold(11.0), if ready { Color32::WHITE } else { t.text_tertiary });
    let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if ready && (resp.clicked() || enter) {
        if owner(app) {
            invite(app, &ui.ctx().clone(), emails);
        } else {
            go_online(app, Some(Pending::Invite(emails)), None);
        }
        typed.clear();
    }
    app.ui.live.share.emails = typed;
}

/// Invite `emails` to the open file (yours on the server).
fn invite(app: &mut FigApp, ctx: &egui::Context, emails: Vec<String>) {
    let n = emails.len();
    app.run_background(
        ctx,
        "collab.invite",
        json!({"emails": emails}),
        Box::new(move |app, ctx, v| {
            let delivered = v["delivered"] == true;
            let given: Vec<(String, String)> = v["invited"]
                .as_array()
                .map(|a| a.iter().filter_map(|p| Some((p["email"].as_str()?.to_string(), shareable(app, p["link"].as_str()?)))).collect())
                .unwrap_or_default();
            if delivered {
                app.toast(crate::i18n::trn("Invited {n} people: the file is on their Home", n, &[]));
            } else if let [(_, link)] = given.as_slice() {
                // Signed out (the desktop app): their link, to send them.
                app.clipboard_set_text(link);
                app.toast(tr("Invited: their link is copied, to send them"));
            } else {
                let lines: Vec<String> = given.iter().map(|(e, l)| format!("{e}: {l}")).collect();
                app.clipboard_set_text(&lines.join("\n"));
                app.toast(crate::i18n::trn("Invited {n} people: their links are copied, to send them", n, &[]));
            }
            refresh(app, ctx);
        }),
    );
}

/// "Who has access": anyone with the link, you, and the people invited.
fn who_has_access(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(W - 2.0 * PAD, 28.0), Sense::hover());
    ui.painter().text(pos2(r.min.x, r.center().y), Align2::LEFT_CENTER, tr("Who has access"), regular(11.0), t.text_secondary);
    let live = app.session.live();
    let local = live.share.as_ref().is_some_and(|s| s.via == Via::Local) || live.role() == Some(Role::Host);
    let level = live.share.as_ref().filter(|s| s.links.is_some()).map(|s| s.access);
    let connecting = live.status == Status::Connecting;
    let guest_access = live.info.as_ref().map_or(Access::Edit, |i| i.access);
    let me = my_name(app);
    let mine = owner(app) || not_yet_online(app) || local || connecting;
    if local {
        person_row(ui, t, Who::Anyone, tr("Anyone on your network with the link"), level_text(Some(Access::Edit)), None);
    } else if mine {
        let picked = person_row(ui, t, Who::Anyone, tr("Anyone"), level_text(level), Some((Id::unique("share_dialog_anyone"), level, false)));
        if let Some(choice) = picked {
            link_access(app, &ui.ctx().clone(), choice);
        }
    }
    let mine_text = if mine { tr("owner") } else { level_text(Some(guest_access)) };
    let my_fill = app.session.live().info.as_ref().map_or(t.success, |i| super::color(&i.color));
    person_row(ui, t, Who::Person(&me, true, my_fill), &me, mine_text, None);
    // In someone else's file only to look or comment: why the tools are fewer.
    if !mine && live_now(app) {
        match guest_access {
            Access::View => crate::widgets::label(ui, tr(super::VIEW_NOTE), t.text_secondary),
            Access::Comment => crate::widgets::label(ui, tr(super::COMMENT_NOTE), t.text_secondary),
            Access::Edit => {}
        }
    }
    let people: Vec<(String, Option<Access>)> = app
        .ui
        .live
        .share
        .access
        .as_ref()
        .and_then(|(_, v)| v["people"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|p| Some((p["email"].as_str()?.to_string(), p["access"].as_str().and_then(Access::parse))))
        .collect();
    for (email, access) in people {
        let id = Id::unique("share_dialog_person").with(&email);
        if let Some(choice) = person_row(ui, t, Who::Person(&email, false, person_fill(&email)), &email, level_text(access), Some((id, access, true)))
        {
            let params = json!({"email": email, "access": choice.map_or("remove", Access::id)});
            app.run_background(&ui.ctx().clone(), "collab.setAccess", params, Box::new(|app, ctx, _| refresh(app, ctx)));
        }
    }
}

/// The open file is in a session that is live.
fn live_now(app: &FigApp) -> bool {
    app.session.live().status == Status::Live
}

/// An invited person's avatar colour: one of the people colours, the same for an address every time.
fn person_fill(email: &str) -> Color32 {
    let h = email.bytes().fold(0usize, |h, b| h.wrapping_mul(31).wrapping_add(usize::from(b)));
    super::people(h)
}

/// Your name as the others see it: the session's, else the account's, else the one in
/// Preferences ("You" while it is the default).
fn my_name(app: &FigApp) -> String {
    let live = app.session.live();
    let from_session = live.info.as_ref().map(|i| i.name.trim().to_string()).filter(|n| !n.is_empty());
    let from_account = app.session.account.account.as_ref().map(|a| a.name.trim().to_string()).filter(|n| !n.is_empty());
    from_session
        .or(from_account)
        .or_else(|| Some(figcraft_engine::live::display_name(&app.session.prefs)))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| tr("You").to_string())
}

/// Give anyone with the link `choice` (`None`: no access).
fn link_access(app: &mut FigApp, ctx: &egui::Context, choice: Option<Access>) {
    if not_yet_online(app) {
        if let Some(a) = choice {
            go_online(app, None, Some(a));
        }
        return;
    }
    let params = json!({"access": choice.map_or("none", Access::id)});
    app.run_background(ctx, "collab.linkAccess", params, Box::new(|app, ctx, _| refresh(app, ctx)));
}

/// Whose row: anyone with the link (a globe), or a person (their initial; you in bold).
enum Who<'a> {
    Anyone,
    Person(&'a str, bool, Color32),
}

/// A row of who has access: the mark, the name, and on the right what they may do; with `menu`
/// (its id, the level now, and whether Remove is offered instead of No access) the right side
/// opens a menu of levels. The level chosen (`None`: no access, or removed), when one was.
fn person_row(ui: &mut Ui, t: &Tokens, who: Who, name: &str, level: &str, menu: Option<(Id, Option<Access>, bool)>) -> Option<Option<Access>> {
    let (row, _) = ui.allocate_exact_size(vec2(W - 2.0 * PAD, ROW_H), Sense::hover());
    let mark = Rect::from_center_size(pos2(row.min.x + 12.0, row.center().y), vec2(24.0, 24.0));
    match who {
        Who::Anyone => icons::paint(ui.painter(), Rect::from_center_size(mark.center(), vec2(16.0, 16.0)), Icon::Globe, t.icon),
        Who::Person(n, _, fill) => super::avatar(ui.painter(), mark.center(), 10.0, n, fill),
    }
    let text_x = row.min.x + 36.0;
    let level_w = ui.painter().layout_no_wrap(level.to_string(), regular(11.0), t.text).size().x + if menu.is_some() { 24.0 } else { 0.0 };
    let name_w = row.max.x - text_x - level_w - 16.0;
    let shown = crate::widgets::elide_font(ui, name, name_w, regular(11.0));
    let me = matches!(who, Who::Person(_, true, _));
    let font = if me { semibold(11.0) } else { regular(11.0) };
    let g = ui.painter().layout_no_wrap(shown, font, t.text);
    let gw = g.size().x;
    ui.painter().galley(pos2(text_x, row.center().y - g.size().y / 2.0), g, t.text);
    if me && name != tr("You") {
        ui.painter().text(pos2(text_x + gw + 4.0, row.center().y), Align2::LEFT_CENTER, tr("(you)"), regular(11.0), t.text_secondary);
    }
    let Some((id, now, removable)) = menu else {
        ui.painter().text(pos2(row.max.x, row.center().y), Align2::RIGHT_CENTER, level, regular(11.0), t.text);
        return None;
    };
    let hit = Rect::from_min_max(pos2(row.max.x - level_w - 8.0, row.min.y + 4.0), pos2(row.max.x + 4.0, row.max.y - 4.0));
    let resp = ui.interact(hit, id, Sense::click()).on_hover_text(name);
    a11y::button(&resp, &trf("{name}: {level}", &[("name", &name), ("level", &level)]));
    if resp.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp)) {
        ui.painter().rect_filled(hit, CornerRadius::same(5), t.hover);
    }
    ui.painter().text(pos2(row.max.x - 20.0, row.center().y), Align2::RIGHT_CENTER, level, regular(11.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(row.max.x - 8.0, row.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, t.icon);
    let mut picked = None;
    menu_popup(&resp).width(160.0).show(|ui| {
        for (a, label) in LEVELS {
            let on = now == Some(a);
            let clicked = menu_item_with(ui, tr(label), "", 16.0, |p, r, c| {
                if on {
                    icons::paint(p, Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(14.0, 14.0)), Icon::Check, c);
                }
            });
            if clicked {
                picked = Some(Some(a));
                ui.close();
            }
        }
        ui.separator();
        let last = if removable { tr("Remove") } else { tr("No access") };
        let on = now.is_none() && !removable;
        let clicked = menu_item_with(ui, last, "", 16.0, |p, r, c| {
            if on {
                icons::paint(p, Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(14.0, 14.0)), Icon::Check, c);
            }
        });
        if clicked {
            picked = Some(None);
            ui.close();
        }
    });
    picked.filter(|p| *p != now || (removable && p.is_none()))
}

/// More, and Move to folder; what More's About this server and Report this file open.
fn second_card(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    ui.add_space(8.0);
    if let Some(l) = link(app) {
        let resp = list_row(ui, t, Id::unique("share_dialog_dev_link"), Icon::Code, tr("Copy Dev Mode link"), t.text, false);
        if resp.clicked() {
            let l = shareable(app, &dev_link(&l));
            app.clipboard_set_text(&l);
            app.toast(tr("Dev Mode link copied"));
        }
    }
    let more = list_row(ui, t, Id::unique("share_dialog_more"), Icon::Dots, tr("More"), t.text_secondary, true);
    more_menu(app, &more);
    if let Some(file) = movable(app) {
        let (r, _) = ui.allocate_exact_size(vec2(W, ROW_H), Sense::hover());
        let row = Rect::from_min_max(pos2(r.min.x + 8.0, r.min.y), pos2(r.max.x - 8.0, r.max.y));
        let resp = ui.interact(row, Id::unique("share_dialog_move"), Sense::click());
        a11y::button(&resp, tr("Move to folder"));
        let fill = if resp.hovered() { BRAND.gamma_multiply(0.18) } else { BRAND.gamma_multiply(0.1) };
        ui.painter().rect_filled(row, CornerRadius::same(5), fill);
        icons::paint(ui.painter(), Rect::from_center_size(pos2(row.min.x + 17.0, row.center().y), vec2(16.0, 16.0)), Icon::MoveToFolder, t.icon);
        ui.painter().text(pos2(row.min.x + 37.0, row.center().y), Align2::LEFT_CENTER, tr("Ready to share with your team?"), regular(11.0), t.text);
        ui.painter().text(pos2(row.max.x - 8.0, row.center().y), Align2::RIGHT_CENTER, tr("Move to folder"), regular(11.0), BRAND);
        if resp.clicked() {
            crate::home::folders::open_move(app, file);
            app.ui.live.share.open = false;
        }
    }
    egui::Frame::NONE.inner_margin(egui::Margin { left: PAD as i8, right: PAD as i8, top: 0, bottom: 0 }).show(ui, |ui| {
        ui.set_width(W - 2.0 * PAD);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        if app.ui.live.share.about {
            about(app, ui, t);
        }
        if app.ui.live.report.is_some() {
            super::report_ui(app, ui, t);
        }
        if app.ui.live.confirm_remove {
            confirm_remove(app, ui, t);
        }
    });
    ui.add_space(8.0);
}

/// A row of the second card: its icon, its label, and a chevron when it opens a menu.
fn list_row(ui: &mut Ui, t: &Tokens, id: Id, icon: Icon, label: &str, color: Color32, chevron: bool) -> egui::Response {
    let (r, _) = ui.allocate_exact_size(vec2(W, ROW_H), Sense::hover());
    let row = Rect::from_min_max(pos2(r.min.x + 8.0, r.min.y), pos2(r.max.x - 8.0, r.max.y));
    let resp = ui.interact(row, id, Sense::click());
    a11y::button(&resp, label);
    if resp.hovered() || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp)) {
        ui.painter().rect_filled(row, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(row.min.x + 17.0, row.center().y), vec2(16.0, 16.0)), icon, t.icon);
    ui.painter().text(pos2(row.min.x + 37.0, row.center().y), Align2::LEFT_CENTER, label, regular(11.0), color);
    if chevron {
        icons::paint(ui.painter(), Rect::from_center_size(pos2(row.max.x - 12.0, row.center().y), vec2(16.0, 16.0)), Icon::ChevronRight, t.icon);
    }
    resp
}

/// The open file as a folder takes it, when it can go in one: signed in, on the account's list,
/// and not in a folder yet.
fn movable(app: &FigApp) -> Option<FolderFile> {
    app.session.account.account.as_ref()?;
    let r = uid(app).and_then(|u| app.session.account.recent_for_tab(u))?;
    if app.session.account.folder_of(&r.target).is_some() {
        return None;
    }
    Some(FolderFile { target: r.target.clone(), name: r.name.clone(), kind: r.kind.clone() })
}

/// More: everything that isn't who has access.
fn more_menu(app: &mut FigApp, anchor: &egui::Response) {
    menu_popup(anchor).width(240.0).show(|ui| {
        let live = app.session.live();
        let status = live.status.clone();
        let share = live.share.clone().filter(|_| status.active());
        let local = share.as_ref().is_some_and(|s| s.via == Via::Local);
        let hosting = live.hosting.is_some();
        let in_session = status.active();
        let code = share.as_ref().and_then(|s| s.code());
        if menu_item(ui, tr("Export…"), "") {
            app.ui.export_preview = true;
            app.ui.live.share.open = false;
            ui.close();
        }
        // A file published to a server (File ▸ Publish to server…): its two links.
        let published = live.published.clone().filter(|_| share.is_none());
        if let Some(p) = published {
            for (label, link, done) in
                [(tr("Copy edit link"), p.edit, tr("Edit link copied")), (tr("Copy view link"), p.view, tr("View link copied"))]
            {
                if !link.is_empty() && menu_item(ui, label, "") {
                    let l = shareable(app, &link);
                    app.clipboard_set_text(&l);
                    app.toast(done);
                    ui.close();
                }
            }
        }
        if let Some(code) = code
            && menu_item(ui, tr("Copy join code"), "")
        {
            app.clipboard_set_text(&code);
            app.toast(trf("Join code copied: {code}", &[("code", &code)]));
            ui.close();
        }
        if menu_item(ui, tr("Join a file with a code or link…"), "") {
            app.ui.live.share.then = Some(Then::Join);
            app.ui.live.share.open = false;
            ui.close();
        }
        ui.separator();
        if app.session.live.can_host() {
            if local {
                if menu_item(ui, tr("Stop sharing on the local network"), "") {
                    app.run("collab.stop", json!({}));
                    ui.close();
                }
            } else if !in_session && menu_item(ui, tr("Share on the local network"), "") {
                app.run("collab.share", json!({"via": "local"}));
                ui.close();
            }
        }
        if !app.session.prefs.publish_server.trim().is_empty() && menu_item(ui, tr("About this server"), "") {
            let server = app.session.prefs.publish_server.trim().to_string();
            app.ui.live.share.about = true;
            if !hosting && app.ui.live.server_info.as_ref().is_none_or(|(at, _)| *at != server) {
                super::ask_server(app, &ui.ctx().clone(), server);
            }
            ui.close();
        }
        if menu_item(ui, tr("Publish to another server…"), "") {
            app.ui.live.share.then = Some(Then::Publish);
            app.ui.live.share.open = false;
            ui.close();
        }
        if hosting && status == Status::Live && !owner(app) && menu_item(ui, tr("Report this file…"), "") {
            app.ui.live.report = Some(String::new());
            ui.close();
        }
        if in_session {
            ui.separator();
            if menu_item(ui, tr("Leave file"), "") {
                app.run("collab.leave", json!({}));
                ui.close();
            }
            if removable(app).is_some() && menu_item(ui, tr("Remove from server…"), "") {
                app.ui.live.confirm_remove = true;
                ui.close();
            }
        }
    });
}

/// What the file's server says about itself: who runs it, its terms, when it deletes files, and
/// that it can read them.
fn about(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    ui.add_space(4.0);
    if let Some(h) = app.session.live().hosting.clone() {
        super::host_line(ui, t, &h.operator, &h.terms);
        super::expiry_line(ui, t, h.expires);
        crate::widgets::label(ui, tr(super::PLAIN_NOTE), t.text_secondary);
        return;
    }
    let server = app.session.prefs.publish_server.trim().to_string();
    match app.ui.live.server_info.as_ref().filter(|(at, _)| *at == server).map(|(_, v)| v.clone()) {
        Some(info) => super::hosting_lines(ui, t, &info),
        None => crate::widgets::label(ui, tr("Asking the server…"), t.text_secondary),
    }
}

/// The file's server and id, when you can take it off the server (you published it without the
/// server's key, here or on this computer before).
fn removable(app: &FigApp) -> Option<(String, String)> {
    let live = app.session.live();
    let (server, file) = live.server_file()?;
    let has = live.published.as_ref().is_some_and(|p| p.owner.is_some())
        || app.session.prefs.online_files.iter().any(|f| f.file == file && figcraft_engine::online::same_server(&f.server, &server));
    has.then_some((server, file))
}

/// Remove from server…: are you sure?
fn confirm_remove(app: &mut FigApp, ui: &mut Ui, t: &Tokens) {
    let Some((server, file)) = removable(app) else {
        app.ui.live.confirm_remove = false;
        return;
    };
    crate::widgets::label(
        ui,
        tr("Remove this file from the server? It can't be shared or opened from there again. Your copy stays open here."),
        t.danger,
    );
    ui.horizontal(|ui| {
        if crate::widgets::button(ui, tr("Remove"), true, true).clicked() {
            let owner = app.session.live().published.as_ref().and_then(|p| p.owner.clone());
            let mut params = json!({"server": server, "file": file, "action": "remove"});
            if let Some(owner) = owner {
                params["owner"] = json!(owner);
            }
            app.ui.live.confirm_remove = false;
            app.run_background(&ui.ctx().clone(), "collab.manageFile", params, Box::new(|app, _, _| app.toast(tr("Removed from the server"))));
        } else if crate::widgets::button(ui, tr("Cancel"), false, true).clicked() {
            app.ui.live.confirm_remove = false;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::typed_emails;

    #[test]
    fn emails_are_split_on_commas_spaces_and_semicolons() {
        assert_eq!(typed_emails(" a@x.com, b@x.com;c@x.com  d@x.com ,,"), ["a@x.com", "b@x.com", "c@x.com", "d@x.com"]);
        assert!(typed_emails(" , ; ").is_empty());
    }
}
