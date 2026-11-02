//! Dev Mode in the kit's layout: the CSS groups of Layer properties, the token estimate, the
//! Inspect and Plugins tabs, the "Ready for development" button, and the dark chrome.

use egui::accesskit::{Node, Role};
use egui::{Pos2, pos2};
use figcraft_doc::{DevStatus, NodeId};
use figcraft_engine::Session;
use serde_json::json;

use crate::RightTab;
use crate::layers_drag_tests::Ui;
use crate::panels::right::dev_panel::{css_groups, estimate_tokens, format_tokens};
use crate::theme::Tokens;

fn tree(u: &mut Ui) -> Vec<Node> {
    u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect()
}

fn centre(n: &Node) -> Pos2 {
    let b = n.bounds().unwrap();
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

fn click(u: &mut Ui, at: Pos2) {
    u.button(at, true);
    u.button(at, false);
}

fn find(u: &mut Ui, role: Role, label: &str) -> Option<Node> {
    tree(u).into_iter().find(|n| n.role() == role && n.label() == Some(label))
}

/// A frame selected, in the given mode.
fn setup(tab: RightTab) -> (Ui, NodeId) {
    let mut s = Session::new();
    let f = crate::present_tests::id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200, "name": "Card"})).unwrap());
    s.execute("select.set", &json!({"ids": [f]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    u.app.ui.right_tab = tab;
    u.frame(vec![]);
    (u, f)
}

#[test]
fn css_is_split_into_layout_typography_and_style() {
    let code =
        "/* Frame */\nwidth: 360px;\nheight: 240px;\ndisplay: flex;\npadding: 16px;\nbackground: #FFFFFF;\nfont-size: 12px;\nborder-radius: 12px;\n";
    let groups = css_groups(code);
    let names: Vec<_> = groups.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, ["Layout", "Typography", "Style"]);
    assert_eq!(groups[0].1, ["width: 360px;", "height: 240px;", "display: flex;", "padding: 16px;"]);
    assert_eq!(groups[1].1, ["font-size: 12px;"]);
    assert_eq!(groups[2].1, ["background: #FFFFFF;", "border-radius: 12px;"]);
    // Empty groups and comments are left out.
    assert_eq!(css_groups("/* a */\nopacity: 0.5;\n").len(), 1);
    assert!(css_groups("").is_empty());
}

#[test]
fn the_token_estimate_is_a_quarter_of_the_characters() {
    assert_eq!(estimate_tokens(""), 0);
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abcde"), 2);
    assert_eq!(format_tokens(56), "56");
    assert_eq!(format_tokens(1234), "1.2k");
    assert_eq!(format_tokens(167_000), "167k");
}

#[test]
fn dev_mode_has_inspect_and_plugins_tabs_where_design_has_design_and_prototype() {
    let (mut u, _) = setup(RightTab::Design);
    assert!(find(&mut u, Role::Tab, "Design").is_some() && find(&mut u, Role::Tab, "Plugins").is_none());
    u.app.ui.right_tab = RightTab::Inspect;
    u.frame(vec![]);
    assert!(find(&mut u, Role::Tab, "Inspect").is_some_and(|n| n.is_selected() == Some(true)));
    let plugins = find(&mut u, Role::Tab, "Plugins").expect("a Plugins tab");
    click(&mut u, centre(&plugins));
    assert!(u.app.ui.dev.plugins_tab);
    assert!(find(&mut u, Role::Tab, "Plugins").is_some_and(|n| n.is_selected() == Some(true)));
    let inspect = find(&mut u, Role::Tab, "Inspect").unwrap();
    click(&mut u, centre(&inspect));
    assert!(!u.app.ui.dev.plugins_tab);
}

#[test]
fn the_ready_for_development_button_marks_the_frame_ready_then_completed() {
    let (mut u, f) = setup(RightTab::Inspect);
    let status = |u: &Ui| u.app.session.doc().unwrap().node(f).unwrap().dev_status;
    assert_eq!(status(&u), DevStatus::None);
    let button = find(&mut u, Role::Button, "Mark as ready for dev").expect("the button");
    click(&mut u, centre(&button));
    assert_eq!(status(&u), DevStatus::ReadyForDev);
    let button = find(&mut u, Role::Button, "Mark as completed").expect("it offers completing now");
    click(&mut u, centre(&button));
    assert_eq!(status(&u), DevStatus::Completed);
}

#[test]
fn dev_mode_chrome_is_dark_whatever_the_theme() {
    let (mut u, _) = setup(RightTab::Design);
    u.app.session.prefs.theme = "light".into();
    u.frame(vec![]);
    assert!(!Tokens::get(&u.ctx).dark);
    u.app.ui.right_tab = RightTab::Inspect;
    u.frame(vec![]);
    assert!(Tokens::get(&u.ctx).dark, "Dev Mode is dark");
    u.app.ui.right_tab = RightTab::Design;
    u.frame(vec![]);
    assert!(!Tokens::get(&u.ctx).dark, "and the design is light again");
}

fn find_label(u: &mut Ui, label: &str) -> Option<Node> {
    tree(u).into_iter().find(|n| n.label() == Some(label))
}

#[test]
fn the_header_button_toggles_ready_for_dev_on_a_frame_and_a_rectangle_has_none() {
    let (mut u, f) = setup(RightTab::Inspect);
    let status = |u: &Ui| u.app.session.doc().unwrap().node(f).unwrap().dev_status;
    let toggle = find_label(&mut u, "Toggle ready for dev status").expect("the toggle on a frame");
    click(&mut u, centre(&toggle));
    assert_eq!(status(&u), DevStatus::ReadyForDev);
    let toggle = find_label(&mut u, "Toggle ready for dev status").unwrap();
    click(&mut u, centre(&toggle));
    assert_eq!(status(&u), DevStatus::None);
    // A rectangle has no handoff status, so no button for it.
    let r = crate::present_tests::id(
        &u.app.session.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 50, "height": 50, "parent": f})).unwrap(),
    );
    u.app.session.execute("select.set", &json!({"ids": [r]})).unwrap();
    u.frame(vec![]);
    assert!(find_label(&mut u, "Toggle ready for dev status").is_none());
}

