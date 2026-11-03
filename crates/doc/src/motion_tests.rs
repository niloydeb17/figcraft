//! Motion: sampling keyframes, composing with the resting layer, timelines and presets.

use figcraft_color::{Color, Paint, PaintKind};
use figcraft_geom::{Affine, Point, Vec2};

use crate::motion::*;
use crate::proto::Easing;
use crate::{Document, Node, NodeId, NodeKind};

fn kf(time: f64, v: f64, easing: Easing) -> Keyframe {
    Keyframe { time, value: MotionValue::Float(v), easing, hold: false }
}

fn track(k: &[Keyframe]) -> Track {
    Track { keyframes: k.to_vec() }
}

fn num(t: &Track, at: f64) -> f64 {
    t.sample(at).and_then(MotionValue::as_f64).unwrap_or(f64::NAN)
}

#[test]
fn values_hold_before_the_first_and_after_the_last_keyframe() {
    let t = track(&[kf(1.0, 10.0, Easing::Linear), kf(2.0, 20.0, Easing::Linear)]);
    assert_eq!(num(&t, 0.0), 10.0);
    assert_eq!(num(&t, 1.5), 15.0);
    assert_eq!(num(&t, 5.0), 20.0);
    assert!(Track::default().sample(1.0).is_none());
}

#[test]
fn easing_shapes_the_way_into_a_keyframe_and_hold_steps() {
    let out = track(&[kf(0.0, 0.0, Easing::Linear), kf(1.0, 100.0, Easing::EaseOut)]);
    // Ease out at half way: cubic-bezier(0, 0, 0.58, 1).
    assert!((num(&out, 0.5) - 100.0 * motion_ease(Easing::EaseOut, 0.5)).abs() < 1e-9);
    let mut hold = out.clone();
    if let Some(k) = hold.keyframes.get_mut(1) {
        k.hold = true;
    }
    assert_eq!(num(&hold, 0.999), 0.0);
    assert_eq!(num(&hold, 1.0), 100.0);
    // A back easing overshoots past the end value.
    let back = track(&[kf(0.0, 0.0, Easing::Linear), kf(1.0, 100.0, Easing::EaseOutBack)]);
    assert!((0..100).map(|i| num(&back, f64::from(i) / 100.0)).any(|v| v > 100.0));
}

#[test]
fn setting_a_keyframe_at_the_same_time_replaces_it_and_keeps_order() {
    let mut t = Track::default();
    t.set(kf(1.0, 1.0, Easing::Linear));
    t.set(kf(0.5, 2.0, Easing::Linear));
    t.set(kf(1.000_000_4, 3.0, Easing::Linear));
    assert_eq!(t.keyframes.len(), 2);
    assert_eq!(t.keyframes.iter().map(|k| k.time).collect::<Vec<_>>(), vec![0.5, 1.0]);
    assert_eq!(num(&t, 1.0), 3.0);
    assert!(t.remove_at(0.5));
    assert!(!t.remove_at(0.5));
    // Junk times are made valid.
    t.set(kf(f64::NAN, 4.0, Easing::Linear));
    t.set(kf(1e12, 5.0, Easing::Linear));
    assert!(t.keyframes.iter().all(|k| k.time.is_finite() && (0.0..=MAX_TIMELINE).contains(&k.time)));
}

#[test]
fn field_keys_round_trip() {
    for f in MotionField::BASIC.into_iter().chain([MotionField::Fill(0), MotionField::Fill(3), MotionField::Stroke(1)]) {
        assert_eq!(MotionField::parse(&f.key()), Some(f), "{f:?}");
    }
    assert_eq!(MotionField::parse("translationX"), Some(MotionField::TranslationX));
    assert_eq!(MotionField::parse("fills/999"), None);
    assert_eq!(MotionField::parse("SHEAR"), None);
}

fn doc_with_rect() -> (Document, NodeId, NodeId, NodeId) {
    let mut d = Document::new("motion");
    let page = d.first_page().unwrap();
    let frame = d.create(page, NodeKind::Frame, "Frame", 0.0, 0.0, 400.0, 300.0).unwrap();
    let rect = d.create(frame, NodeKind::Rectangle, "Box", 20.0, 30.0, 100.0, 50.0).unwrap();
    let r = d.get_mut(rect).unwrap();
    r.transform = Affine::translate(Vec2::new(20.0, 30.0));
    r.fills = vec![Paint::solid(Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 })];
    (d, page, frame, rect)
}

