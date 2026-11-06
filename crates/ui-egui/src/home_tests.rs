//! The home screen, drawn headless: the Templates grid (opened by click and by keyboard, searched,
//! filtered, its empty state, its names for screen readers, thumbnails cached per theme and no
//! frames once they are in), the first-run row (only without recent files) and the Getting
//! started card (closed, kept closed, and brought back from the Help menu).

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers, PointerButton, Pos2, Shape};
use figcraft_engine::Session;
use figcraft_engine::online::OnlineFile;
use figcraft_engine::templates::TEMPLATES;
use serde_json::json;

use crate::home::{Section, cached_thumbnails};
use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, has_text, text_center, texts};

/// The app as a first run finds it: no documents, no recent files.
fn first_run() -> Ui {
    Ui::new(Session::empty())
}

fn shapes(u: &mut Ui) -> Vec<Shape> {
    flat(&u.frame_output(vec![]).shapes)
}

/// Enough frames for every thumbnail in view (they render one a frame) and what follows a click,
/// then until every recent file has been looked at: the reader is a thread of its own, and until it
/// answers a file's card is a skeleton without its name (on a loaded machine, 12 frames weren't
/// always enough, #351).
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

fn click(u: &mut Ui, p: Pos2) {
    for pressed in [true, false] {
        u.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    }
    u.frame(vec![]);
}

fn press(u: &mut Ui, key: Key) {
    for pressed in [true, false] {
        u.frame(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE }]);
    }
}

fn templates_tab(u: &mut Ui) -> Vec<Shape> {
    let s = shapes(u);
    click(u, text_center(&s, "Templates"));
    assert_eq!(u.app.ui.home_view.section, Section::Templates);
    settle(u)
}

#[test]
fn the_sidebar_has_the_sections_and_the_workspace_rows() {
    let mut u = first_run();
    let s = shapes(&mut u);
    assert!(has_text(&s, "Recents") && has_text(&s, "Online") && has_text(&s, "Templates"));
    for row in [
        "Drafts",
        "All folders",
        "Resources",
        "Trash",
        "See what's included",
        "Your plan and usage",
        "Starred",
        "Team folder",
        "Upgrade",
        "Your workspace",
        "Free",
    ] {
        assert!(has_text(&s, row), "{row}");
    }
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let nodes: Vec<_> = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect();
    let tab = |label: &str| nodes.iter().find(|n| n.role() == Role::Tab && n.label() == Some(label)).unwrap_or_else(|| panic!("no tab {label}"));
    assert_eq!((tab("Recents").is_selected(), tab("Online").is_selected(), tab("Templates").is_selected()), (Some(true), Some(false), Some(false)));
    // In reading order: Recents, Online, Templates.
    let top = |label: &str| tab(label).bounds().unwrap().y0;
    assert!(top("Recents") < top("Online") && top("Online") < top("Templates"));
    // Each is clicked into, and `ui.inspect` and `ui.set {homeSection}` name them the same way.
    for (label, section) in [("Online", Section::Online), ("Templates", Section::Templates), ("Recents", Section::Recents)] {
        let s = shapes(&mut u);
        click(&mut u, text_center(&s, label));
        assert_eq!(u.app.ui.home_view.section, section);
        let inspect = u.app.inspect();
        assert_eq!(inspect["homeSection"], section.id());
        assert_eq!(inspect["online"]["open"], section == Section::Online);
        assert_eq!(Section::parse(section.id()), Some(section));
    }
    assert_eq!(Section::parse("drafts"), None);
    // The Online section is the online files' own.
    u.app.ui.home_view.section = Section::Online;
    let s = settle(&mut u);
    assert!(has_text(&s, "Online files") && !has_text(&s, "Getting started") && !has_text(&s, "Start from a template"));
}

#[test]
fn the_templates_grid_shows_every_template_and_a_click_opens_a_new_document() {
    let mut u = first_run();
    let s = templates_tab(&mut u);
    for t in TEMPLATES {
        assert!(has_text(&s, t.name), "{}", t.name);
    }
    assert!(has_text(&s, "Apps · 390 × 844"), "the kind and size tag");
    assert_eq!(cached_thumbnails(&u.ctx, false).len(), TEMPLATES.len(), "every card has its picture");
    click(&mut u, text_center(&s, "Dashboard"));
    assert_eq!(u.app.session.docs.len(), 1);
    let ds = u.app.session.ds().unwrap();
    assert_eq!(ds.doc.name, "Dashboard");
    assert!(ds.path.is_none() && !ds.is_dirty(), "a new, untitled-on-disk document");
    assert!(!u.app.ui.home, "and the editor shows it");
    // Again: another document, the first untouched.
    let before = u.app.session.docs[0].doc.clone();
    u.app.ui.home = true;
    let s = settle(&mut u);
    // (Its tag: the name is also on the open document's tab now.)
    click(&mut u, text_center(&s, "Apps · 1440 × 960"));
    assert_eq!(u.app.session.docs.len(), 2);
    assert!(std::sync::Arc::ptr_eq(&u.app.session.docs[0].doc, &before));
}

