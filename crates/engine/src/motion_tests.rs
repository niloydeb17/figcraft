//! Motion commands: keyframes, presets, the timeline's length, the playhead and undo.

use figcraft_doc::NodeId;
use figcraft_geom::Point;
use serde_json::{Value, json};

use crate::Session;

fn ex(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn idv(v: &Value) -> NodeId {
    serde_json::from_value(v["id"].clone()).or_else(|_| serde_json::from_value(v["ids"][0].clone())).unwrap()
}

/// A frame with a 100×50 rectangle at (20, 30) inside it.
fn setup() -> (Session, NodeId, NodeId) {
    let mut s = Session::new();
    let frame = idv(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 400, "height": 300, "name": "Screen"})));
    let rect = idv(&ex(&mut s, "shape.rectangle", json!({"x": 20, "y": 30, "width": 100, "height": 50, "parent": frame})));
    (s, frame, rect)
}

fn x_at(s: &Session, page_t: f64, id: NodeId) -> f64 {
    let ds = s.ds().unwrap();
    let doc = ds.doc.at_time(ds.page, page_t).unwrap();
    (doc.node(id).unwrap().transform * Point::ORIGIN).x
}

#[test]
fn keyframes_animate_the_layer_and_undo_removes_them() {
    let (mut s, frame, rect) = setup();
    ex(&mut s, "motion.keyframe.add", json!({"ids": [rect], "field": "TRANSLATION_X", "time": 0, "value": 0, "easing": "linear"}));
    ex(&mut s, "motion.keyframe.add", json!({"ids": [rect], "field": "TRANSLATION_X", "time": 1, "value": 100, "easing": {"type": "LINEAR"}}));
    assert!((x_at(&s, 0.5, rect) - 70.0).abs() < 1e-9);
    assert!((x_at(&s, 2.0, rect) - 120.0).abs() < 1e-9);
    // The document's own layer stays at rest.
    assert_eq!((s.doc().unwrap().node(rect).unwrap().transform * Point::ORIGIN).x, 20.0);
    let got = ex(&mut s, "motion.get", json!({"ids": [rect]}));
    assert_eq!(got["timeline"]["id"], json!(frame));
    assert_eq!(got["layers"][0]["tracks"]["TRANSLATION_X"][1]["easing"], json!("LINEAR"));
    ex(&mut s, "edit.undo", json!({}));
    ex(&mut s, "edit.undo", json!({}));
    assert!(!s.doc().unwrap().node(rect).unwrap().is_animated());
}

#[test]
fn a_keyframe_without_a_value_records_what_is_shown() {
    let (mut s, _frame, rect) = setup();
    ex(&mut s, "motion.keyframe.add", json!({"ids": [rect], "field": "OPACITY", "time": 0}));
    ex(&mut s, "motion.keyframe.add", json!({"ids": [rect], "field": "fills/0", "time": 0}));
    let got = ex(&mut s, "motion.get", json!({"ids": [rect]}));
    assert_eq!(got["layers"][0]["tracks"]["OPACITY"][0]["value"], json!(1.0));
    assert!(got["layers"][0]["tracks"]["fills/0"][0]["value"].is_object());
}

#[test]
fn top_level_frames_and_loose_layers_are_refused() {
    let (mut s, frame, _rect) = setup();
    let e = s.execute("motion.keyframe.add", &json!({"ids": [frame], "field": "OPACITY", "value": 0}));
    assert!(e.is_err());
    let page = s.ds().unwrap().page;
    let loose = idv(&ex(&mut s, "shape.rectangle", json!({"x": 900, "y": 0, "width": 10, "height": 10, "parent": page})));
    assert!(s.execute("motion.keyframe.add", &json!({"ids": [loose], "field": "OPACITY", "value": 0})).is_err());
    assert!(s.execute("motion.keyframe.add", &json!({"ids": [_rect], "field": "SHEAR", "value": 0})).is_err());
    assert!(s.execute("motion.keyframe.add", &json!({"ids": [_rect], "field": "OPACITY", "value": "x"})).is_err());
}

