//! The motion timeline drawn headless: a layer's animation is a solid bar over the span it
//! covers, and each animated field an outlined bar named for it with a tick at each keyframe; a
//! tick takes clicks (it picks the keyframe and moves the playhead there).

use egui::{Event, Modifiers, PointerButton, Shape};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::layers_drag_tests::Ui;
use crate::live_tests::{flat, texts};

fn idv(v: &Value) -> NodeId {
    serde_json::from_value(v["id"].clone()).or_else(|_| serde_json::from_value(v["ids"][0].clone())).unwrap()
}

/// A frame with a rectangle scaled from 0 s to 0.5 s, selected, and the timeline open.
fn scaled() -> (Ui, NodeId) {
    let mut s = Session::new();
    let frame = idv(&s.execute("frame.create", &json!({"x": 0, "y": 0, "width": 400, "height": 300, "name": "Screen"})).unwrap());
    let rect = idv(&s.execute("shape.rectangle", &json!({"x": 20, "y": 30, "width": 100, "height": 50, "parent": frame, "name": "Card"})).unwrap());
    for (time, value) in [(0.0, 1.0), (0.5, 1.5)] {
        s.execute("motion.keyframe.add", &json!({"ids": [rect], "field": "SCALE_X", "time": time, "value": value})).unwrap();
    }
    s.execute("select.set", &json!({"ids": [rect]})).unwrap();
    s.execute("view.motionTimeline", &json!({"show": true})).unwrap();
    let mut u = Ui::new(s);
    u.frame(vec![]);
    (u, rect)
}

/// The timeline panel's top in the 1440 × 900 test window.
const TIMELINE_TOP: f32 = 900.0 - crate::panels::motion_timeline::HEIGHT;

fn shapes(u: &mut Ui) -> Vec<Shape> {
    flat(&u.frame_output(vec![]).shapes)
}

/// The filled, rounded rectangles of `color` in the timeline panel (the bars).
fn bars(s: &[Shape], color: egui::Color32) -> Vec<egui::Rect> {
    s.iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if r.fill == color && r.rect.width() > 20.0 && r.corner_radius.nw > 0 && r.rect.top() > TIMELINE_TOP => Some(r.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn a_layer_is_a_solid_bar_and_a_field_an_outlined_bar_named_for_it() {
    let (mut u, _) = scaled();
    let s = shapes(&mut u);
    // The layer's bar, in the selected layer's blue, over the span its keyframes cover.
    let layer = bars(&s, egui::Color32::from_rgb(0x00, 0x7b, 0xe5));
    assert_eq!(layer.len(), 1, "{layer:?}");
    // The field's bar, as long as the layer's and just under it.
    let field = bars(&s, egui::Color32::from_rgb(0xf0, 0xf7, 0xfe));
    assert_eq!(field.len(), 1, "{field:?}");
    let (l, f) = (layer[0], field[0]);
    assert!((l.left() - f.left()).abs() < 1.0 && (l.right() - f.right()).abs() < 1.0, "{l:?} {f:?}");
    assert!(f.top() > l.bottom() && f.top() - l.bottom() < 8.0, "{l:?} {f:?}");
    assert!(l.width() > 200.0, "half a second of a 1440-wide window: {l:?}");
    // Its name sits in the middle of the bar (as well as in the names column).
    let named: Vec<_> = texts(&s).into_iter().filter(|(t, ..)| t == "Scale X").collect();
    assert!(named.iter().any(|(_, at, size)| (at.x + size.x / 2.0 - f.center().x).abs() < 2.0), "{named:?} {f:?}");
    // No diamonds: the keyframes are ticks inside the bars.
    let rows = egui::Rect::from_min_max(egui::pos2(l.left() - 10.0, l.top()), egui::pos2(l.right() + 10.0, f.bottom()));
    assert!(
        !s.iter().any(|s| matches!(s, Shape::Path(p) if p.points.len() == 4 && p.closed && p.points.iter().all(|q| rows.contains(*q)))),
        "no diamonds over the bars"
    );
}

#[test]
fn a_tick_picks_its_keyframe_and_moves_the_playhead_there() {
    let (mut u, _) = scaled();
    let s = shapes(&mut u);
    let f = bars(&s, egui::Color32::from_rgb(0xf0, 0xf7, 0xfe))[0];
    // The last keyframe's tick sits inside the bar's right end.
    let at = egui::pos2(f.right() - 7.0, f.center().y);
    for pressed in [true, false] {
        u.frame(vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    }
    u.frame(vec![]);
    let playhead = u.app.session.ds().unwrap().motion.playhead;
    assert!((playhead - 0.5).abs() < 1e-6, "playhead {playhead}");
}

/// The ruler counts in milliseconds under a second and in seconds after it ("1.2s", not a second
/// "200ms").
#[test]
fn the_ruler_counts_seconds_past_one() {
    let (mut u, _) = scaled();
    let s = shapes(&mut u);
    let labels: Vec<String> = texts(&s).into_iter().filter(|(_, at, _)| at.y > TIMELINE_TOP).map(|(t, ..)| t).collect();
    assert!(labels.iter().any(|t| t == "1.2s"), "{labels:?}");
    assert_eq!(labels.iter().filter(|t| t.as_str() == "200ms").count(), 1, "{labels:?}");
}

fn click(u: &mut Ui, at: egui::Pos2, modifiers: Modifiers) {
    u.frame_with(vec![Event::PointerMoved(at)], modifiers);
    for pressed in [true, false] {
        u.frame_with(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers }], modifiers);
    }
    u.frame(vec![]);
}

fn press(u: &mut Ui, key: egui::Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        u.frame_with(vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }], modifiers);
    }
    u.frame(vec![]);
}