#[test]
fn cards_are_reached_with_tab_named_for_screen_readers_and_opened_with_enter() {
    let mut u = first_run();
    templates_tab(&mut u);
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let nodes: Vec<_> = u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect();
    for t in TEMPLATES {
        let name = format!("{}: {}", t.name, t.description);
        assert!(nodes.iter().any(|n| n.role() == Role::Button && n.label() == Some(name.as_str())), "no button named {name:?}");
    }
    assert!(nodes.iter().any(|n| n.label() == Some("Search templates")));
    // Tab walks the controls; the first card it reaches opens with Enter.
    let mut reached = false;
    for _ in 0..40 {
        press(&mut u, Key::Tab);
        if crate::a11y::focused_name(&u.ctx).starts_with("Mobile app: ") {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab never reached the first card");
    press(&mut u, Key::Enter);
    u.frame(vec![]);
    assert_eq!(u.app.session.doc().unwrap().name, "Mobile app");
    assert!(!u.app.ui.home);
}

#[test]
fn search_and_kind_filter_the_grid_and_nothing_found_offers_a_way_back() {
    let mut u = first_run();
    templates_tab(&mut u);
    u.app.ui.home_view.query = "icon".into();
    let s = settle(&mut u);
    assert!(has_text(&s, "Icon set starter") && !has_text(&s, "Dashboard"));
    // Descriptions and kinds match too, whatever the case.
    u.app.ui.home_view.query = "  PROTOTYPE ".into();
    let s = settle(&mut u);
    assert!(has_text(&s, "Mobile app") && !has_text(&s, "Landing page"));
    u.app.ui.home_view.query.clear();
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Websites"));
    let s = settle(&mut u);
    assert!(has_text(&s, "Landing page") && !has_text(&s, "Mobile app") && !has_text(&s, "Icon set starter"));
    u.app.ui.home_view.query = "zzz".into();
    let s = settle(&mut u);
    assert!(has_text(&s, "No templates match your search"));
    assert!(TEMPLATES.iter().all(|t| !has_text(&s, t.name)));
    click(&mut u, text_center(&s, "Show all templates"));
    let s = settle(&mut u);
    assert!(u.app.ui.home_view.query.is_empty() && u.app.ui.home_view.kind.is_none());
    assert!(TEMPLATES.iter().all(|t| has_text(&s, t.name)));
}

#[test]
fn typing_in_the_search_field_filters() {
    let mut u = first_run();
    let s = templates_tab(&mut u);
    let field = text_center(&s, "Search templates");
    click(&mut u, field);
    u.frame(vec![Event::Text("slides".into())]);
    let s = settle(&mut u);
    assert_eq!(u.app.ui.home_view.query, "slides");
    assert!(has_text(&s, "Presentation slides") && !has_text(&s, "Dashboard"));
    assert!(u.app.session.docs.is_empty(), "typing opens nothing");
}

#[test]
fn home_search_finds_known_files_and_opens_an_existing_tab() {
    let mut session = Session::new();
    session.execute("file.rename", &json!({"name": "North Trail"})).unwrap();
    session.docs[0].path = Some("C:/designs/North Trail.figcraft".into());
    session.prefs.recent_files =
        vec!["C:/designs/North Trail.figcraft".into(), "C:/designs/South Trail.figcraft".into(), "C:/designs/Other.figcraft".into()];
    session.prefs.online_files.push(OnlineFile {
        server: "wss://share.example/".into(),
        file: "trail".into(),
        name: "Trail Live".into(),
        owner: "test-only-secret".into(),
        ..OnlineFile::default()
    });
    let mut u = Ui::new(session);
    u.app.ui.home = true;
    u.app.ui.home_view.file_query = "  TRAIL  ".into();
    let s = settle(&mut u);
    assert!(has_text(&s, "Search results"));
    assert!(has_text(&s, "North Trail") && has_text(&s, "South Trail") && has_text(&s, "Trail Live"));
    assert!(!has_text(&s, "Other") && !has_text(&s, "test-only-secret"));
    assert_eq!(texts(&s).iter().filter(|(name, pos, _)| name == "North Trail" && pos.y > 90.0).count(), 1, "an open file is not repeated in recents");
    assert_eq!(u.app.inspect()["homeSearch"], "  TRAIL  ");

    u.app.ui.home_view.file_query = "nothing matches".into();
    let s = shapes(&mut u);
    assert!(has_text(&s, "No files or templates match your search"));
    assert!(!has_text(&s, "Trail Live") && !has_text(&s, "South Trail"));
    assert!(!texts(&s).iter().any(|(name, pos, _)| name == "North Trail" && pos.y > 90.0));

    u.app.ui.home_view.file_query = "north".into();
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "North Trail"));
    assert!(!u.app.ui.home, "choosing an open result shows its editor tab");
    assert_eq!(u.app.session.docs.len(), 1);
}