#[test]
fn presets_extend_but_never_shorten_a_set_timeline() {
    let (mut s, frame, rect) = setup();
    ex(&mut s, "motion.timeline.duration", json!({"id": frame, "seconds": 1}));
    let v =
        ex(&mut s, "motion.preset", json!({"ids": [rect], "preset": "position", "direction": "left", "amount": 100, "start": 1.0, "duration": 0.5}));
    assert_eq!(v["end"], json!(1.5));
    assert_eq!(s.doc().unwrap().timeline_duration(frame), 1.5);
    // Sliding in from the left starts 100 px to the left.
    assert!((x_at(&s, 1.0, rect) - (20.0 - 100.0)).abs() < 1e-9);
    assert!((x_at(&s, 1.5, rect) - 20.0).abs() < 1e-9);
    ex(&mut s, "motion.timeline.duration", json!({"id": rect, "seconds": 4}));
    assert_eq!(s.doc().unwrap().timeline_duration(frame), 4.0);
}

#[test]
fn editing_and_removing_keyframes() {
    let (mut s, _frame, rect) = setup();
    ex(&mut s, "motion.preset", json!({"ids": [rect], "preset": "opacity", "start": 0, "duration": 1}));
    ex(&mut s, "motion.keyframe.set", json!({"ids": [rect], "field": "OPACITY", "time": 1, "newTime": 2, "easing": "hold"}));
    let got = ex(&mut s, "motion.get", json!({"ids": [rect]}));
    assert_eq!(got["layers"][0]["tracks"]["OPACITY"][1]["time"], json!(2.0));
    assert_eq!(got["layers"][0]["tracks"]["OPACITY"][1]["easing"], json!("HOLD"));
    ex(&mut s, "motion.keyframe.remove", json!({"ids": [rect], "time": 2}));
    assert!(s.execute("motion.keyframe.remove", &json!({"ids": [rect], "time": 2})).is_err());
    ex(&mut s, "motion.clear", json!({"ids": [rect]}));
    assert!(s.doc().unwrap().node(rect).unwrap().motion.is_none());
}

#[test]
fn the_playhead_plays_stops_at_the_end_and_loops() {
    let (mut s, frame, rect) = setup();
    ex(&mut s, "select.set", json!({"ids": [rect]}));
    ex(&mut s, "motion.timeline.duration", json!({"id": frame, "seconds": 2}));
    ex(&mut s, "view.motionTimeline", json!({"show": true}));
    assert_eq!(ex(&mut s, "motion.seek", json!({"time": 9}))["time"], json!(2.0));
    // Playback loops by default, as the incumbent's default playback style does; without loop
    // it stops at the end. Playing from the end starts over.
    assert!(s.ds().unwrap().motion.looping);
    assert_eq!(ex(&mut s, "motion.play", json!({"loop": false}))["time"], json!(0.0));
    let ds = s.ds_mut().unwrap();
    // One step moves at most a second (a stalled window doesn't jump to the end).
    ds.motion.advance(5.0, 2.0);
    ds.motion.advance(0.5, 2.0);
    assert!(ds.motion.playing && (ds.motion.playhead - 1.5).abs() < 1e-9);
    ds.motion.advance(1.0, 2.0);
    assert!(!ds.motion.playing && ds.motion.playhead == 2.0);
    ds.motion.playing = true;
    ds.motion.looping = true;
    ds.motion.playhead = 1.5;
    ds.motion.advance(1.0, 2.0);
    assert!(ds.motion.playing && (ds.motion.playhead - 0.5).abs() < 1e-9);
}

#[test]
fn motion_survives_save_and_open() {
    let (mut s, _frame, rect) = setup();
    ex(
        &mut s,
        "motion.preset",
        json!({"ids": [rect], "preset": "rotation", "direction": "counterClockwise", "start": 0, "duration": 1, "easing": "bouncy"}),
    );
    let dir = std::env::temp_dir().join(format!("figcraft-motion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("spin.figcraft");
    ex(&mut s, "file.saveAs", json!({"path": path.to_string_lossy()}));
    let mut t = Session::new();
    ex(&mut t, "file.open", json!({"path": path.to_string_lossy()}));
    let n = t.doc().unwrap().node(rect).unwrap();
    assert_eq!(n.motion, s.doc().unwrap().node(rect).unwrap().motion);
    std::fs::remove_dir_all(&dir).ok();
}
