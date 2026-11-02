//! The Auto layout section in the kit's layout: the Flow switch (free-form, vertical, horizontal,
//! grid), the heading's button, the sizing menus inside the W and H fields and the settings menu.

use egui::accesskit::Node;
use egui::{Pos2, pos2};
use figcraft_doc::{LayoutMode, NodeId};
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;

fn tree(u: &mut Ui) -> Vec<Node> {
    u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect()
}

fn centre(n: &Node) -> Pos2 {
    let b = n.bounds().unwrap();
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

fn click_named(u: &mut Ui, label: &str) {
    let at = tree(u).into_iter().find(|n| n.label() == Some(label)).map(|n| centre(&n)).unwrap_or_else(|| panic!("no control named {label}"));
    u.button(at, true);
    u.button(at, false);
}

/// A frame with auto layout (horizontal), selected.
fn setup() -> (Ui, NodeId) {
    let mut s = Session::new();
    let f = crate::present_tests::id(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 300, "height": 200, "name": "Card"})).unwrap());
    s.execute("layout.add", &json!({"ids": [f]})).unwrap();
    s.execute("select.set", &json!({"ids": [f]})).unwrap();
    let mut u = Ui::new(s);
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    (u, f)
}

fn mode(u: &Ui, f: NodeId) -> Option<LayoutMode> {
    u.app.session.doc().ok()?.node(f)?.layout.as_ref().map(|l| l.mode)
}

#[test]
fn the_flow_switch_picks_the_flow() {
    let (mut u, f) = setup();
    assert_eq!(mode(&u, f), Some(LayoutMode::Horizontal));
    click_named(&mut u, "Vertical");
    assert_eq!(mode(&u, f), Some(LayoutMode::Vertical));
    u.frame(vec![]);
    click_named(&mut u, "Grid");
    assert_eq!(mode(&u, f), Some(LayoutMode::Grid));
}

#[test]
fn free_form_and_the_heading_button_remove_auto_layout() {
    let (mut u, f) = setup();
    click_named(&mut u, "Free-form");
    assert_eq!(mode(&u, f), None);
    let (mut u, f) = setup();
    click_named(&mut u, "Remove auto layout");
    assert_eq!(mode(&u, f), None);
}

#[test]
fn wrap_is_a_toggle_beside_the_switch_and_grid_has_none() {
    let (mut u, f) = setup();
    click_named(&mut u, "Wrap");
    assert!(u.app.session.doc().unwrap().node(f).unwrap().layout.as_ref().unwrap().wrap);
    u.frame(vec![]);
    click_named(&mut u, "Grid");
    u.frame(vec![]);
    assert!(tree(&mut u).iter().all(|n| n.label() != Some("Wrap")));
}