#[test]
fn home_search_shows_a_saved_files_preview() {
    let dir = std::env::temp_dir().join(format!("figcraft-home-search-preview-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Search preview.figcraft");
    let image = image::RgbaImage::from_pixel(64, 32, image::Rgba([30, 140, 210, 255]));
    let mut png = Vec::new();
    image.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let doc = figcraft_doc::Document::new("Search preview");
    std::fs::write(&path, figcraft_format::save_bytes(&doc, Some(&png)).unwrap()).unwrap();

    let mut session = Session::empty();
    session.prefs.recent_files.push(path.to_string_lossy().into_owned());
    let mut u = Ui::new(session);
    u.app.ui.home_view.file_query = "Search preview".into();
    for _ in 0..400 {
        u.frame(vec![]);
        if u.app.inspect()["homeRecents"]["files"][0]["picture"] == "picture" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let s = shapes(&mut u);
    assert!(has_text(&s, "Search preview"));
    assert!(s.iter().any(|s| matches!(s, Shape::Mesh(m) if m.texture_id != egui::TextureId::default())), "the saved preview is drawn");
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_dir(dir);
}

#[test]
fn home_search_field_filters_as_typed_and_a_section_click_clears_it() {
    let mut u = first_run();
    u.app.session.prefs.recent_files = vec!["C:/designs/Glacier.figcraft".into(), "C:/designs/Forest.figcraft".into()];
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Search files and templates"));
    u.frame(vec![Event::Text("glacier".into())]);
    let s = settle(&mut u);
    assert_eq!(u.app.ui.home_view.file_query, "glacier");
    assert!(has_text(&s, "Glacier") && !has_text(&s, "Forest"));
    click(&mut u, text_center(&s, "Recents"));
    assert!(u.app.ui.home_view.file_query.is_empty());
    let s = shapes(&mut u);
    assert!(has_text(&s, "Forest"));
    // Escape in the field clears the search too.
    click(&mut u, text_center(&s, "Search files and templates"));
    u.frame(vec![Event::Text("forest".into())]);
    assert_eq!(u.app.ui.home_view.file_query, "forest");
    u.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
    assert!(u.app.ui.home_view.file_query.is_empty(), "Escape clears the search");
}

/// A search keeps what the recent files it doesn't match know: back from it, their pictures and
/// the selection are still there (nothing is read again).
#[test]
fn a_search_keeps_the_other_recent_files_pictures_and_the_selection() {
    let dir = std::env::temp_dir().join(format!("figcraft-home-search-cache-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([30, 140, 210, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let paths: Vec<String> = ["Alpine", "Basin"]
        .into_iter()
        .map(|name| {
            let path = dir.join(format!("{name}.figcraft"));
            std::fs::write(&path, figcraft_format::save_bytes(&figcraft_doc::Document::new(name), Some(&png)).unwrap()).unwrap();
            path.to_string_lossy().into_owned()
        })
        .collect();
    let mut session = Session::empty();
    session.prefs.recent_files = paths.clone();
    let mut u = Ui::new(session);
    let pictures = |u: &mut Ui| -> Vec<serde_json::Value> {
        u.app.inspect()["homeRecents"]["files"].as_array().unwrap().iter().map(|f| f["picture"].clone()).collect()
    };
    for _ in 0..400 {
        u.frame(vec![]);
        if pictures(&mut u).iter().all(|p| p == "picture") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(pictures(&mut u).iter().all(|p| p == "picture"), "both pictures are in");
    u.app.ui.home_view.recents.selected = Some(paths[1].clone());
    u.app.ui.home_view.file_query = "alpine".into();
    let s = shapes(&mut u);
    assert!(has_text(&s, "Alpine") && !has_text(&s, "Basin"));
    u.app.ui.home_view.file_query.clear();
    u.frame(vec![]);
    assert!(pictures(&mut u).iter().all(|p| p == "picture"), "no picture is read again");
    assert_eq!(u.app.ui.home_view.recents.selected.as_deref(), Some(paths[1].as_str()), "the selection stays");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn home_search_includes_templates_and_opens_a_matching_card() {
    let mut u = first_run();
    u.app.ui.home_view.file_query = "MOBILE".into();
    let s = settle(&mut u);
    assert!(has_text(&s, "Templates") && has_text(&s, "Mobile app"));
    assert!(!has_text(&s, "Dashboard"));
    click(&mut u, text_center(&s, "Mobile app"));
    assert_eq!(u.app.session.doc().unwrap().name, "Mobile app");
    assert!(!u.app.ui.home);
}

#[test]
fn empty_home_search_can_browse_all_templates() {
    let mut u = first_run();
    u.app.ui.home_view.file_query = "no such design".into();
    let s = shapes(&mut u);
    assert!(has_text(&s, "No files or templates match your search"));
    click(&mut u, text_center(&s, "Show all templates"));
    assert!(u.app.ui.home_view.file_query.is_empty());
    assert_eq!(u.app.ui.home_view.section, Section::Templates);
    assert!(has_text(&settle(&mut u), "Dashboard"));
}

#[test]
fn thumbnails_are_cached_per_theme() {
    let mut u = first_run();
    templates_tab(&mut u);
    assert_eq!(cached_thumbnails(&u.ctx, false).len(), TEMPLATES.len());
    assert!(cached_thumbnails(&u.ctx, true).is_empty());
    u.app.session.prefs.theme = "dark".into();
    settle(&mut u);
    assert_eq!(cached_thumbnails(&u.ctx, true).len(), TEMPLATES.len(), "the dark theme's pictures");
    assert_eq!(cached_thumbnails(&u.ctx, false).len(), TEMPLATES.len(), "the light ones are kept");
}

#[test]
fn the_home_screen_draws_no_frames_once_its_thumbnails_are_in() {
    let (mut h, _) = crate::live_tests::Harness::new();
    h.app.ui.home = true;
    for tab in [Section::Templates, Section::Online, Section::Recents] {
        h.app.ui.home_view.section = tab;
        h.frames(60);
        let (_, delay) = h.frame(vec![]);
        assert!(delay >= std::time::Duration::from_secs(1), "{tab:?} asks for a frame in {delay:?}: {:?}", h.ctx.repaint_causes());
    }
    assert_eq!(cached_thumbnails(&h.ctx, false).len(), TEMPLATES.len());
}

#[test]
fn a_first_run_offers_templates_on_recents_and_recent_files_replace_them() {
    let mut u = first_run();
    let s = settle(&mut u);
    assert!(has_text(&s, "Start from a template"));
    let offered = TEMPLATES.iter().filter(|t| has_text(&s, t.name)).count();
    assert_eq!(offered, 4);
    assert!(has_text(&s, "Recently viewed") && !has_text(&s, "Open files"), "the tabs are there before any file");
    assert!(!has_text(&s, "Show as grid") && !has_text(&s, "Last modified"), "but not the sort and view buttons");
    click(&mut u, text_center(&s, "See all templates"));
    assert_eq!(u.app.ui.home_view.section, Section::Templates);
    // From the row: a click opens it.
    u.app.ui.home_view.section = Section::Recents;
    let s = settle(&mut u);
    click(&mut u, text_center(&s, "Landing page"));
    assert_eq!(u.app.session.doc().unwrap().name, "Landing page");
    // With a recent file, the list shows in the row's place.
    u.app.ui.home = true;
    u.app.session.prefs.recent_files = vec!["/tmp/ridgeline.figcraft".into()];
    let s = settle(&mut u);
    assert!(has_text(&s, "Recently viewed") && has_text(&s, "ridgeline"));
    assert!(!has_text(&s, "Start from a template") && !has_text(&s, "See all templates"));
    assert!(!has_text(&s, "Open files"), "there is no list of open files apart from the recent ones");
    assert!(has_text(&s, "Landing page"), "the document opened from the template is listed with them");
}

#[test]
fn open_files_are_recent_cards_with_a_picture_of_the_file_as_it_is() {
    let short = "/tmp/wireframes.figcraft";
    let mut s = Session::new();
    s.execute("file.rename", &json!({"name": "Wireframes"})).unwrap();
    s.execute("shape.rectangle", &json!({"x": 0, "y": 0, "width": 320, "height": 200})).unwrap();
    s.docs[0].path = Some(short.into());
    s.execute("file.new", &json!({"name": "Blank"})).unwrap();
    let mut u = Ui::new(s);
    u.app.ui.home = true;
    u.app.session.prefs.getting_started_dismissed = true;
    let s = settle(&mut u);
    assert!(!has_text(&s, "Open files") && has_text(&s, "Recently viewed"));
    // A card shows the name (and "Open" for a file that is not saved anywhere yet).
    assert!(has_text(&s, "Wireframes"), "{:?}", texts(&s));
    assert!(has_text(&s, "Blank") && has_text(&s, "Open"));
    // The file with a frame in it has its picture, drawn from its content now; the empty one
    // keeps the placeholder.
    assert_eq!(crate::home::open_pictures(&u.ctx), 1);
    // Choosing a card shows that tab.
    let all = texts(&s);
    let at = all.iter().find(|(t, _, _)| t == "Blank").map(|(_, p, size)| *p + *size / 2.0).expect("the Blank card");
    // (A click selects a card, a double-click opens it, as with every recent file.)
    click(&mut u, at);
    click(&mut u, at);
    assert!(!u.app.ui.home);
    assert_eq!(u.app.session.doc().unwrap().name, "Blank");
}

/// How many files the Recently viewed list has (`ui.inspect` doesn't list the unsaved ones).
fn app_cards(u: &Ui) -> usize {
    crate::home::open_list(&u.app).len()
}

#[test]
fn tabs_that_show_one_file_are_one_card() {
    let mut s = Session::new();
    s.docs[0].path = Some("/tmp/one.figcraft".into());
    s.execute("file.new", &json!({})).unwrap();
    s.docs[1].path = Some("/tmp/one.figcraft".into());
    s.execute("file.new", &json!({})).unwrap();
    s.execute("file.new", &json!({})).unwrap();
    let mut u = Ui::new(s);
    u.app.ui.home = true;
    u.app.session.prefs.getting_started_dismissed = true;
    let s = settle(&mut u);
    let named = |n: &str| texts(&s).iter().filter(|(t, _, _)| t == n).count();
    // Four tabs: two on one path, two nowhere. The tab bar draws "Untitled" once per tab, so count
    // the cards by their detail line: the unsaved ones say "Open".
    assert_eq!(named("Open"), 2, "two unsaved files, two cards");
    assert_eq!(app_cards(&u), 3, "the two tabs on one path are one card");
}

#[test]
fn the_home_screen_has_no_tips_card_or_join_row_and_help_still_leads_back_to_it() {
    let mut u = first_run();
    let s = settle(&mut u);
    for gone in ["Getting started", "Draw a frame", "Join with a code", "Code or invite link"] {
        assert!(!has_text(&s, gone), "{gone}");
    }
    assert!(has_text(&s, "Start from a template"), "the rest of the screen stays");
    // Help ▸ Getting started: back on the home screen, from wherever.
    u.app.run("file.new", json!({}));
    u.app.ui.home = false;
    u.app.ui.home_view.section = Section::Templates;
    crate::menus::invoke(&mut u.app, "help.gettingStarted");
    assert!(u.app.ui.home && u.app.ui.home_view.section == Section::Recents);
}

#[test]
fn the_file_menu_item_shows_the_templates() {
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    crate::menus::invoke(&mut u.app, "file.newFromTemplate");
    assert!(u.app.ui.home && u.app.ui.home_view.section == Section::Templates);
    assert_eq!(u.app.session.docs.len(), 1, "nothing opens until one is chosen");
    assert!(has_text(&settle(&mut u), "Wireframe kit"));
}

#[test]
fn tab_walks_the_sidebar_in_order_before_a_sections_controls() {
    let mut u = first_run();
    let mut names = Vec::new();
    for _ in 0..40 {
        press(&mut u, Key::Tab);
        let name = crate::a11y::focused_name(&u.ctx);
        if !name.is_empty() && names.last() != Some(&name) {
            names.push(name);
        }
    }
    let at = |name: &str| names.iter().position(|n| n == name).unwrap_or_else(|| panic!("Tab never reached {name:?}: {names:?}"));
    assert!(at("Recents") < at("Online") && at("Online") < at("Templates") && at("Templates") < at("Open file"), "{names:?}");
}

/// A temporary directory of its own for a test's files, removed with them when dropped.
struct Temp(std::path::PathBuf);

impl Temp {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("figcraft-home-drop-{}-{stamp}-{n}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if let Ok(files) = std::fs::read_dir(&self.0) {
            for file in files.flatten() {
                let _ = std::fs::remove_file(file.path());
            }
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// A file dropped on the window, as the windowing backend hands it over (read from its path).
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Drag `path` over the window (one frame: what it shows), then drop it (one frame).
fn drop_file(u: &mut Ui, path: &std::path::Path) -> Vec<Shape> {
    let hovered = egui::HoveredFile { path: Some(path.to_path_buf()), mime: String::new() };
    let hovering = flat(&u.frame_raw(egui::RawInput { hovered_files: vec![hovered], ..Default::default() }).shapes);
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path.to_path_buf()));
    u.frame_raw(egui::RawInput { dropped_files: vec![file], ..Default::default() });
    hovering
}

/// A file dropped with its contents but no file on disk (a virtual file from another app).
#[derive(Debug)]
struct DroppedBytes(std::path::PathBuf, Vec<u8>);

impl egui::DroppedFile for DroppedBytes {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.1.clone())
    }
}

#[test]
fn dropping_a_file_without_one_on_disk_opens_it_as_a_new_unsaved_document() {
    let dir = Temp::new();
    let path = dir.path("virtual.svg");
    assert!(!path.exists());
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#.to_vec();
    let mut u = first_run();
    u.app.ui.home = true;
    let file: egui::DroppedFileHandle = std::sync::Arc::new(DroppedBytes(path.clone(), svg));
    u.frame_raw(egui::RawInput { dropped_files: vec![file], ..Default::default() });
    assert_eq!(u.app.ui.pending_opens.len(), 1);
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.ui.last_error, None);
    assert_eq!(u.app.session.docs.len(), 1, "the dropped contents open");
    assert_eq!(u.app.session.docs[0].path, None, "with no save path made up for them");
    assert!(u.app.session.docs[0].doc.nodes().any(|n| matches!(n.kind, figcraft_doc::NodeKind::Rectangle)), "the SVG's content");
    assert!(!u.app.ui.home, "and the editor shows it");
    assert!(!path.exists(), "nothing is written");
}

/// What a drop's read gives, once it has arrived.
type Arrival = std::rc::Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>>;

/// A drop whose contents arrive later, as every drop in the browser does: pending until `arrived`
/// holds them.
fn later(arrived: &Arrival) -> crate::ReadFuture {
    let arrived = arrived.clone();
    Box::pin(std::future::poll_fn(move |_| match arrived.borrow_mut().take() {
        Some(bytes) => std::task::Poll::Ready(bytes),
        None => std::task::Poll::Pending,
    }))
}

#[test]
fn a_dropped_file_whose_contents_arrive_later_opens_once_they_do() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#.to_vec();
    let arrived = std::rc::Rc::new(std::cell::RefCell::new(None));
    let mut u = first_run();
    u.app.ui.home = true;
    u.app.ui.pending_opens.push(crate::pending_open::PendingOpen {
        label: "Browser drop.svg".into(),
        params: json!({"name": "Browser drop.svg"}),
        command: "file.open",
        target: None,
        read: Some(later(&arrived)),
    });
    // egui lays a new card out invisibly on its first frame.
    u.frame(vec![]);
    for _ in 0..5 {
        let s = flat(&u.frame_output(vec![]).shapes);
        assert!(has_text(&s, "Browser drop.svg"), "the card shows while the contents are read");
    }
    assert!(u.app.session.docs.is_empty(), "nothing opens before the contents arrive");
    *arrived.borrow_mut() = Some(Ok(svg));
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.ui.last_error, None);
    assert_eq!(u.app.session.docs.len(), 1, "the contents open");
    assert_eq!(u.app.session.docs[0].path, None);
    assert!(u.app.session.docs[0].doc.nodes().any(|n| matches!(n.kind, figcraft_doc::NodeKind::Rectangle)), "the SVG's content");
    assert!(!u.app.ui.home);
}

#[test]
fn a_dropped_file_that_cannot_be_read_says_so_and_opens_nothing() {
    let arrived = std::rc::Rc::new(std::cell::RefCell::new(Some(Err(crate::unreadable("Unreadable.figcraft")))));
    let mut u = first_run();
    u.app.ui.home = true;
    u.app.ui.pending_opens.push(crate::pending_open::PendingOpen {
        label: "Unreadable.figcraft".into(),
        params: json!({"name": "Unreadable.figcraft"}),
        command: "file.open",
        target: None,
        read: Some(later(&arrived)),
    });
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert!(u.app.session.docs.is_empty());
    assert_eq!(u.app.ui.last_error.as_deref(), Some("Couldn't read Unreadable.figcraft."));
}

#[test]
fn dropping_a_native_file_on_the_home_screen_opens_it() {
    let dir = Temp::new();
    let path = dir.path("Dropped.figcraft");
    let mut saved = Session::new();
    saved.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    assert!(path.exists());
    let mut u = first_run();
    u.app.ui.home = true;
    let s = drop_file(&mut u, &path);
    assert!(has_text(&s, "Drop to open"), "dragging over the home screen says what a drop does");
    // The file waits behind its card, which is on screen before it opens.
    assert_eq!(u.app.ui.pending_opens.len(), 1);
    assert!(u.app.session.docs.is_empty(), "nothing opens in the frame of the drop");
    let s = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&s, "Opening as a new file") && has_text(&s, "Opening the file…") && has_text(&s, "Dropped.figcraft"), "the card shows");
    assert_eq!(u.app.ui.pending_opens.len(), 1);
    assert!(u.app.session.docs.is_empty(), "the card is drawn first");
    assert_eq!(u.app.inspect()["pendingOpens"], 1);
    u.frame(vec![]);
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.session.docs.len(), 1);
    assert_eq!(u.app.session.docs[0].path.as_deref(), Some(path.to_string_lossy().as_ref()));
    assert!(!u.app.ui.home, "and the editor shows it");
    assert!(u.app.ui.toast.as_ref().is_none_or(|t| !t.0.starts_with("Drop a")), "no turning away");
}