fn animate(d: &mut Document, id: NodeId, f: MotionField, keys: &[Keyframe]) {
    if let Ok(n) = d.get_mut(id) {
        let m = n.motion.get_or_insert_with(Default::default);
        *m.track_mut(f) = track(keys);
    }
}

#[test]
fn transform_fields_compose_with_the_resting_transform() {
    let (mut d, page, _frame, rect) = doc_with_rect();
    animate(&mut d, rect, MotionField::TranslationX, &[kf(0.0, 0.0, Easing::Linear), kf(1.0, 50.0, Easing::Linear)]);
    animate(&mut d, rect, MotionField::ScaleX, &[kf(0.0, 1.0, Easing::Linear), kf(1.0, 2.0, Easing::Linear)]);
    let at = d.at_time(page, 1.0).unwrap();
    let n = at.node(rect).unwrap();
    // Scale 2 about the centre (50, 25) then a 50 px move: the left edge goes from 20 to 20 - 50 + 50.
    let left = n.transform * Point::new(0.0, 0.0);
    assert!((left.x - 20.0).abs() < 1e-9 && (left.y - 30.0).abs() < 1e-9, "{left:?}");
    let right = n.transform * Point::new(100.0, 0.0);
    assert!((right.x - 220.0).abs() < 1e-9, "{right:?}");
    // At the start nothing moved, and the document itself is untouched.
    let start = d.at_time(page, 0.0).unwrap();
    assert_eq!(start.node(rect).unwrap().transform, d.node(rect).unwrap().transform);
}

#[test]
fn rotation_is_counter_clockwise_about_the_centre() {
    let (mut d, page, _frame, rect) = doc_with_rect();
    animate(&mut d, rect, MotionField::Rotation, &[kf(0.0, 0.0, Easing::Linear), kf(1.0, 90.0, Easing::Linear)]);
    let at = d.at_time(page, 1.0).unwrap();
    let n = at.node(rect).unwrap();
    let centre = n.transform * Point::new(50.0, 25.0);
    assert!((centre.x - 70.0).abs() < 1e-9 && (centre.y - 55.0).abs() < 1e-9);
    assert!((figcraft_geom::rotation_degrees(n.transform) - 90.0).abs() < 1e-6);
}

#[test]
fn absolute_fields_replace_the_value_and_colours_blend() {
    let (mut d, page, _frame, rect) = doc_with_rect();
    animate(&mut d, rect, MotionField::Opacity, &[kf(0.0, 0.0, Easing::Linear), kf(1.0, 1.0, Easing::Linear)]);
    animate(&mut d, rect, MotionField::Width, &[kf(0.0, 100.0, Easing::Linear), kf(1.0, 300.0, Easing::Linear)]);
    if let Ok(n) = d.get_mut(rect) {
        let m = n.motion.get_or_insert_with(Default::default);
        let blue = Color { r: 0.0, g: 0.0, b: 1.0, a: 1.0 };
        let red = Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 };
        *m.track_mut(MotionField::Fill(0)) = Track {
            keyframes: vec![
                Keyframe { time: 0.0, value: MotionValue::Color(red), easing: Easing::Linear, hold: false },
                Keyframe { time: 1.0, value: MotionValue::Color(blue), easing: Easing::Linear, hold: false },
            ],
        };
    }
    let at = d.at_time(page, 0.5).unwrap();
    let n = at.node(rect).unwrap();
    assert!((n.opacity - 0.5).abs() < 1e-6);
    assert!((n.width - 200.0).abs() < 1e-9);
    let PaintKind::Solid { color } = n.fills[0].kind else { panic!("solid") };
    assert!((color.r - 0.5).abs() < 1e-6 && (color.b - 0.5).abs() < 1e-6);
}