fn fields(u: &Ui, id: NodeId) -> Vec<String> {
    u.app.session.doc().unwrap().node(id).unwrap().motion.as_ref().map(|m| m.fields().into_iter().map(|(f, _)| f.key()).collect()).unwrap_or_default()
}

/// The rectangle with Scale X and Opacity animated, and the bars of both fields.
fn two_fields() -> (Ui, NodeId, egui::Rect, egui::Rect) {
    let (mut u, rect) = scaled();
    for (time, value) in [(0.0, 0.0), (0.25, 1.0)] {
        u.app.run("motion.keyframe.add", json!({"ids": [rect], "field": "OPACITY", "time": time, "value": value}));
    }
    let s = shapes(&mut u);
    let mut field = bars(&s, egui::Color32::from_rgb(0xf0, 0xf7, 0xfe));
    field.sort_by(|a, b| a.top().total_cmp(&b.top()));
    assert_eq!(field.len(), 2, "{field:?}");
    (u, rect, field[0], field[1])
}

#[test]
fn a_clicked_bar_is_selected_and_delete_removes_it_and_not_the_layer() {
    let (mut u, rect, scale, _) = two_fields();
    // Between the ticks, on the bar itself.
    click(&mut u, egui::pos2(scale.left() + 30.0, scale.center().y), Modifiers::NONE);
    let s = shapes(&mut u);
    assert_eq!(bars(&s, egui::Color32::from_rgb(0xf0, 0xf7, 0xfe)).len(), 1, "the selected bar is filled");
    press(&mut u, egui::Key::Delete, Modifiers::NONE);
    assert_eq!(fields(&u, rect), ["OPACITY"]);
    assert!(u.app.session.doc().unwrap().node(rect).is_some(), "the layer stays");
    u.app.run("edit.undo", json!({}));
    assert_eq!(fields(&u, rect).len(), 2, "one undo brings it back");
}

#[test]
fn shift_adds_a_bar_and_the_layer_bar_selects_all_of_its_animations() {
    let (mut u, rect, scale, opacity) = two_fields();
    click(&mut u, egui::pos2(scale.left() + 30.0, scale.center().y), Modifiers::NONE);
    click(&mut u, egui::pos2(opacity.left() + 30.0, opacity.center().y), Modifiers::SHIFT);
    press(&mut u, egui::Key::Backspace, Modifiers::NONE);
    assert!(fields(&u, rect).is_empty(), "both went");
    u.app.run("edit.undo", json!({}));
    assert_eq!(fields(&u, rect).len(), 2, "in one undo step");
    // The layer's bar: all of them.
    let s = shapes(&mut u);
    let layer = bars(&s, egui::Color32::from_rgb(0x00, 0x7b, 0xe5))[0];
    click(&mut u, egui::pos2(layer.left() + 30.0, layer.center().y), Modifiers::NONE);
    press(&mut u, egui::Key::Delete, Modifiers::NONE);
    assert!(fields(&u, rect).is_empty());
}