#[test]
fn dropping_a_native_file_on_canvas_keeps_its_receiving_document_after_tab_switch() {
    let dir = Temp::new();
    let path = dir.path("Canvas import.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#).unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    let target_page = session.page().unwrap();
    let target_name = session.doc().unwrap().name.clone();
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path.clone()));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    let pending = u.app.ui.pending_opens.first().unwrap();
    assert_eq!(pending.target.as_ref().map(|target| (target.uid, target.page)), Some((target_uid, target_page)));
    assert_eq!(pending.target.as_ref().map(|target| target.name.as_str()), Some(target_name.as_str()));

    u.frame(vec![]);
    u.app.session.active = 1;
    assert_eq!(u.app.session.ds().unwrap().uid, other_uid, "the UI switched tabs while the card was displayed");
    assert_eq!(u.app.ui.pending_opens.len(), 1, "the switch happens before dispatch");
    u.frame(vec![]);

    assert_eq!(u.app.session.ds().unwrap().uid, other_uid, "the user's active tab stays in front");
    let layers = |uid: u64| {
        let doc = &u.app.session.docs.iter().find(|doc| doc.uid == uid).unwrap().doc;
        doc.node(doc.pages[0]).unwrap().children.len()
    };
    assert_eq!((layers(target_uid), layers(other_uid)), (1, 0), "the drop lands in the tab that received it");
}

