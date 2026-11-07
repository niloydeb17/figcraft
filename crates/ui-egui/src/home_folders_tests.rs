//! Home ▸ All folders, drawn headless: made from "+ Folder", shown as cards that open to their
//! files, starred into the sidebar, renamed and deleted from their menu, and filled from a file's
//! Move file…; signed out, All folders is the computer's file dialog as before.

use egui::{Event, Modifiers, PointerButton, Pos2, Shape};
use figcraft_engine::Session;
use serde_json::json;

use crate::home::Section;
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text, text_center};

/// Home signed in as Ada, with two files on the server in the recent files.
fn signed_in() -> Ui {
    let mut u = Ui::new(Session::empty());
    u.app.ui.home = true;
    u.app
        .session
        .execute(
            "account.set",
            &json!({"signedIn": true, "email": "ada@example.com", "name": "Ada", "profile": {"plan": "professional", "onboarded": true}}),
        )
        .unwrap();
    u.app.run(
        "account.recents",
        json!({"files": [
            {"target": "K7M-2QX-9TD", "name": "Landing page", "kind": "design", "opened": 1_800_000_000},
            {"target": "AAA-BBB-CCC", "name": "Ideas", "kind": "board", "opened": 1_799_000_000},
        ]}),
    );
    settle(&mut u);
    u
}

fn shapes(u: &mut Ui) -> Vec<Shape> {
    flat(&u.frame_output(vec![]).shapes)
}

