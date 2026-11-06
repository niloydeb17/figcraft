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

/// FigCraft's ready-made animations: twelve of each group, listed by `motion.styles`; each writes
/// keyframes from the playhead in time order; Enter ends at the layer's rest, Exit starts there,
/// Cycle starts and ends there (so it can repeat).
#[test]
fn every_ready_made_animation_starts_or_ends_at_rest_as_its_group_says() {
    let listed = {
        let (mut s, ..) = setup();
        ex(&mut s, "motion.styles", json!({}))
    };
    let all = listed["styles"].as_array().unwrap().clone();
    assert_eq!(all.len(), 36);
    for g in ["enter", "cycle", "exit"] {
        assert_eq!(all.iter().filter(|st| st["group"] == g).count(), 12, "{g}");
    }
    for st in &all {
        let (mut s, _frame, rect) = setup();
        ex(&mut s, "node.set", json!({"id": rect, "props": {"opacity": 0.8}}));
        ex(&mut s, "motion.seek", json!({"time": 0.25}));
        let id = st["id"].as_str().unwrap();
        let r = ex(&mut s, "motion.style", json!({"ids": [rect], "style": id}));
        assert!((r["start"].as_f64().unwrap() - 0.25).abs() < 1e-9, "{id}: from the playhead");
        let d = st["duration"].as_f64().unwrap();
        assert!(d > 0.0 && (r["end"].as_f64().unwrap() - (0.25 + d)).abs() < 1e-6, "{id}: {r}");
        let n = s.doc().unwrap().node(rect).unwrap().clone();
        let m = n.motion.as_ref().unwrap();
        for (f, track) in m.fields() {
            let ks = &track.keyframes;
            assert!(ks.len() >= 2, "{id} {f:?}");
            assert!(ks.windows(2).all(|w| w[0].time < w[1].time), "{id} {f:?}: in time order");
            let rest = match f {
                figcraft_doc::motion::MotionField::ScaleX | figcraft_doc::motion::MotionField::ScaleY => 1.0,
                figcraft_doc::motion::MotionField::Opacity => 0.8,
                _ => 0.0,
            };
            let first = ks[0].value.as_f64().unwrap();
            let last = ks[ks.len() - 1].value.as_f64().unwrap();
            // A whole turn looks the same as none.
            let turn = matches!(f, figcraft_doc::motion::MotionField::Rotation);
            let at_rest = |v: f64| {
                let d = v - rest;
                if turn { d.rem_euclid(360.0).min(360.0 - d.rem_euclid(360.0)) < 1e-6 } else { d.abs() < 1e-6 }
            };
            match st["group"].as_str().unwrap() {
                "enter" => assert!(at_rest(last), "{id} {f:?} ends at rest: {last}"),
                "exit" => assert!(at_rest(first), "{id} {f:?} starts at rest: {first}"),
                _ => assert!(at_rest(first) && at_rest(last), "{id} {f:?} starts and ends at rest: {first}..{last}"),
            }
            assert!(ks.iter().all(|k| k.value.as_f64().is_some_and(f64::is_finite)));
        }
        // The timeline grew to hold it, and the layer is drawn at every moment.
        assert!(s.doc().unwrap().timeline_duration(_frame) + 1e-9 >= 0.25 + d, "{id}");
        let page = s.ds().unwrap().page;
        for t in [0.0, 0.25, 0.25 + d / 2.0, 0.25 + d] {
            assert!(s.ds().unwrap().doc.at_time(page, t).is_some(), "{id} at {t}");
        }
    }
}

#[test]
fn a_ready_made_animation_by_name_and_undo_and_junk() {
    let (mut s, _frame, rect) = setup();
    ex(&mut s, "select.set", json!({"ids": [rect]}));
    let r = ex(&mut s, "motion.style", json!({"style": "Rise", "start": 0}));
    assert_eq!(r["style"], "rise");
    assert_eq!(r["fields"], json!(["OPACITY", "TRANSLATION_Y"]));
    ex(&mut s, "edit.undo", json!({}));
    assert!(s.doc().unwrap().node(rect).unwrap().motion.as_ref().is_none_or(|m| m.fields().is_empty()), "undone");
    assert!(s.execute("motion.style", &json!({"ids": [rect], "style": "nope"})).is_err());
    assert!(s.execute("motion.style", &json!({"ids": [rect]})).is_err());
    assert!(s.execute("motion.styles", &json!({"group": "sideways"})).is_err());
    assert_eq!(ex(&mut s, "motion.styles", json!({"group": "cycle"}))["styles"].as_array().unwrap().len(), 12);
}

/// Several animations removed at once (the timeline's Delete): one undo step brings them back.
#[test]
fn several_tracks_go_in_one_undo_step() {
    let (mut s, _frame, rect) = setup();
    ex(&mut s, "motion.style", json!({"ids": [rect], "style": "burst", "start": 0}));
    let before = s.doc().unwrap().node(rect).unwrap().motion.clone();
    let r = ex(
        &mut s,
        "motion.tracks.remove",
        json!({"tracks": [{"id": rect, "field": "SCALE_X"}, {"id": rect, "field": "SCALE_Y"}, {"id": rect, "field": "OPACITY"}, {"id": rect, "field": "ROTATION"}]}),
    );
    assert_eq!(r["removed"], 3, "the one not animated is skipped");
    assert!(s.doc().unwrap().node(rect).unwrap().motion.as_ref().is_none_or(|m| m.fields().is_empty()));
    ex(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc().unwrap().node(rect).unwrap().motion, before);
    assert!(s.execute("motion.tracks.remove", &json!({"tracks": [{"id": rect, "field": "OPACITY"}]})).is_ok());
    assert!(s.execute("motion.tracks.remove", &json!({"tracks": [{"id": rect, "field": "OPACITY"}]})).is_err(), "nothing left");
    assert!(s.execute("motion.tracks.remove", &json!({"tracks": "x"})).is_err());
    assert!(s.execute("motion.tracks.remove", &json!({"tracks": [{"field": "OPACITY"}]})).is_err());
}
