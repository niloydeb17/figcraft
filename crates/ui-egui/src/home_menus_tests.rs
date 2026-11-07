//! Home's menus, drawn headless: the account menu (who you are, the theme, Settings, the desktop
//! app, the account in use, Add account and Log out) and a recent file's right-click menu (each
//! item does what FigCraft can, and says so where it can't), with the star on a hovered card.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use egui::{Event, Modifiers, PointerButton, Pos2, Shape};
use figcraft_engine::{Prefs, Session};
use serde_json::json;

use crate::Services;
use crate::home::home_menus::{DESKTOP_APP_URL, Then, joined};
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text, text_center};

/// What the services were asked to do.
#[derive(Default)]
struct Log {
    copied: Vec<String>,
    urls: Vec<String>,
    signed_out: usize,
}

struct Watch(Arc<Mutex<Log>>);
impl Services for Watch {
    fn save_prefs(&mut self, _prefs: &Prefs) {}
    fn clipboard_set_text(&mut self, s: &str) {
        self.0.lock().unwrap().copied.push(s.to_string());
    }
    fn open_url(&mut self, url: &str) {
        self.0.lock().unwrap().urls.push(url.to_string());
    }
    fn sign_out(&mut self) {
        self.0.lock().unwrap().signed_out += 1;
    }
    fn file_link(&self, target: &str) -> Option<String> {
        Some(format!("https://figcraft.test/#file={target}"))
    }
}

/// Home with one recent file on disk (`pic.figcraft`) and the services watched.
fn home(name: &str) -> (Ui, String, Arc<Mutex<Log>>) {
    let dir = std::env::temp_dir().join(format!("figcraft-home-menus-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path: PathBuf = dir.join("pic.figcraft");
    std::fs::write(&path, figcraft_format::save_bytes(&figcraft_doc::Document::new("pic"), None).unwrap()).unwrap();
    let path = path.to_string_lossy().into_owned();
    let mut s = Session::empty();
    s.prefs.recent_files = vec![path.clone()];
    s.prefs.getting_started_dismissed = true;
    let mut u = Ui::new(s);
    u.app.ui.home = true;
    let log = Arc::new(Mutex::new(Log::default()));
    u.app.services = Box::new(Watch(log.clone()));
    settle(&mut u);
    (u, path, log)
}

fn shapes(u: &mut Ui) -> Vec<Shape> {
    flat(&u.frame_output(vec![]).shapes)
}

/// Frames until every recent file has been looked at (the reader is a thread of its own).
fn settle(u: &mut Ui) -> Vec<Shape> {
    for _ in 0..12 {
        u.frame(vec![]);
    }
    for _ in 0..400 {
        let files = u.app.inspect()["homeRecents"]["files"].as_array().cloned().unwrap_or_default();
        if files.iter().all(|f| !f["exists"].is_null()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
        u.frame(vec![]);
    }
    shapes(u)
}

fn click_with(u: &mut Ui, p: Pos2, button: PointerButton) {
    for pressed in [true, false] {
        u.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE }]);
    }
}

fn click(u: &mut Ui, p: Pos2) {
    click_with(u, p, PointerButton::Primary);
    u.frame(vec![]);
}

fn hover(u: &mut Ui, p: Pos2) -> Vec<Shape> {
    u.frame(vec![Event::PointerMoved(p)]);
    shapes(u)
}

fn toast(u: &Ui) -> String {
    u.app.ui.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

/// The middle of the last text `s` drawn (a card's name: the window's title, drawn first, can
/// name an open file too).
fn last_center(shapes: &[Shape], s: &str) -> Pos2 {
    let (_, p, size) = crate::live_tests::texts(shapes).into_iter().rfind(|(t, _, _)| t == s).unwrap_or_else(|| panic!("no text {s:?}"));
    p + size / 2.0
}

/// Right-click `name`'s card and choose `item` from its menu.
fn choose(u: &mut Ui, name: &str, item: &str) {
    let s = settle(u);
    click_with(u, last_center(&s, name), PointerButton::Secondary);
    let s = shapes(u);
    click(u, text_center(&s, item));
}

fn reveal() -> &'static str {
    if cfg!(target_os = "macos") {
        "Show in Finder"
    } else if cfg!(windows) {
        "Show in Explorer"
    } else {
        "Show in folder"
    }
}

