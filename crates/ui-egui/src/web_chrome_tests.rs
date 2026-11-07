//! The web page has no tab strip (one file to a page, as in the browser): the main menu leads back
//! to Home with Back to files, and its menus open where the strip would have ended.

use egui::accesskit::{Node, Role};
use egui::{Pos2, pos2};
use figcraft_engine::Session;

use crate::layers_drag_tests::Ui;

fn tree(u: &mut Ui) -> Vec<Node> {
    u.frame_output(vec![]).platform_output.accesskit_update.unwrap().nodes.into_iter().map(|(_, n)| n).collect()
}

fn centre(n: &Node) -> Pos2 {
    let b = n.bounds().unwrap();
    pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
}

fn setup(no_tab_strip: bool) -> Ui {
    let mut u = Ui::new(Session::new());
    u.ctx.enable_accesskit();
    u.app.ui.no_tab_strip = no_tab_strip;
    u.frame(vec![]);
    u
}

#[test]
fn without_a_tab_strip_no_tabs_are_drawn_and_the_content_starts_at_the_top() {
    let mut u = setup(true);
    assert!(tree(&mut u).iter().all(|n| !(n.role() == Role::Tab && n.label() == Some("Home"))), "a stripless page has no Home tab");
    let mut with = setup(false);
    assert!(tree(&mut with).iter().any(|n| n.role() == Role::Tab && n.label() == Some("Home")));
    assert_eq!(crate::tabs::bar_height(&u.app), 0.0);
    assert_eq!(crate::tabs::bar_height(&with.app), crate::tabs::HEIGHT);
}

#[test]
fn back_to_files_in_the_main_menu_goes_home_and_only_without_a_strip() {
    let mut u = setup(true);
    u.app.ui.main_menu = true;
    u.frame(vec![]);
    let item = tree(&mut u).into_iter().find(|n| n.label() == Some("Back to files")).expect("the menu offers Back to files");
    let at = centre(&item);
    u.button(at, true);
    u.button(at, false);
    assert!(u.app.ui.home);
    assert!(!u.app.ui.main_menu);
    let mut with = setup(false);
    with.app.ui.main_menu = true;
    with.frame(vec![]);
    assert!(tree(&mut with).iter().all(|n| n.label() != Some("Back to files")), "with tabs, Home is the tab");
}

#[test]
fn back_to_files_lights_up_under_the_pointer_like_the_other_rows() {
    let mut u = setup(true);
    u.app.ui.main_menu = true;
    u.frame(vec![]);
    let item = tree(&mut u).into_iter().find(|n| n.label() == Some("Back to files")).expect("the menu offers Back to files");
    let at = centre(&item);
    let hover = crate::theme::Tokens::get(&u.ctx).menu_hover;
    let lit = |u: &mut Ui, events: Vec<egui::Event>| {
        let shapes = crate::live_tests::flat(&u.frame_output(events).shapes);
        shapes.iter().any(|s| matches!(s, egui::Shape::Rect(r) if r.fill == hover && r.rect.contains(at)))
    };
    for _ in 0..30 {
        u.frame(vec![]);
    }
    assert!(!lit(&mut u, vec![]), "not lit before the pointer comes");
    lit(&mut u, vec![egui::Event::PointerMoved(at)]);
    // Once the menu has faded in.
    for _ in 0..30 {
        u.frame(vec![]);
    }
    assert!(lit(&mut u, vec![]), "lit under the pointer");
}