#[test]
fn select_all_in_the_timeline_takes_every_animation_and_escape_lets_go() {
    let (mut u, rect, scale, _) = two_fields();
    let outlined = |u: &mut Ui| bars(&shapes(u), egui::Color32::from_rgb(0xf0, 0xf7, 0xfe)).len();
    // A click on the track area's empty part gives the timeline the keyboard.
    click(&mut u, egui::pos2(scale.right() + 200.0, scale.center().y), Modifiers::NONE);
    press(&mut u, egui::Key::A, Modifiers::COMMAND);
    assert_eq!(outlined(&mut u), 0, "both bars selected (filled)");
    press(&mut u, egui::Key::Escape, Modifiers::NONE);
    assert_eq!(outlined(&mut u), 2, "Escape lets go of them");
    assert_eq!(fields(&u, rect).len(), 2);
    press(&mut u, egui::Key::A, Modifiers::COMMAND);
    press(&mut u, egui::Key::Delete, Modifiers::NONE);
    assert!(fields(&u, rect).is_empty(), "select all, then delete");
    assert!(u.app.session.doc().unwrap().node(rect).is_some(), "the layer stays");
}

/// A keyframe in the middle of a bar: the name moves to the widest gap, clear of every tick.
#[test]
fn a_bars_name_keeps_clear_of_its_ticks() {
    let (mut u, rect) = scaled();
    u.app.run("motion.keyframe.add", json!({"ids": [rect], "field": "SCALE_X", "time": 0.25, "value": 1.2}));
    let s = shapes(&mut u);
    let (_, at, size) =
        texts(&s).into_iter().find(|(t, at, _)| t == "Scale X" && at.y > TIMELINE_TOP + 60.0 && at.x > 300.0).expect("the bar's name");
    let ticks: Vec<egui::Rect> = s
        .iter()
        .filter_map(|s| match s {
            Shape::Rect(r) if (r.rect.width() - 1.5).abs() < 0.01 && (r.rect.height() - 9.0).abs() < 0.01 => Some(r.rect),
            _ => None,
        })
        .filter(|r| r.center().y > at.y && r.center().y < at.y + size.y)
        .collect();
    assert_eq!(ticks.len(), 3, "{ticks:?}");
    for t in ticks {
        assert!(t.right() < at.x || t.left() > at.x + size.x, "tick {t:?} runs through the name at {at:?} {size:?}");
    }
}

/// The timeline's end drags: shorter to the left, longer to the right, set when let go.
#[test]
fn dragging_the_timelines_end_changes_its_length() {
    let (mut u, rect) = scaled();
    let frame = u.app.session.doc().unwrap().top_level_frame(rect).unwrap();
    let length = |u: &Ui| u.app.session.doc().unwrap().timeline_duration(frame);
    let before = length(&u);
    let s = shapes(&mut u);
    // The 0 s and 1 s labels give the scale; the end sits at the timeline's length.
    let label_x = |t: &str| texts(&s).into_iter().find(|(x, at, _)| x == t && at.y > TIMELINE_TOP).map(|(_, at, size)| at.x + size.x / 2.0).unwrap();
    let (x0, x1) = (label_x("0s"), label_x("1s"));
    let px = x1 - x0;
    let end = egui::pos2(x0 + px * before as f32, TIMELINE_TOP + 120.0);
    let to = egui::pos2(end.x - px * 0.5, end.y);
    u.frame(vec![Event::PointerMoved(end)]);
    u.frame(vec![Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
    for k in 1..=6 {
        u.frame(vec![Event::PointerMoved(end + (to - end) * (k as f32 / 6.0))]);
    }
    u.frame(vec![Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    u.frame(vec![]);
    let after = length(&u);
    assert!((after - (before - 0.5)).abs() < 0.03, "{before} → {after}");
}