#[test]
fn a_recent_files_menu_has_the_kits_items() {
    let (mut u, ..) = home("items");
    let s = settle(&mut u);
    click_with(&mut u, text_center(&s, "pic"), PointerButton::Secondary);
    let s = shapes(&mut u);
    for item in [
        reveal(),
        "Open",
        "Open in new tab",
        "Add to your favorites",
        "Copy link",
        "Share",
        "Duplicate",
        "Show version history",
        "Rename",
        "Move file…",
        "Move to trash",
        "Remove from recent",
    ] {
        assert!(has_text(&s, item), "{item}");
    }
}

#[test]
fn what_figcraft_has_no_place_for_says_so() {
    let (mut u, ..) = home("unsupported");
    for (item, said) in [
        ("Add to your favorites", "Starring isn't in FigCraft yet."),
        ("Move file…", "Only files kept on your account's server go in folders: sign in and open the file there first."),
        ("Move to trash", "There's no trash in FigCraft yet."),
    ] {
        choose(&mut u, "pic", item);
        assert_eq!(toast(&u), said, "{item}");
        u.app.ui.toast = None;
    }
    assert!(u.app.ui.home, "nothing opened");
}

#[test]
fn copy_link_copies_a_files_path_or_its_address_on_the_server() {
    let (mut u, path, log) = home("copy");
    choose(&mut u, "pic", "Copy link");
    assert_eq!(log.lock().unwrap().copied, vec![path]);
    assert_eq!(toast(&u), "Link copied");
    u.app.run("account.recents", json!({"files": [{"target": "K7M-2QX-9TD", "name": "Landing page", "kind": "design", "opened": 1_800_000_000}]}));
    choose(&mut u, "Landing page", "Copy link");
    assert_eq!(log.lock().unwrap().copied.last().map(String::as_str), Some("https://figcraft.test/#file=K7M-2QX-9TD"));
    // Open in new tab opens the page again at the file's address; Home stays here.
    choose(&mut u, "Landing page", "Open in new tab");
    assert_eq!(log.lock().unwrap().urls, vec!["https://figcraft.test/#file=K7M-2QX-9TD".to_string()]);
    assert!(u.app.ui.home);
}

#[test]
fn rename_duplicate_share_and_versions_open_the_file_first() {
    let (mut u, path, _) = home("then");
    choose(&mut u, "pic", "Rename");
    assert!(!u.app.ui.home, "the file opened");
    assert_eq!(u.app.session.ds().unwrap().path.as_deref(), Some(path.as_str()));
    // The editor starts renaming the file in the left panel (and takes the flag).
    assert!(u.app.session.docs.len() == 1);
    // An open file is switched to: Duplicate makes the copy beside it.
    u.app.ui.home = true;
    choose(&mut u, "pic", "Duplicate");
    assert!(!u.app.ui.home);
    assert_eq!(u.app.session.docs.len(), 2);
    assert_eq!(u.app.session.ds().unwrap().doc.name, "pic (Copy)");
    // Share opens the file's Share dialog.
    u.app.ui.home = true;
    choose(&mut u, "pic", "Share");
    assert!(!u.app.ui.home);
    assert!(!u.app.ui.open_share, "the right panel took it");
    assert!(u.app.ui.live.share.open, "the Share dialog is open");
    // Show version history opens the file and its versions.
    u.app.ui.home = true;
    choose(&mut u, "pic", "Show version history");
    assert!(!u.app.ui.home);
    assert!(u.app.ui.toast.is_none(), "{:?}", u.app.ui.toast);
}