#[test]
fn dropping_a_document_on_canvas_opens_it_as_a_new_file() {
    let dir = Temp::new();
    let path = dir.path("Dropped.figcraft");
    let mut saved = Session::new();
    saved.execute("file.saveAs", &json!({"path": path.to_string_lossy()})).unwrap();
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    u.frame(vec![]);
    let before = u.app.session.doc().unwrap().clone();
    let s = drop_file(&mut u, &path);
    assert!(has_text(&s, "Drop to open"), "dragging a document over the canvas says it opens");
    let s = flat(&u.frame_output(vec![]).shapes);
    assert!(has_text(&s, "Opening as a new file"), "the card says so too");
    u.frame(vec![]);
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.session.docs.len(), 2, "a tab of its own");
    assert_eq!(u.app.session.ds().unwrap().path.as_deref(), Some(path.to_string_lossy().as_ref()), "in front");
    assert_eq!(u.app.session.docs[0].doc.as_ref(), &before, "the file that was open gets no pages added");
}

#[test]
fn a_document_dropped_on_canvas_without_a_file_on_disk_opens_as_a_new_file() {
    // Every drop in the browser (#916).
    let dir = Temp::new();
    let saved = dir.path("Saved.figcraft");
    Session::new().execute("file.saveAs", &json!({"path": saved.to_string_lossy()})).unwrap();
    let bytes = std::fs::read(&saved).unwrap();
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(DroppedBytes("Shared.figcraft".into(), bytes));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    for _ in 0..3 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    assert_eq!(u.app.ui.last_error, None);
    assert_eq!(u.app.session.docs.len(), 2, "a tab of its own");
    assert_eq!(u.app.session.docs[0].doc.pages.len(), 1, "no pages added to the open file");
    assert_eq!(u.app.session.docs[1].path, None, "with no save path made up for it");
}

#[test]
fn dropping_several_files_on_canvas_places_them_side_by_side() {
    let dir = Temp::new();
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#f00"/></svg>"##;
    let paths: Vec<_> = ["a.svg", "b.svg", "c.svg"].iter().map(|n| dir.path(n)).collect();
    for path in &paths {
        std::fs::write(path, svg).unwrap();
    }
    let mut u = Ui::new(Session::new());
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let files: Vec<egui::DroppedFileHandle> = paths.iter().map(|p| std::sync::Arc::new(Dropped(p.clone())) as egui::DroppedFileHandle).collect();
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: files, ..Default::default() });
    for _ in 0..4 {
        u.frame(vec![]);
    }
    assert!(u.app.ui.pending_opens.is_empty());
    let doc = u.app.session.doc().unwrap();
    let page = u.app.session.page().unwrap();
    let rects: Vec<_> = doc.node(page).unwrap().children.iter().map(|c| doc.world_bounds(*c).unwrap()).collect();
    assert_eq!(rects.len(), 3);
    for pair in rects.windows(2) {
        assert!(pair[1].x0 > pair[0].x1, "left to right with a gap: {rects:?}");
    }
}

