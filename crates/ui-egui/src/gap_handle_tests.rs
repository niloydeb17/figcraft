//! The on-canvas gap handles of an auto layout frame whose gap is automatic (the lines of a
//! wrapping layout spread across it, or the items spread along it): the handle shows the distance
//! the layout gives the gap, and dragging it makes the gap a fixed one from there, as typing a gap
//! in the panel does. A gap or padding drags only from the bar across its band. Driven through egui
//! headless.

use egui::{Event, vec2};
use figcraft_doc::{ContentAlign, MainAlign, NodeId};
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::layers_drag_tests::Ui;

fn id(v: Value) -> NodeId {
    figcraft_engine::params::id_of(&v["id"]).unwrap()
}

/// A 300 × 300 frame of three 120 × 60 rectangles with `props` on top of a horizontal layout
/// without padding (gap 10): selected, on screen.
fn scene(props: Value) -> (Ui, NodeId) {
    let mut s = Session::new();
    let mut all = json!({"layout": "horizontal", "padding": 0, "gap": 10, "crossGap": 10});
    all.as_object_mut().unwrap().extend(props.as_object().unwrap().clone());
    let f = id(s.execute("frame.create", &json!({"x": 100, "y": 100, "width": 300, "height": 300, "props": all})).unwrap());
    for _ in 0..3 {
        s.execute("shape.rectangle", &json!({"parent": f, "x": 0, "y": 0, "width": 120, "height": 60})).unwrap();
    }
    s.execute("select.set", &json!({"ids": [f]})).unwrap();
    (Ui::new(s), f)
}

/// Press at page point `from`, move by `by` page units in steps, release.
fn drag(u: &mut Ui, from: (f64, f64), by: (f32, f32)) {
    let p = u.app.to_screen(from.0, from.1);
    let zoom = u.app.session.ds().unwrap().view.zoom as f32;
    u.frame(vec![Event::PointerMoved(p)]);
    u.frame(vec![Event::PointerMoved(p)]);
    u.button(p, true);
    for k in 1..=4 {
        u.frame(vec![Event::PointerMoved(p + vec2(by.0, by.1) * zoom * (k as f32 / 4.0))]);
    }
    u.button(p + vec2(by.0, by.1) * zoom, false);
    u.frame(vec![]);
}

#[test]
fn dragging_the_gap_between_spread_rows_fixes_it_from_where_it_was() {
    // Two rows of 60 in 300: spread, they are 180 apart (the stored row gap of 10 doesn't count).
    let (mut u, f) = scene(json!({"wrap": true, "contentAlign": "spaceBetween"}));
    // Between the rows, at the frame's middle: page (250, 250). 20 down: 180 + 20.
    drag(&mut u, (250.0, 250.0), (0.0, 20.0));
    let l = u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap();
    assert_eq!((l.content_align, l.cross_gap), (ContentAlign::Auto, 200.0));
    // One undo step for the whole drag.
    u.app.run("edit.undo", json!({}));
    let l = u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap();
    assert_eq!((l.content_align, l.cross_gap), (ContentAlign::SpaceBetween, 10.0));
}

#[test]
fn dragging_an_auto_gap_between_items_fixes_it_too_and_fixed_gaps_drag_as_before() {
    // Three 60-wide items spread along 300: (300 - 3 × 60) / 2 = 60 apart.
    let (mut u, f) = scene(json!({"mainAlign": "spaceBetween"}));
    for k in u.app.session.doc().unwrap().node(f).unwrap().children.clone() {
        u.app.run("node.set", json!({"id": k, "props": {"width": 60}}));
    }
    u.frame(vec![]);
    // The bar between the first two items: x from 160 to 220 on the page, across the frame's
    // middle.
    drag(&mut u, (190.0, 250.0), (-10.0, 0.0));
    let l = u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap();
    assert_eq!((l.main_align, l.gap), (MainAlign::Min, 50.0));
    // A fixed gap: the drag changes the value and nothing else.
    drag(&mut u, (185.0, 250.0), (6.0, 0.0));
    let l = u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap();
    assert_eq!((l.main_align, l.gap), (MainAlign::Min, 56.0));
}

/// Where the frame's left edge is on the page.
fn frame_x(u: &Ui, f: NodeId) -> f64 {
    (u.app.session.doc().unwrap().world_transform(f) * figcraft_geom::Point::new(0.0, 0.0)).x
}

#[test]
fn only_the_bar_drags_a_gap_not_the_rest_of_its_band() {
    // Gap 10 between items at x 100..220 and 230..350 on the page: the band is 220..230, its bar
    // across the frame's middle (y 250).
    let (mut u, f) = scene(json!({}));
    let gap = |u: &Ui| u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap().gap;
    // In the band, well above the bar: the press goes to the frame, which moves; the gap stays.
    drag(&mut u, (225.0, 130.0), (8.0, 0.0));
    assert_eq!((gap(&u), frame_x(&u, f)), (10.0, 108.0));
    u.app.run("edit.undo", json!({}));
    // On the bar: it changes.
    drag(&mut u, (225.0, 250.0), (8.0, 0.0));
    assert_eq!(gap(&u), 18.0);
}

#[test]
fn a_gap_of_nothing_still_has_a_bar_to_drag() {
    let (mut u, f) = scene(json!({"gap": 0}));
    // The items meet at x 220 on the page.
    drag(&mut u, (220.0, 250.0), (12.0, 0.0));
    assert_eq!(u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap().gap, 12.0);
}

#[test]
fn only_the_bar_drags_a_padding() {
    // Padding 40 all round on the 300 × 300 frame at (100, 100): the left band is x 100..140, its
    // bar at its middle (x 120), halfway down (y 250).
    let (mut u, f) = scene(json!({"padding": 40}));
    let left = |u: &Ui| u.app.session.doc().unwrap().node(f).unwrap().layout.clone().unwrap().padding[3];
    drag(&mut u, (120.0, 150.0), (10.0, 0.0));
    assert_eq!((left(&u), frame_x(&u, f)), (40.0, 110.0));
    u.app.run("edit.undo", json!({}));
    drag(&mut u, (120.0, 250.0), (10.0, 0.0));
    assert_eq!(left(&u), 50.0);
}

#[test]
fn a_bar_shows_the_padding_arrows_hovered_and_dragged_not_a_hand() {
    use egui::CursorIcon;
    // Padding 40 round the frame at (100, 100); gap 10 between the items at x 220..230.
    let (mut u, _) = scene(json!({"padding": 40}));
    let cursor = |u: &mut Ui, events: Vec<Event>| {
        u.frame(events.clone());
        u.frame_output(events).platform_output.cursor_icon
    };
    let left = u.app.to_screen(120.0, 250.0);
    let top = u.app.to_screen(250.0, 120.0);
    assert_eq!(cursor(&mut u, vec![Event::PointerMoved(left)]), CursorIcon::ResizeColumn);
    assert_eq!(cursor(&mut u, vec![Event::PointerMoved(top)]), CursorIcon::ResizeRow);
    // Held down and moving: still the arrows.
    u.button(left, true);
    assert_eq!(cursor(&mut u, vec![Event::PointerMoved(left + vec2(5.0, 0.0))]), CursorIcon::ResizeColumn);
    u.button(left + vec2(5.0, 0.0), false);
    // The kit draws them as its padding cursor, turned for top and bottom.
    let (art, turn) = crate::cursors::kit_icon(CursorIcon::ResizeRow).unwrap();
    let (plain, _) = crate::cursors::kit_icon(CursorIcon::ResizeVertical).unwrap();
    assert!(!std::ptr::eq(art, plain) && turn == 90.0);
}