#[test]
fn what_was_asked_for_a_joined_file_happens_once_it_is_in() {
    let (mut u, ..) = home("joined");
    u.app.ui.home_view.then = Some((7, Then::Rename));
    joined(&mut u.app, 8);
    assert_eq!(u.app.ui.home_view.then, Some((7, Then::Rename)), "another join leaves it");
    assert!(!u.app.ui.rename_file);
    joined(&mut u.app, 7);
    assert_eq!(u.app.ui.home_view.then, None);
    assert!(u.app.ui.rename_file && !u.app.ui.home);
}

#[test]
fn a_hovered_card_shows_its_star() {
    let (mut u, ..) = home("star");
    u.ctx.enable_accesskit();
    let s = settle(&mut u);
    hover(&mut u, text_center(&s, "pic"));
    let out = u.frame_output(vec![]);
    let nodes: Vec<_> = out.platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect();
    let star = nodes.iter().find(|n| n.label() == Some("Add to your favorites")).expect("the star shows on the hovered card");
    let b = star.bounds().unwrap();
    click(&mut u, Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32));
    assert_eq!(toast(&u), "Starring isn't in FigCraft yet.");
    // Away from the cards, no star.
    let out = u.frame_output(vec![Event::PointerMoved(Pos2::new(1400.0, 880.0))]);
    let _ = out;
    let out = u.frame_output(vec![]);
    let nodes: Vec<_> = out.platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect();
    assert!(!nodes.iter().any(|n| n.label() == Some("Add to your favorites")));
}

/// Home signed in as Ada.
fn signed_in() -> (Ui, Arc<Mutex<Log>>) {
    let mut u = Ui::new(Session::empty());
    u.app.ui.home = true;
    let log = Arc::new(Mutex::new(Log::default()));
    u.app.services = Box::new(Watch(log.clone()));
    u.app
        .session
        .execute("account.set", &json!({"signedIn": true, "email": "ada@example.com", "name": "Ada", "profile": {"onboarded": true}}))
        .unwrap();
    (u, log)
}

/// Open the account menu from the name in the sidebar.
fn account_menu(u: &mut Ui) -> Vec<Shape> {
    let s = settle(u);
    click(u, text_center(&s, "Ada"));
    shapes(u)
}

#[test]
fn the_account_menu_shows_who_you_are_and_what_you_can_do() {
    let (mut u, log) = signed_in();
    let s = account_menu(&mut u);
    for text in
        ["ada@example.com", "Change theme", "Settings", "Get desktop app", "Create a community profile", "Switch account", "Add account", "Log out"]
    {
        assert!(has_text(&s, text), "{text}");
    }
    click(&mut u, text_center(&s, "Settings"));
    assert!(u.app.ui.preferences);
    u.app.ui.preferences = false;
    let s = account_menu(&mut u);
    click(&mut u, text_center(&s, "Get desktop app"));
    assert_eq!(log.lock().unwrap().urls, vec![DESKTOP_APP_URL.to_string()]);
    let s = account_menu(&mut u);
    click(&mut u, text_center(&s, "Create a community profile"));
    assert_eq!(toast(&u), "Community profiles aren't in FigCraft yet.");
    let s = account_menu(&mut u);
    click(&mut u, text_center(&s, "Log out"));
    let s = account_menu(&mut u);
    click(&mut u, text_center(&s, "Add account"));
    assert_eq!(log.lock().unwrap().signed_out, 2, "Log out and Add account sign this account out");
}

#[test]
fn change_theme_picks_light_dark_or_the_systems() {
    let (mut u, _) = signed_in();
    let s = account_menu(&mut u);
    let s2 = hover(&mut u, text_center(&s, "Change theme"));
    assert!(has_text(&s2, "Light") && has_text(&s2, "Dark") && has_text(&s2, "Follow system"));
    let dark = text_center(&s2, "Dark");
    // Over to the submenu, then the choice.
    hover(&mut u, dark);
    click(&mut u, dark);
    assert_eq!(u.app.session.prefs.theme, "dark");
}