#[test]
fn canvas_import_uses_queued_page_and_restores_the_page_selected_while_waiting() {
    let dir = Temp::new();
    let path = dir.path("Queued.svg");
    std::fs::write(&path, r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="#f00"/></svg>"##)
        .unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    let first_page = session.page().unwrap();
    session.execute("page.add", &json!({"name": "Second page"})).unwrap();
    let second_page = session.page().unwrap();
    let second_page_shape = session.execute("node.create", &json!({"type": "rectangle", "x": 30, "y": 40, "width": 12, "height": 14})).unwrap()["id"]
        .as_u64()
        .map(figcraft_doc::NodeId)
        .unwrap();
    session.execute("page.select", &json!({"id": first_page})).unwrap();
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    let queued_params = u.app.ui.pending_opens[0].params.clone();
    assert_eq!(u.app.ui.pending_opens[0].target.as_ref().map(|target| (target.uid, target.page)), Some((target_uid, first_page)));

    u.frame(vec![]);
    u.app.session.execute("page.select", &json!({"id": second_page})).unwrap();
    u.app.session.execute("select.set", &json!({"ids": [second_page_shape]})).unwrap();
    u.app.session.active = 1;
    u.frame(vec![]);
    let target_index = u.app.session.docs.iter().position(|doc| doc.uid == target_uid).unwrap();
    let imported = u.app.session.docs[target_index].doc.node(first_page).unwrap().children[0];
    let doc = &u.app.session.docs[target_index].doc;
    assert_eq!(doc.page_of(imported), Some(first_page));
    let bounds = doc.world_bounds(imported).unwrap();
    assert!((bounds.x0 - queued_params["x"].as_f64().unwrap()).abs() < 1.0);
    assert!((bounds.y0 - queued_params["y"].as_f64().unwrap()).abs() < 1.0);
    assert_eq!(doc.node(second_page).unwrap().children, [second_page_shape]);
    assert_eq!(u.app.session.ds().unwrap().uid, other_uid);
    assert_eq!(u.app.session.docs[target_index].page, second_page, "the page selected during the wait is restored");
    assert_eq!(u.app.session.docs[target_index].selection, [second_page_shape], "the restored page keeps its selection");

    u.app.session.active = target_index;
    u.app.session.execute("edit.delete", &json!({})).unwrap();
    let doc = &u.app.session.docs[target_index].doc;
    assert!(doc.node(second_page).unwrap().children.is_empty(), "the next edit acts on the restored page selection");
    assert_eq!(doc.node(first_page).unwrap().children.len(), 1, "the imported object on the queued page remains");
}

#[test]
fn queued_canvas_import_is_cancelled_if_its_page_is_deleted() {
    let dir = Temp::new();
    let path = dir.path("Removed target.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#).unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    session.execute("page.add", &json!({"name": "Keep this page"})).unwrap();
    let target_page = session.page().unwrap();
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    u.app.session.execute("page.delete", &json!({"id": target_page})).unwrap();
    let other_before = u.app.session.docs.iter().find(|doc| doc.uid == other_uid).unwrap().doc.clone();
    u.app.ui.pending_open_frames = 2;
    crate::pending_open::pending_opens(&mut u.app, &u.ctx);
    assert_eq!(u.app.session.docs.iter().find(|doc| doc.uid == other_uid).unwrap().doc, other_before);
    let target = u.app.session.docs.iter().find(|doc| doc.uid == target_uid).unwrap();
    assert_eq!(target.doc.pages.len(), 1);
    assert!(!target.doc.nodes().any(|node| node.name == "Removed target"));
    assert!(u.app.ui.last_error.as_deref().is_some_and(|error| error.contains("destination page was removed")));
}

#[test]
fn queued_canvas_import_is_cancelled_if_its_document_is_closed() {
    let dir = Temp::new();
    let path = dir.path("Closed target.svg");
    std::fs::write(&path, r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#).unwrap();

    let mut session = Session::new();
    let target_uid = session.ds().unwrap().uid;
    session.execute("file.new", &json!({"name": "Other tab"})).unwrap();
    let other_uid = session.ds().unwrap().uid;
    session.active = 0;
    let mut u = Ui::new(session);
    u.app.ui.home = false;
    u.frame(vec![]);
    let pos = u.app.canvas.rect.center();
    let file: egui::DroppedFileHandle = std::sync::Arc::new(Dropped(path));
    u.frame_raw(egui::RawInput { events: vec![Event::PointerMoved(pos)], dropped_files: vec![file], ..Default::default() });
    u.frame(vec![]);
    let other_before = u.app.session.docs.iter().find(|doc| doc.uid == other_uid).unwrap().doc.clone();
    let target_index = u.app.session.docs.iter().position(|doc| doc.uid == target_uid).unwrap();
    u.app.session.docs.remove(target_index);
    u.app.session.active = 0;

    while !u.app.ui.pending_opens.is_empty() {
        u.frame(vec![]);
    }
    assert_eq!(u.app.session.docs.len(), 1);
    assert_eq!(u.app.session.ds().unwrap().uid, other_uid);
    assert_eq!(u.app.session.doc().unwrap(), other_before.as_ref());
    assert!(u.app.ui.last_error.as_deref().is_some_and(|error| error.contains("destination document was closed")));
}

#[test]
fn dropping_an_unsupported_file_on_the_home_screen_opens_nothing() {
    let dir = Temp::new();
    let path = dir.path("unsupported.txt");
    std::fs::write(&path, "not a design").unwrap();
    let mut u = first_run();
    u.app.ui.home = true;
    drop_file(&mut u, &path);
    assert!(u.app.session.docs.is_empty());
    assert!(u.app.ui.home);
    assert_eq!(u.app.ui.toast.as_ref().map(|t| t.0.as_str()), Some("Drop a .figcraft, .fig, .jam, .sketch, .json, .svg or image file."));
}

#[test]
fn sketch_files_open_and_drop_as_documents() {
    let p = std::path::Path::new("/tmp/Design.SKETCH");
    assert!(crate::opens(p));
    assert!(crate::is_document(p));
}

#[test]
fn a_jam_file_is_a_document_open_and_drops_take() {
    // FigJam `.jam` files (#1342) open like `.fig` files, from File ▸ Open, a drop or the
    // command line, and a drop on the canvas opens one as a file of its own.
    for name in ["Board.jam", "BOARD.JAM"] {
        let path = std::path::Path::new(name);
        assert!(crate::open::opens(path) && crate::open::is_document(path), "{name}");
    }
}

/// A square PNG of one colour (what the page's server would hand over, small).
fn png(side: u32, rgba: [u8; 4]) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(side, side, image::Rgba(rgba));
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn picture_meshes(s: &[Shape]) -> usize {
    s.iter().filter(|s| matches!(s, Shape::Mesh(m) if m.texture_id != egui::TextureId::default() && m.vertices.len() > 40)).count()
}

#[test]
fn the_sidebar_shows_the_signed_in_account_with_its_picture_or_initials() {
    let mut u = first_run();
    u.app.ui.home = true;
    let s = shapes(&mut u);
    assert!(!has_text(&s, "Ada Lovelace"), "signed out: the app's name, not a person's");
    u.app.session.execute("account.set", &json!({"signedIn": true, "email": "ada@example.com", "name": "Ada Lovelace"})).unwrap();
    let s = shapes(&mut u);
    assert!(has_text(&s, "Ada Lovelace"), "their name tops the sidebar");
    assert!(has_text(&s, "AL"), "their initials until the picture is in");
    assert_eq!(picture_meshes(&s), 0);
    // The picture arrives: a circle of it replaces the initials.
    let ctx = u.ctx.clone();
    assert!(u.app.set_account_picture(&ctx, &png(200, [0xE0, 0x40, 0x40, 0xFF])));
    let s = shapes(&mut u);
    assert_eq!(picture_meshes(&s), 1, "the picture is drawn");
    assert!(!has_text(&s, "AL"));
    // Bytes that are not a picture leave the initials (and drop the old picture).
    for junk in [&b""[..], b"not an image", &[0xFF; 600_000]] {
        assert!(!u.app.set_account_picture(&ctx, junk));
    }
    let s = shapes(&mut u);
    assert_eq!(picture_meshes(&s), 0);
    assert!(has_text(&s, "AL"));
    // Signing out takes the person off the sidebar.
    assert!(u.app.set_account_picture(&ctx, &png(8, [0, 0, 0, 0xFF])));
    u.app.session.execute("account.set", &json!({"signedIn": false})).unwrap();
    let s = shapes(&mut u);
    assert!(!has_text(&s, "Ada Lovelace") && picture_meshes(&s) == 0);
}

#[test]
fn the_top_bar_names_the_section_and_starts_files() {
    let mut u = first_run();
    u.app.ui.home = true;
    let s = shapes(&mut u);
    for pill in ["New design file", "New board", "Open file"] {
        assert!(has_text(&s, pill), "{pill}");
    }
    click(&mut u, text_center(&s, "New board"));
    assert_eq!(u.app.session.docs.len(), 1, "a board was made");
    assert!(!u.app.ui.home, "and the editor shows it");
}

fn toast_text(u: &Ui) -> String {
    u.app.ui.toast.as_ref().map(|t| t.0.clone()).unwrap_or_default()
}

#[test]
fn the_sidebar_rows_go_where_they_can_and_say_when_they_cannot() {
    let mut u = first_run();
    let s = shapes(&mut u);
    // Resources are the templates; the workspace is your online files.
    click(&mut u, text_center(&s, "Resources"));
    assert_eq!(u.app.ui.home_view.section, Section::Templates);
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Your workspace"));
    assert_eq!(u.app.ui.home_view.section, Section::Online);
    // What FigCraft has no feature for says so instead of doing nothing.
    for (row, said) in [
        ("Trash", "There's no trash in FigCraft yet."),
        ("Drafts", "Drafts aren't in FigCraft yet: files live on this computer or on a server."),
        ("Team folder", "Starring isn't in FigCraft yet."),
        ("Upgrade", "Everything in FigCraft is included: it's free and open source."),
        ("See what's included", "Everything in FigCraft is included: it's free and open source."),
    ] {
        let s = shapes(&mut u);
        click(&mut u, text_center(&s, row));
        assert_eq!(toast_text(&u), said, "{row}");
        u.app.ui.toast = None;
    }
}

#[test]
fn a_signed_in_workspace_is_the_accounts_team_with_the_plan_it_chose() {
    let mut u = first_run();
    u.app
        .session
        .execute(
            "account.set",
            &json!({"signedIn": true, "email": "ada@example.com", "name": "Ada", "profile": {"plan": "professional", "onboarded": true}}),
        )
        .unwrap();
    let s = shapes(&mut u);
    assert!(has_text(&s, "Ada's team") && has_text(&s, "Pro") && !has_text(&s, "Free"));
}

#[test]
fn the_files_row_has_the_tabs_and_filters_and_each_does_what_it_can() {
    let mut u = first_run();
    let s = settle(&mut u);
    for text in ["Recently viewed", "Shared files", "Shared folders", "All organizations", "All files"] {
        assert!(has_text(&s, text), "{text}");
    }
    // Shared folders: no such thing yet, and it says so.
    click(&mut u, text_center(&s, "Shared folders"));
    assert_eq!(toast_text(&u), "Shared folders aren't in FigCraft yet.");
    // Shared files: the files shared online.
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "Shared files"));
    assert_eq!(u.app.ui.home_view.section, Section::Online);
}

#[test]
fn the_all_files_filter_keeps_design_files_or_boards() {
    let mut u = first_run();
    let s = shapes(&mut u);
    click(&mut u, text_center(&s, "All files"));
    let s = shapes(&mut u);
    let entry = |s: &[Shape], item: &str| texts(s).into_iter().map(|(t, _, _)| t).find(|t| t.ends_with(item)).unwrap_or_else(|| panic!("no {item}"));
    entry(&s, "Design files");
    let boards = entry(&s, "Boards");
    click(&mut u, text_center(&s, &boards));
    assert_eq!(u.app.ui.home_view.files_filter, Some(figcraft_doc::FileKind::Board));
    let s = shapes(&mut u);
    assert!(has_text(&s, "Boards"), "the button names the filter");
}

/// The browser build's account keeps its files on the page's server: Recently viewed lists them
/// (not the first-run templates) by name, never showing the code or link that opens them, one
/// open in a tab only once, and a file taken off the list goes.
#[test]
fn the_accounts_files_on_the_server_are_recent_cards() {
    let mut u = first_run();
    u.app.run(
        "account.recents",
        json!({"files": [
            {"target": "K7M-2QX-9TD", "name": "Landing page", "kind": "design", "opened": 1_800_000_000},
            {"target": "https://figcraft.example/#file=B0A", "name": "Ideas board", "kind": "board", "opened": 1_700_000_000},
        ]}),
    );
    let s = settle(&mut u);
    assert!(has_text(&s, "Landing page") && has_text(&s, "Ideas board"), "{:?}", texts(&s));
    assert!(!texts(&s).iter().any(|(t, ..)| t.contains("K7M-2QX-9TD") || t.contains("#file=")), "a file's code is never shown");
    assert!(!has_text(&s, "Start from a template"), "not a first run any more");
    // Open in a tab: the tab's card stands for it (the other "Landing page" up top is the title).
    u.app.run("file.new", json!({"name": "Landing page"}));
    let uid = u.app.session.ds().unwrap().uid;
    u.app.run("account.recentOpened", json!({"target": "K7M-2QX-9TD", "name": "Landing page", "tab": uid}));
    u.app.ui.home = true;
    let s = settle(&mut u);
    assert_eq!(texts(&s).iter().filter(|(t, at, _)| t == "Landing page" && at.y > 200.0).count(), 1, "{:?}", texts(&s));
    // Taken off the list.
    u.app.run("account.recentForget", json!({"target": "https://figcraft.example/#file=B0A"}));
    let s = settle(&mut u);
    assert!(!has_text(&s, "Ideas board"));
}

/// A document with one red frame, as a picture of it would show it.
fn red_frame_doc() -> figcraft_doc::Document {
    let mut s = Session::empty();
    s.execute("file.new", &json!({"name": "Red"})).unwrap();
    s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 400, "height": 300})).unwrap();
    s.execute("fill.set", &json!({"color": "#ff0000"})).unwrap();
    (*s.ds().unwrap().doc).clone()
}