#[test]
fn timelines_default_stretch_to_the_last_keyframe_and_clamp_the_playhead() {
    let (mut d, page, frame, rect) = doc_with_rect();
    assert_eq!(d.timeline_duration(frame), DEFAULT_TIMELINE);
    assert!(d.at_time(page, 1.0).is_none(), "nothing animated");
    animate(&mut d, rect, MotionField::TranslationY, &[kf(0.0, 0.0, Easing::Linear), kf(5.0, 100.0, Easing::Linear)]);
    assert_eq!(d.timeline_duration(frame), 5.0);
    if let Ok(f) = d.get_mut(frame) {
        f.motion.get_or_insert_with(Default::default).duration = Some(2.0);
    }
    assert_eq!(d.timeline_duration(frame), 2.0);
    // Past the end of a 2 s timeline the layer shows the 2 s moment.
    let late = d.at_time(page, 9.0).unwrap();
    let y = (late.node(rect).unwrap().transform * Point::ORIGIN).y;
    assert!((y - 70.0).abs() < 1e-9, "{y}");
}

#[test]
fn presets_write_tracks_from_away_to_rest() {
    let (d, _page, _frame, rect) = doc_with_rect();
    let n = d.node(rect).unwrap();
    let fade = Preset::Opacity.tracks(n, &PresetArgs { duration: 0.5, ..Default::default() });
    assert_eq!(fade.len(), 1);
    let (f, t) = &fade[0];
    assert_eq!(*f, MotionField::Opacity);
    assert_eq!((num(t, 0.0), num(t, 0.5)), (0.0, 1.0));
    // Position slides in from the left by default, 200 px away.
    let slide = Preset::Position.tracks(n, &PresetArgs::default());
    let x = slide.iter().find(|(f, _)| *f == MotionField::TranslationX).map(|(_, t)| t).unwrap();
    assert_eq!((num(x, 0.0), num(x, 0.5)), (-200.0, 0.0));
    let out = Preset::Position.tracks(n, &PresetArgs { start: 1.0, inward: false, side: Side::BottomRight, ..Default::default() });
    let y = out.iter().find(|(f, _)| *f == MotionField::TranslationY).map(|(_, t)| t).unwrap();
    assert_eq!((num(y, 1.0), num(y, 1.5)), (0.0, 200.0));
    // Scale in from 120 %, rotate clockwise into place from 180°, grow from 50 px larger.
    let scale = Preset::Scale.tracks(n, &PresetArgs::default());
    assert!((num(&scale[0].1, 0.0) - 1.2).abs() < 1e-12);
    let rot = Preset::Rotation.tracks(n, &PresetArgs::default());
    assert_eq!((num(&rot[0].1, 0.0), num(&rot[0].1, 0.5)), (180.0, 0.0));
    let rot_out = Preset::Rotation.tracks(n, &PresetArgs { inward: false, ..Default::default() });
    assert_eq!(num(&rot_out[0].1, 0.5), -180.0);
    let size = Preset::Size.tracks(n, &PresetArgs::default());
    assert_eq!((num(&size[0].1, 0.0), num(&size[0].1, 0.5)), (150.0, 100.0));
    for p in Preset::ALL {
        assert_eq!(Preset::parse(p.id()), Some(p));
    }
}

#[test]
fn named_easings_are_the_incumbents_bezier_curves() {
    // Ease out is cubic-bezier(0, 0, 0.58, 1), not 1 - (1 - t)³.
    let mid = motion_ease(Easing::EaseOut, 0.5);
    assert!((mid - 0.875).abs() > 0.01 && mid > 0.5 && mid < 1.0, "{mid}");
    assert_eq!(bezier_points(Easing::EaseInOutBack), Some((0.7, -0.4, 0.4, 1.4)));
    assert!(bezier_points(Easing::Gentle).is_none());
    for e in [Easing::Linear, Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
        assert!(motion_ease(e, 0.0).abs() < 1e-9 && (motion_ease(e, 1.0) - 1.0).abs() < 1e-9);
    }
    assert!((0..=100).any(|i| motion_ease(Easing::EaseOutBack, f64::from(i) / 100.0) > 1.0));
}

#[test]
fn motion_round_trips_through_json() {
    let (mut d, _page, _frame, rect) = doc_with_rect();
    animate(&mut d, rect, MotionField::Rotation, &[kf(0.0, 0.0, Easing::Linear), kf(1.0, -360.0, Easing::Gentle)]);
    let n = d.node(rect).unwrap();
    let json = serde_json::to_string(n).unwrap();
    assert!(json.contains("ROTATION"), "{json}");
    let back: Node = serde_json::from_str(&json).unwrap();
    assert_eq!(back.motion, n.motion);
}
