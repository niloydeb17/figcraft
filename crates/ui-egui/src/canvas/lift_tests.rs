//! Dragging a section of many layers trailed its outline: every step of the drag rendered the
//! page anew. The dragged layers are now lifted off the canvas (rendered once on their own, the
//! rest once without them) and slid with the pointer; a drag whose layers pass under others is
//! rendered as before.

use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::json;

use super::lift;
use crate::layers_drag_tests::Ui;
use crate::present_tests::id;

/// A section holding a few frames, selected, with a frame beside it below it in z-order.
fn page() -> (Session, NodeId) {
    let mut s = Session::new();
    s.execute("node.create", &json!({"type": "frame", "x": 500, "y": 0, "width": 150, "height": 150})).unwrap();
    let sec = id(&s.execute("node.create", &json!({"type": "section", "x": 0, "y": 0, "width": 400, "height": 300})).unwrap());
    for k in 0..3 {
        s.execute("node.create", &json!({"type": "frame", "x": 20 + k * 120, "y": 60, "width": 100, "height": 100, "parent": sec})).unwrap();
    }
    s.execute("select.set", &json!({"ids": [sec]})).unwrap();
    (s, sec)
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

/// Frames until `done` holds (the render threads take a moment), at most a few seconds.
fn until(u: &mut Ui, done: impl Fn(&Ui) -> bool) -> bool {
    let start = Instant::now();
    while !done(u) {
        if start.elapsed() > Duration::from_secs(10) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
        u.frame(vec![]);
    }
    true
}

#[test]
fn a_dragged_section_is_lifted_and_put_back() {
    let (s, sec) = page();
    let mut u = Ui::new(s);
    let from = u.app.to_screen(10.0, 280.0);
    u.frame(vec![Event::PointerMoved(from)]);
    u.frame(vec![button(from, true)]);
    for k in 1..=4 {
        u.frame(vec![Event::PointerMoved(from + egui::vec2(10.0 * k as f32, 6.0 * k as f32))]);
    }
    assert!(lift::active(&u.app), "a plain move of the selection is lifted");
    assert!(until(&mut u, |u| lift::has_image(&u.app) && u.app.canvas.tex_skipped), "the lifted image and the page without it arrive");
    // Further steps only slide the image: the canvas asks for nothing new.
    let asked = u.app.canvas.submitted.clone();
    for k in 5..=8 {
        u.frame(vec![Event::PointerMoved(from + egui::vec2(10.0 * k as f32, 6.0 * k as f32))]);
    }
    assert!(u.app.canvas.submitted == asked, "no new render of the page while it slides");
    let to = from + egui::vec2(80.0, 48.0);
    u.frame(vec![button(to, false)]);
    assert!(!lift::active(&u.app));
    assert!(lift::kept(&u.app), "the lifted image stays up until the canvas has the layers where they were dropped");
    assert!(until(&mut u, |u| !lift::kept(&u.app) && !u.app.canvas.tex_skipped), "then it goes");
    let b = u.app.session.doc().unwrap().world_bounds(sec).unwrap();
    let zoom = u.app.session.ds().unwrap().view.zoom;
    assert!((b.x0 - 80.0 / zoom).abs() < 1.0 && (b.y0 - 48.0 / zoom).abs() < 1.0, "moved: {b:?}");
}

#[test]
fn a_drag_under_a_layer_above_is_not_lifted() {
    let (mut s, sec) = page();
    // A rectangle above the section, where it is dragged to.
    s.execute("shape.rectangle", &json!({"x": 420, "y": 0, "width": 60, "height": 60})).unwrap();
    s.execute("select.set", &json!({"ids": [sec]})).unwrap();
    let mut u = Ui::new(s);
    let from = u.app.to_screen(10.0, 280.0);
    u.frame(vec![Event::PointerMoved(from)]);
    u.frame(vec![button(from, true)]);
    for k in 1..=4 {
        u.frame(vec![Event::PointerMoved(from + egui::vec2(20.0 * k as f32, 0.0))]);
    }
    assert!(!lift::active(&u.app), "the rectangle would end up under the section");
    u.frame(vec![button(from + egui::vec2(80.0, 0.0), false)]);
}