/// The picture a file on the page's server keeps of itself: 16:9, the page's colour round it and
/// the frames in the middle; a file with nothing on its page is its colour alone.
#[test]
fn a_card_picture_is_the_page_and_its_frames() {
    let png = crate::home::card_picture_png(&red_frame_doc()).expect("a picture");
    let p = figcraft_format::thumbnail::decode_thumbnail(&png, 4096).unwrap();
    assert_eq!((p.width, p.height), (crate::home::CARD_PICTURE_PX, crate::home::CARD_PICTURE_PX * 9 / 16));
    let at = |x: u32, y: u32| {
        let i = ((y * p.width + x) * 4) as usize;
        [p.rgba[i], p.rgba[i + 1], p.rgba[i + 2], p.rgba[i + 3]]
    };
    let corner = at(0, 0);
    assert_eq!(corner[3], 255, "the page's colour fills the edges");
    assert_ne!(corner, [255, 0, 0, 255]);
    assert_eq!(at(p.width / 2, p.height / 2), [255, 0, 0, 255], "the frame is in the middle");
    let empty = crate::home::card_picture_png(&figcraft_doc::Document::new("Empty")).expect("a picture");
    let e = figcraft_format::thumbnail::decode_thumbnail(&empty, 4096).unwrap();
    assert!(e.rgba.chunks(4).all(|c| c == e.rgba.get(..4).unwrap()), "an empty page is its colour alone");
}