#[test]
fn the_more_menu_adds_a_dev_resource_link_and_marks_ready() {
    let (mut u, f) = setup(RightTab::Inspect);
    let more = find_label(&mut u, "More").expect("the menu button");
    click(&mut u, centre(&more));
    let item = find_label(&mut u, "Add a dev resource link").expect("the menu item");
    click(&mut u, centre(&item));
    assert_eq!(u.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("devres_adding", f)))), Some(true), "the field opens");
    let more = find_label(&mut u, "More").unwrap();
    click(&mut u, centre(&more));
    let item = find_label(&mut u, "Mark as ready for dev").expect("the menu offers it for a frame");
    click(&mut u, centre(&item));
    assert_eq!(u.app.session.doc().unwrap().node(f).unwrap().dev_status, DevStatus::ReadyForDev);
}

#[test]
fn dev_mode_panel_is_at_least_as_wide_as_the_kits() {
    let (mut u, _) = setup(RightTab::Design);
    u.frame(vec![]);
    u.frame(vec![]);
    let design = u.app.ui.right_panel_rect.expect("drawn").width();
    // (The panel's own rect leaves out its one point edge.)
    assert!((design - u.app.ui.right_width).abs() <= 1.5, "{design}");
    u.app.ui.right_tab = RightTab::Inspect;
    u.frame(vec![]);
    u.frame(vec![]);
    let dev = u.app.ui.right_panel_rect.expect("drawn").width();
    assert!((dev - crate::state::DEV_PANEL_W).abs() <= 1.5, "{dev}");
}

#[test]
fn the_page_view_has_code_settings_that_set_the_language_and_unit() {
    use figcraft_engine::codegen::CodeLang;
    let (mut u, _) = setup(RightTab::Inspect);
    u.app.session.execute("select.set", &json!({"ids": []})).unwrap();
    u.frame(vec![]);
    let lang = |u: &Ui| {
        u.ctx.data(|d| d.get_temp::<usize>(egui::Id::unique("inspect_lang"))).and_then(|i| CodeLang::ALL.get(i).copied()).unwrap_or_default()
    };
    assert_eq!(lang(&u), CodeLang::Css);
    let chip = find(&mut u, Role::ComboBox, "")
        .or_else(|| tree(&mut u).into_iter().find(|n| n.role() == Role::ComboBox && n.value() == Some("CSS")))
        .expect("the language chip");
    click(&mut u, centre(&chip));
    let react = find_label(&mut u, "React").expect("React in the language menu");
    click(&mut u, centre(&react));
    assert_eq!(lang(&u), CodeLang::React);
    // The unit is the other chip.
    let unit = tree(&mut u).into_iter().find(|n| n.role() == Role::ComboBox && n.value() == Some("px")).expect("the unit chip");
    click(&mut u, centre(&unit));
    let rem = find_label(&mut u, "rem").expect("rem in the unit menu");
    click(&mut u, centre(&rem));
    assert_eq!(u.ctx.data(|d| d.get_temp::<bool>(egui::Id::unique("inspect_rem"))), Some(true));
}
