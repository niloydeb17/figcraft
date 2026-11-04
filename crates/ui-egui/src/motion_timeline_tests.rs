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