/// After a reload a file on the page's server has no tab to draw its picture from: the picture
/// kept for it is drawn on its card, which has a file glyph until then; bytes that aren't a picture
/// change nothing.
#[test]
fn a_recent_file_on_the_server_shows_the_picture_kept_of_it() {
    let mut u = first_run();
    u.app.run("account.recents", json!({"files": [{"target": "K7M-2QX-9TD", "name": "Landing page", "kind": "design", "opened": 1_800_000_000}]}));
    let s = settle(&mut u);
    assert!(has_text(&s, "Landing page"));
    let images = |s: &[Shape]| s.iter().filter(|s| matches!(s, Shape::Mesh(m) if m.texture_id != egui::TextureId::default())).count();
    let before = images(&s);
    let ctx = u.ctx.clone();
    assert!(!u.app.set_recent_picture(&ctx, "K7M-2QX-9TD", b"not a picture"));
    assert!(!u.app.ui.home_view.recents.has_cloud_picture("K7M-2QX-9TD"));
    let png = crate::home::card_picture_png(&red_frame_doc()).unwrap();
    assert!(u.app.set_recent_picture(&ctx, "K7M-2QX-9TD", &png));
    let s = settle(&mut u);
    assert_eq!(images(&s), before + 1, "the kept picture is drawn on the card");
}

/// Recently viewed is newest first by when each file was last opened or viewed: an open file goes
/// by when you last opened it (not "now" for every tab), so a file opened since comes before it.
#[test]
fn recent_files_are_newest_viewed_first() {
    let mut u = first_run();
    let now = figcraft_engine::cmd::comment::now_secs();
    u.app.run(
        "account.recents",
        json!({"files": [
            {"target": "OLD", "name": "Last week", "kind": "design", "opened": now - 7 * 86_400},
            {"target": "HOUR", "name": "An hour ago", "kind": "design", "opened": now - 3_600},
            {"target": "TEN", "name": "Ten minutes ago", "kind": "design", "opened": now - 600},
            {"target": "DAY", "name": "Yesterday", "kind": "design", "opened": now - 86_400},
        ]}),
    );
    // Open in a tab, last viewed two hours ago.
    u.app.run("file.new", json!({"name": "Open since this morning"}));
    let uid = u.app.session.ds().unwrap().uid;
    u.app.run("account.recentOpened", json!({"target": "TAB", "name": "Open since this morning", "tab": uid}));
    u.app.session.account.recents.iter_mut().filter(|r| r.target == "TAB").for_each(|r| r.opened = now - 7_200);
    u.app.ui.home = true;
    let s = settle(&mut u);
    let order = ["Ten minutes ago", "An hour ago", "Open since this morning", "Yesterday", "Last week"];
    let ys: Vec<f32> = order
        .iter()
        .map(|name| texts(&s).iter().filter(|(t, at, _)| t == name && at.y > 200.0).map(|(_, at, _)| at.y).next().unwrap_or(f32::NAN))
        .collect();
    let xs: Vec<f32> = order
        .iter()
        .map(|name| texts(&s).iter().filter(|(t, at, _)| t == name && at.y > 200.0).map(|(_, at, _)| at.x).next().unwrap_or(f32::NAN))
        .collect();
    let place: Vec<(i32, i32)> = ys.iter().zip(&xs).map(|(y, x)| (*y as i32, *x as i32)).collect();
    assert!(place.windows(2).all(|w| w[0] < w[1]), "newest first, row by row: {place:?}");
    // Switching to the open file puts it first.
    u.app.run("account.recentOpened", json!({"target": "TAB", "name": "Open since this morning", "tab": uid}));
    let s = settle(&mut u);
    let first = texts(&s)
        .iter()
        .filter(|(t, at, _)| order.contains(&t.as_str()) && at.y > 200.0)
        .min_by(|a, b| (a.1.y, a.1.x).partial_cmp(&(b.1.y, b.1.x)).unwrap())
        .map(|(t, ..)| t.clone());
    assert_eq!(first.as_deref(), Some("Open since this morning"));
}