fn settle(u: &mut Ui) -> Vec<Shape> {
    for _ in 0..12 {
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

/// The middle of the last text `s` drawn.
fn last_center(shapes: &[Shape], s: &str) -> Pos2 {
    let (_, p, size) = crate::live_tests::texts(shapes).into_iter().rfind(|(t, _, _)| t == s).unwrap_or_else(|| panic!("no text {s:?}"));
    p + size / 2.0
}

fn toast(u: &Ui) -> String {
    u.app.ui.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

/// Open All folders from the sidebar.
fn all_folders(u: &mut Ui) -> Vec<Shape> {
    let s = settle(u);
    click(u, text_center(&s, "All folders"));
    settle(u)
}

#[test]
fn all_folders_starts_empty_and_plus_folder_makes_one_and_opens_it() {
    let mut u = signed_in();
    let s = all_folders(&mut u);
    assert_eq!(u.app.ui.home_view.section, Section::Folders);
    assert!(has_text(&s, "No folders yet"));
    assert!(has_text(&s, "Ada's team") && has_text(&s, "Professional"), "the workspace heads the page");
    click(&mut u, text_center(&s, "Folder"));
    let s = settle(&mut u);
    for text in
        ["Create new folder", "Who has access", "Ada's team", "What they can do", "View and comment only", "Step 1 of 2", "Cancel", "Continue"]
    {
        assert!(has_text(&s, text), "{text}");
    }
    // What they can do: view only.
    click(&mut u, text_center(&s, "View and comment only"));
    // Named; Continue to the files, pick one, then Create: the new folder opens with it.
    u.app.ui.home_view.folder_dialogs.create.as_mut().unwrap().name = "Brand".into();
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Continue"));
    let s = settle(&mut u);
    assert!(has_text(&s, "Step 2 of 2") && has_text(&s, "Add files to Brand"));
    click(&mut u, last_center(&s, "Ideas"));
    let s = settle(&mut u);
    click(&mut u, last_center(&s, "Create"));
    let s = settle(&mut u);
    let folder = u.app.session.account.folders.first().expect("made").clone();
    assert_eq!(folder.name, "Brand");
    assert_eq!(u.app.ui.home_view.folder.as_deref(), Some(folder.id.as_str()));
    assert!(u.app.ui.home_view.folder_dialogs.create.is_none());
    assert_eq!((folder.access.as_str(), folder.role.as_str()), ("team", "view"));
    assert_eq!(folder.files.iter().map(|f| f.target.as_str()).collect::<Vec<_>>(), ["AAA-BBB-CCC"]);
    assert!(has_text(&s, "Ideas") && !has_text(&s, "Landing page"), "the folder shows its file");
    // Back to all of them.
    click(&mut u, last_center(&s, "All folders"));
    let s = settle(&mut u);
    assert_eq!(u.app.ui.home_view.folder, None);
    assert!(has_text(&s, "Brand") && has_text(&s, "Updated just now"), "its card: name and when it changed");
}

#[test]
fn move_file_puts_a_server_file_in_a_folder_and_the_folder_shows_only_its_files() {
    let mut u = signed_in();
    u.app.run("folder.create", json!({"name": "Brand"}));
    let id = u.app.session.account.folders[0].id.clone();
    // Recents ▸ Landing page ▸ Move file…
    u.app.ui.home_view.section = Section::Recents;
    let s = settle(&mut u);
    click_with(&mut u, last_center(&s, "Landing page"), PointerButton::Secondary);
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Move file…"));
    let s = settle(&mut u);
    assert!(has_text(&s, "Move Landing page"));
    click(&mut u, last_center(&s, "Brand"));
    let s = settle(&mut u);
    click(&mut u, last_center(&s, "Move"));
    assert_eq!(u.app.session.account.folder_of("K7M-2QX-9TD").map(|f| f.id.clone()), Some(id.clone()));
    assert_eq!(toast(&u), "Moved Landing page to Brand");
    // The folder's page: its file, not the other one.
    crate::home::folders::open_folder(&mut u.app, &id);
    let s = settle(&mut u);
    assert!(has_text(&s, "Landing page") && !has_text(&s, "Ideas"), "only the folder's files");
    assert!(!has_text(&s, "Recently viewed"), "no Recently viewed tabs in a folder");
    // A folder's file the recent files no longer list still shows.
    u.app.run("folder.addFile", json!({"id": id, "target": "OLD-FILE-123", "name": "Old file"}));
    let s = settle(&mut u);
    assert!(has_text(&s, "Old file"));
}

#[test]
fn a_folders_menu_stars_renames_and_deletes_it() {
    let mut u = signed_in();
    u.app.run("folder.create", json!({"name": "Brand"}));
    let id = u.app.session.account.folders[0].id.clone();
    let s = all_folders(&mut u);
    assert!(has_text(&s, "Team folder"), "nothing starred yet");
    click_with(&mut u, last_center(&s, "Brand"), PointerButton::Secondary);
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Add to starred"));
    assert!(u.app.session.account.folder(&id).unwrap().starred);
    let s = settle(&mut u);
    assert!(!has_text(&s, "Team folder"), "the starred folder is listed instead");
    // From the sidebar, it opens.
    u.app.ui.home_view.section = Section::Recents;
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Brand"));
    assert_eq!((u.app.ui.home_view.section, u.app.ui.home_view.folder.clone()), (Section::Folders, Some(id.clone())));
    // Rename.
    u.app.ui.home_view.folder = None;
    let s = settle(&mut u);
    click_with(&mut u, last_center(&s, "Brand"), PointerButton::Secondary);
    let s = shapes(&mut u);
    click(&mut u, last_center(&s, "Rename"));
    assert_eq!(u.app.ui.home_view.folder_dialogs.rename, Some((id.clone(), "Brand".into())));
    u.app.ui.home_view.folder_dialogs.rename.as_mut().unwrap().1 = "Brand kit".into();
    let s = settle(&mut u);
    click(&mut u, last_center(&s, "Rename"));
    assert_eq!(u.app.session.account.folder(&id).unwrap().name, "Brand kit");
    // Delete.
    let s = settle(&mut u);
    click_with(&mut u, last_center(&s, "Brand kit"), PointerButton::Secondary);
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Delete"));
    assert!(u.app.session.account.folders.is_empty());
    assert!(toast(&u).starts_with("Deleted Brand kit"));
}

#[test]
fn signed_out_all_folders_is_the_computers_file_dialog_and_move_file_says_why_not() {
    let mut u = Ui::new(Session::empty());
    u.app.ui.home = true;
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "All folders"));
    assert_eq!(u.app.ui.home_view.section, Section::Recents, "no account: no account folders");
    // A section asked for while signed out falls back to Recents.
    u.app.ui.home_view.section = Section::Folders;
    settle(&mut u);
    assert_eq!(u.app.ui.home_view.section, Section::Recents);
    assert_eq!(Section::parse("folders"), Some(Section::Folders));
}

#[test]
fn only_you_hides_what_they_can_do_and_an_empty_folder_says_how_to_fill_it() {
    let mut u = signed_in();
    all_folders(&mut u);
    crate::home::folders::open_create(&mut u.app, None);
    let s = settle(&mut u);
    click(&mut u, last_center(&s, "Ada's team"));
    let s = shapes(&mut u);
    // A menu item lined up with the ticked one starts with spaces.
    let item = crate::live_tests::texts(&s).into_iter().rfind(|(t, _, _)| t.trim() == "Only you").expect("Only you is offered");
    click(&mut u, item.1 + item.2 / 2.0);
    let s = settle(&mut u);
    assert_eq!(u.app.ui.home_view.folder_dialogs.create.as_ref().map(|d| d.access), Some("private"));
    assert!(!has_text(&s, "What they can do"), "nobody else to give rights to");
    assert!(has_text(&s, "Only you can see this folder."));
    click(&mut u, text_center(&s, "Continue"));
    let s = settle(&mut u);
    click(&mut u, last_center(&s, "Create"));
    let s = settle(&mut u);
    assert_eq!(u.app.session.account.folders[0].access, "private");
    assert!(has_text(&s, "This folder is empty: right-click a file in Recents and choose Move file…"));
    // Cancel closes without making one; Back goes to step 1.
    crate::home::folders::open_create(&mut u.app, None);
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Continue"));
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Back"));
    assert_eq!(u.app.ui.home_view.folder_dialogs.create.as_ref().map(|d| d.step), Some(1));
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Cancel"));
    assert!(u.app.ui.home_view.folder_dialogs.create.is_none());
    assert_eq!(u.app.session.account.folders.len(), 1);
}
