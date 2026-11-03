//! Motion: keyframe animation on a top-level frame's timeline.
//!
//! Behaviour follows Figma's public Motion plugin-API documentation (see `MECHANICS.md`):
//! - a layer holds keyframe tracks, one per animatable field; positions on the timeline are seconds;
//! - a keyframe's easing shapes the way *into* it from the previous keyframe (`hold` steps);
//! - before the first keyframe a field holds the first value, after the last the last value;
//! - transform fields compose with the layer's resting transform (translation and rotation add,
//!   scale multiplies, rotation and scale pivot on the layer's centre); the other fields replace
//!   the layer's value while animated;
//! - every top-level frame has one timeline with a length in seconds; top-level frames themselves
//!   aren't animated, their contents are.

use std::collections::BTreeMap;

use figcraft_color::{Color, PaintKind};
use figcraft_geom::{Affine, Vec2};
use serde::{Deserialize, Serialize};

use crate::proto::Easing;
use crate::{Document, Node, NodeId};

/// Length of a timeline nothing has set (seconds): the incumbent's timeline showed 2000 ms on
/// frames whose length was never set.
pub const DEFAULT_TIMELINE: f64 = 2.0;
/// Longest timeline (seconds).
pub const MAX_TIMELINE: f64 = 600.0;
/// Shortest timeline (seconds).
pub const MIN_TIMELINE: f64 = 0.1;
/// Most keyframes one track keeps.
pub const MAX_KEYFRAMES: usize = 2000;
/// Keyframe positions are whole microseconds (the incumbent keeps timeline times in µs); two
/// keyframes closer than this are one.
pub const TIME_STEP: f64 = 0.000_001;
/// Largest translation, size or radius a keyframe takes (pixels).
const MAX_PIXELS: f64 = 1.0e6;

/// The fields a keyframe track can animate, named as the public API names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MotionField {
    TranslationX,
    TranslationY,
    Rotation,
    ScaleX,
    ScaleY,
    Opacity,
    CornerRadius,
    StrokeWeight,
    Width,
    Height,
    /// The colour of the solid fill at this index.
    Fill(usize),
    /// The colour of the solid stroke at this index.
    Stroke(usize),
}

impl MotionField {
    /// The fields the timeline offers for every layer, in panel order (fills and strokes add one
    /// per solid paint).
    pub const BASIC: [MotionField; 10] = [
        MotionField::TranslationX,
        MotionField::TranslationY,
        MotionField::Rotation,
        MotionField::ScaleX,
        MotionField::ScaleY,
        MotionField::Opacity,
        MotionField::CornerRadius,
        MotionField::StrokeWeight,
        MotionField::Width,
        MotionField::Height,
    ];

    /// The key the track is saved under (`TRANSLATION_X`, `fills/0`, …).
    pub fn key(self) -> String {
        match self {
            MotionField::Fill(i) => format!("fills/{i}"),
            MotionField::Stroke(i) => format!("strokes/{i}"),
            other => other.simple_key().to_string(),
        }
    }

    fn simple_key(self) -> &'static str {
        match self {
            MotionField::TranslationX => "TRANSLATION_X",
            MotionField::TranslationY => "TRANSLATION_Y",
            MotionField::Rotation => "ROTATION",
            MotionField::ScaleX => "SCALE_X",
            MotionField::ScaleY => "SCALE_Y",
            MotionField::Opacity => "OPACITY",
            MotionField::CornerRadius => "CORNER_RADIUS",
            MotionField::StrokeWeight => "STROKE_WEIGHT",
            MotionField::Width => "WIDTH",
            MotionField::Height => "HEIGHT",
            MotionField::Fill(_) => "fills",
            MotionField::Stroke(_) => "strokes",
        }
    }

    /// Read a track key back (`TRANSLATION_X`, `translationX`, `fills/0`, `fill`, …).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some((coll, idx)) = s.split_once('/') {
            let i: usize = idx.trim().parse().ok().filter(|i| *i < 64)?;
            return match coll.trim().to_ascii_lowercase().as_str() {
                "fills" | "fill" => Some(MotionField::Fill(i)),
                "strokes" | "stroke" => Some(MotionField::Stroke(i)),
                _ => None,
            };
        }
        let norm: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
        Some(match norm.as_str() {
            "translationx" | "x" | "movex" => MotionField::TranslationX,
            "translationy" | "y" | "movey" => MotionField::TranslationY,
            "rotation" | "rotate" => MotionField::Rotation,
            "scalex" => MotionField::ScaleX,
            "scaley" => MotionField::ScaleY,
            "opacity" => MotionField::Opacity,
            "cornerradius" | "radius" => MotionField::CornerRadius,
            "strokeweight" => MotionField::StrokeWeight,
            "width" | "w" => MotionField::Width,
            "height" | "h" => MotionField::Height,
            "fills" | "fill" | "fillcolor" => MotionField::Fill(0),
            "strokes" | "stroke" | "strokecolor" => MotionField::Stroke(0),
            _ => return None,
        })
    }

    /// The name the timeline shows.
    pub fn label(self) -> String {
        match self {
            MotionField::TranslationX => "Position X".into(),
            MotionField::TranslationY => "Position Y".into(),
            MotionField::Rotation => "Rotation".into(),
            MotionField::ScaleX => "Scale X".into(),
            MotionField::ScaleY => "Scale Y".into(),
            MotionField::Opacity => "Opacity".into(),
            MotionField::CornerRadius => "Corner radius".into(),
            MotionField::StrokeWeight => "Stroke weight".into(),
            MotionField::Width => "Width".into(),
            MotionField::Height => "Height".into(),
            MotionField::Fill(0) => "Fill".into(),
            MotionField::Fill(i) => format!("Fill {}", i + 1),
            MotionField::Stroke(0) => "Stroke".into(),
            MotionField::Stroke(i) => format!("Stroke {}", i + 1),
        }
    }

    pub fn is_color(self) -> bool {
        matches!(self, MotionField::Fill(_) | MotionField::Stroke(_))
    }

    /// Transform fields compose with the resting transform; their neutral value means "no change".
    pub fn neutral(self) -> Option<f64> {
        match self {
            MotionField::TranslationX | MotionField::TranslationY | MotionField::Rotation => Some(0.0),
            MotionField::ScaleX | MotionField::ScaleY => Some(1.0),
            _ => None,
        }
    }

    /// The layer's own value of the field at rest: a transform field's neutral value, or the
    /// layer's value (None: the layer has no such field, a fill index past its fills or a paint
    /// that isn't solid).
    pub fn rest_value(self, n: &Node) -> Option<MotionValue> {
        if let Some(v) = self.neutral() {
            return Some(MotionValue::Float(v));
        }
        Some(match self {
            MotionField::Opacity => MotionValue::Float(f64::from(n.opacity)),
            MotionField::CornerRadius => MotionValue::Float(n.corners.radii.first().copied().unwrap_or(0.0)),
            MotionField::StrokeWeight => MotionValue::Float(n.stroke.weight),
            MotionField::Width => MotionValue::Float(n.width),
            MotionField::Height => MotionValue::Float(n.height),
            MotionField::Fill(i) => MotionValue::Color(solid(n.fills.get(i)?)?),
            MotionField::Stroke(i) => MotionValue::Color(solid(n.strokes.get(i)?)?),
            _ => return None,
        })
    }

    /// `v` made valid for this field (None: a colour for a number field or the other way round,
    /// or not a finite number).
    pub fn clean(self, v: MotionValue) -> Option<MotionValue> {
        match (self.is_color(), v) {
            (true, MotionValue::Color(c)) => {
                let f = |x: f32| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
                Some(MotionValue::Color(Color { r: f(c.r), g: f(c.g), b: f(c.b), a: f(c.a) }))
            }
            (false, MotionValue::Float(x)) if x.is_finite() => Some(MotionValue::Float(match self {
                MotionField::Opacity => x.clamp(0.0, 1.0),
                MotionField::Width | MotionField::Height => x.clamp(0.01, MAX_PIXELS),
                MotionField::CornerRadius | MotionField::StrokeWeight => x.clamp(0.0, MAX_PIXELS),
                MotionField::ScaleX | MotionField::ScaleY => x.clamp(-1000.0, 1000.0),
                MotionField::Rotation => x.clamp(-36_000.0, 36_000.0),
                _ => x.clamp(-MAX_PIXELS, MAX_PIXELS),
            })),
            _ => None,
        }
    }
}

fn solid(p: &figcraft_color::Paint) -> Option<Color> {
    match &p.kind {
        PaintKind::Solid { color } => Some(*color),
        _ => None,
    }
}

/// A keyframe's value: a number (pixels, degrees, a multiplier, an opacity) or a colour.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MotionValue {
    Float(f64),
    Color(Color),
}

impl MotionValue {
    pub fn as_f64(self) -> Option<f64> {
        match self {
            MotionValue::Float(v) => Some(v),
            MotionValue::Color(_) => None,
        }
    }

    /// The value `u` of the way from `self` to `to` (`u` may leave 0..1 for overshooting easings;
    /// colours stay in range).
    pub fn lerp(self, to: MotionValue, u: f64) -> MotionValue {
        match (self, to) {
            (MotionValue::Float(a), MotionValue::Float(b)) => MotionValue::Float(a + (b - a) * u),
            (MotionValue::Color(a), MotionValue::Color(b)) => {
                let u = u as f32;
                let m = |x: f32, y: f32| (x + (y - x) * u).clamp(0.0, 1.0);
                MotionValue::Color(Color { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: m(a.a, b.a) })
            }
            // Mismatched kinds can't blend: step at the end.
            (a, b) => {
                if u >= 1.0 {
                    b
                } else {
                    a
                }
            }
        }
    }
}

/// One keyframe: the value at a position on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Keyframe {
    /// Seconds from the start of the timeline.
    pub time: f64,
    pub value: MotionValue,
    /// How the value moves from the previous keyframe to this one.
    #[serde(default)]
    pub easing: Easing,
    /// Hold: keep the previous keyframe's value until this one, then step.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hold: bool,
}

/// One animated field's keyframes, in time order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Track {
    #[serde(default)]
    pub keyframes: Vec<Keyframe>,
}

impl Track {
    /// The value at `t` seconds (None: no keyframes).
    pub fn sample(&self, t: f64) -> Option<MotionValue> {
        let first = self.keyframes.first()?;
        let t = if t.is_finite() { t } else { 0.0 };
        if t <= first.time {
            return Some(first.value);
        }
        for pair in self.keyframes.windows(2) {
            let [a, b] = pair else { continue };
            if t < b.time {
                if b.hold {
                    return Some(a.value);
                }
                let span = b.time - a.time;
                let u = if span > 0.0 { (t - a.time) / span } else { 1.0 };
                return Some(a.value.lerp(b.value, motion_ease(b.easing, u)));
            }
        }
        self.keyframes.last().map(|k| k.value)
    }

    /// Put a keyframe at `k.time` (replacing one already there). Its index, or None when the
    /// track is full.
    pub fn set(&mut self, mut k: Keyframe) -> Option<usize> {
        k.time = snap_time(k.time);
        if let Some(i) = self.keyframes.iter().position(|e| (e.time - k.time).abs() < TIME_STEP / 2.0) {
            if let Some(slot) = self.keyframes.get_mut(i) {
                *slot = k;
            }
            return Some(i);
        }
        if self.keyframes.len() >= MAX_KEYFRAMES {
            return None;
        }
        let i = self.keyframes.iter().position(|e| e.time > k.time).unwrap_or(self.keyframes.len());
        self.keyframes.insert(i, k);
        Some(i)
    }

    /// Remove the keyframe at `t` (within half a millisecond). Whether one was there.
    pub fn remove_at(&mut self, t: f64) -> bool {
        let t = snap_time(t);
        let before = self.keyframes.len();
        self.keyframes.retain(|k| (k.time - t).abs() >= TIME_STEP / 2.0);
        self.keyframes.len() != before
    }

    /// The last keyframe's position (0 with none).
    pub fn end(&self) -> f64 {
        self.keyframes.last().map_or(0.0, |k| k.time)
    }

    /// Keyframes back in time order after their times changed.
    pub fn sort(&mut self) {
        self.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}

/// A position on the timeline made valid: finite, not negative, at most [`MAX_TIMELINE`], whole
/// milliseconds.
pub fn snap_time(t: f64) -> f64 {
    let t = if t.is_finite() { t.clamp(0.0, MAX_TIMELINE) } else { 0.0 };
    (t / TIME_STEP).round() * TIME_STEP
}

/// A layer's motion: its keyframe tracks, and on a top-level frame the timeline's length.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// Field key ([`MotionField::key`]) → track.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tracks: BTreeMap<String, Track>,
    /// Top-level frames: the timeline's length in seconds (None: [`DEFAULT_TIMELINE`], or longer
    /// to reach the last keyframe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
}

impl Motion {
    pub fn is_empty(&self) -> bool {
        self.tracks.values().all(|t| t.keyframes.is_empty()) && self.duration.is_none()
    }

    /// The tracks with keyframes and the field each animates, in field order.
    pub fn fields(&self) -> Vec<(MotionField, &Track)> {
        let mut v: Vec<(MotionField, &Track)> =
            self.tracks.iter().filter(|(_, t)| !t.keyframes.is_empty()).filter_map(|(k, t)| Some((MotionField::parse(k)?, t))).collect();
        v.sort_by_key(|(f, _)| *f);
        v
    }

    pub fn track(&self, f: MotionField) -> Option<&Track> {
        self.tracks.get(&f.key()).filter(|t| !t.keyframes.is_empty())
    }

    pub fn track_mut(&mut self, f: MotionField) -> &mut Track {
        self.tracks.entry(f.key()).or_default()
    }

    /// The last keyframe's position over all tracks.
    pub fn end(&self) -> f64 {
        self.tracks.values().map(Track::end).fold(0.0, f64::max)
    }
}

impl Node {
    /// This layer's keyframes have something to play.
    pub fn is_animated(&self) -> bool {
        self.motion.as_ref().is_some_and(|m| m.tracks.values().any(|t| !t.keyframes.is_empty()))
    }

    /// This layer as it looks `t` seconds into its timeline.
    pub fn animated_at(&self, t: f64) -> Option<Node> {
        let m = self.motion.as_deref()?;
        if !self.is_animated() {
            return None;
        }
        let mut n = self.clone();
        let num = |f: MotionField| m.track(f).and_then(|tr| tr.sample(t)).and_then(MotionValue::as_f64).filter(|v| v.is_finite());
        if let Some(w) = num(MotionField::Width) {
            n.width = w.clamp(0.01, MAX_PIXELS);
        }
        if let Some(h) = num(MotionField::Height) {
            n.height = h.clamp(0.01, MAX_PIXELS);
        }
        if let Some(o) = num(MotionField::Opacity) {
            n.opacity = o.clamp(0.0, 1.0) as f32;
        }
        if let Some(r) = num(MotionField::CornerRadius) {
            n.corners.radii = [r.clamp(0.0, MAX_PIXELS); 4];
        }
        if let Some(sw) = num(MotionField::StrokeWeight) {
            n.stroke.weight = sw.clamp(0.0, MAX_PIXELS);
        }
        for (f, tr) in m.fields() {
            let (paints, i) = match f {
                MotionField::Fill(i) => (&mut n.fills, i),
                MotionField::Stroke(i) => (&mut n.strokes, i),
                _ => continue,
            };
            if let (Some(MotionValue::Color(c)), Some(p)) = (tr.sample(t), paints.get_mut(i))
                && let PaintKind::Solid { color } = &mut p.kind
            {
                *color = c;
            }
        }
        let tx = num(MotionField::TranslationX).unwrap_or(0.0);
        let ty = num(MotionField::TranslationY).unwrap_or(0.0);
        let rot = num(MotionField::Rotation).unwrap_or(0.0);
        let sx = num(MotionField::ScaleX).unwrap_or(1.0);
        let sy = num(MotionField::ScaleY).unwrap_or(1.0);
        if tx != 0.0 || ty != 0.0 || rot != 0.0 || sx != 1.0 || sy != 1.0 {
            // Rotation and scale pivot on the layer's centre (its local box); translation moves it
            // in its parent's space (positive x right, positive y down).
            let c = Vec2::new(n.width / 2.0, n.height / 2.0);
            let local = Affine::translate(c) * Affine::rotate(-rot.to_radians()) * Affine::scale_non_uniform(sx, sy) * Affine::translate(-c);
            let xf = Affine::translate(Vec2::new(tx, ty)) * n.transform * local;
            if xf.as_coeffs().iter().all(|v| v.is_finite()) {
                n.transform = xf;
            }
        }
        Some(n)
    }
}

impl Document {
    /// The length of `frame`'s timeline (seconds): the length set on it, or [`DEFAULT_TIMELINE`]
    /// stretched to reach the last keyframe inside it.
    pub fn timeline_duration(&self, frame: NodeId) -> f64 {
        if let Some(d) = self.node(frame).and_then(|n| n.motion.as_ref()).and_then(|m| m.duration).filter(|d| d.is_finite()) {
            return d.clamp(MIN_TIMELINE, MAX_TIMELINE);
        }
        let end =
            self.animated_in(frame).into_iter().filter_map(|i| self.node(i)).filter_map(|n| n.motion.as_ref()).map(|m| m.end()).fold(0.0, f64::max);
        DEFAULT_TIMELINE.max(end).min(MAX_TIMELINE)
    }

    /// The animated layers inside `root` (itself included), in layer order, top to bottom.
    pub fn animated_in(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![(root, 0usize)];
        while let Some((id, depth)) = stack.pop() {
            let Some(n) = self.node(id) else { continue };
            if n.is_animated() {
                out.push(id);
            }
            if depth < 256 {
                // Children are bottom to top; pushing them in order pops the topmost first.
                stack.extend(n.children.iter().map(|c| (*c, depth + 1)));
            }
        }
        out
    }

    /// Whether anything on `page` is animated.
    pub fn page_has_motion(&self, page: NodeId) -> bool {
        !self.animated_in(page).is_empty()
    }

    /// This document with every animated layer on `page` as it looks `t` seconds into its
    /// top-level frame's timeline (`t` past a timeline's end shows its end). None: nothing on the
    /// page is animated.
    pub fn at_time(&self, page: NodeId, t: f64) -> Option<Document> {
        let ids = self.animated_in(page);
        if ids.is_empty() {
            return None;
        }
        let mut out = self.clone();
        let mut lengths: BTreeMap<NodeId, f64> = BTreeMap::new();
        for id in ids {
            let frame = self.top_level_frame(id).unwrap_or(id);
            let len = *lengths.entry(frame).or_insert_with(|| self.timeline_duration(frame));
            let at = if t.is_finite() { t.clamp(0.0, len) } else { 0.0 };
            let Some(n) = self.node(id).and_then(|n| n.animated_at(at)) else { continue };
            if let Ok(slot) = out.get_mut(id) {
                *slot = n;
            }
        }
        Some(out)
    }
}

/// The incumbent's curve for a keyframe easing at `u` (0..1): the named curves are cubic
/// béziers through these control points (its motion easing table); springs and custom curves
/// are as the prototype easings evaluate them.
pub fn motion_ease(e: Easing, u: f64) -> f64 {
    match bezier_points(e) {
        // The straight line is exact (no search along the curve).
        Some(_) if e == Easing::Linear => e.eval(u),
        Some((x1, y1, x2, y2)) => Easing::CustomBezier { x1, y1, x2, y2 }.eval(u),
        None => e.eval(u),
    }
}

/// The control points of the named bézier easings (None: springs and custom curves).
pub fn bezier_points(e: Easing) -> Option<(f64, f64, f64, f64)> {
    Some(match e {
        Easing::Linear => (0.0, 0.0, 1.0, 1.0),
        Easing::EaseIn => (0.42, 0.0, 1.0, 1.0),
        Easing::EaseOut => (0.0, 0.0, 0.58, 1.0),
        Easing::EaseInOut => (0.42, 0.0, 0.58, 1.0),
        Easing::EaseInBack => (0.3, -0.05, 0.7, -0.5),
        Easing::EaseOutBack => (0.45, 1.45, 0.8, 1.0),
        Easing::EaseInOutBack => (0.7, -0.4, 0.4, 1.4),
        _ => return None,
    })
}

/// Ready-made animations (the incumbent's first-party "animation styles"): each writes keyframes
/// onto a layer, relative to its resting value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Position: slide in from / out to a side, `amount` pixels away (default 200).
    Position,
    /// Scale: scale in from / out to `amount` percent (default 120).
    Scale,
    /// Rotation: rotate in from / out by `amount` degrees (default 180), clockwise by default.
    Rotation,
    /// Size: resize in from / out to the size plus `amount` pixels (default 50).
    Size,
    /// Opacity: fade in from / out to `amount` percent (default 0).
    Opacity,
}

/// A ready-made animation's length when none is given (seconds).
pub const PRESET_DURATION: f64 = 0.5;

/// Where a Position slide comes from or goes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Side {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "left" => Side::Left,
            "right" => Side::Right,
            "top" | "up" => Side::Top,
            "bottom" | "down" => Side::Bottom,
            "top_left" => Side::TopLeft,
            "top_right" => Side::TopRight,
            "bottom_left" => Side::BottomLeft,
            "bottom_right" => Side::BottomRight,
            _ => return None,
        })
    }

    /// The offset `d` pixels towards this side (+x right, +y down).
    pub fn offset(self, d: f64) -> (f64, f64) {
        match self {
            Side::Left => (-d, 0.0),
            Side::Right => (d, 0.0),
            Side::Top => (0.0, -d),
            Side::Bottom => (0.0, d),
            Side::TopLeft => (-d, -d),
            Side::TopRight => (d, -d),
            Side::BottomLeft => (-d, d),
            Side::BottomRight => (d, d),
        }
    }
}

/// A ready-made animation's settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresetArgs {
    pub start: f64,
    pub duration: f64,
    pub easing: Easing,
    /// In (to the layer's rest) or out (away from it).
    pub inward: bool,
    /// Pixels, percent or degrees by preset (None: its default).
    pub amount: Option<f64>,
    /// Position: the side (default left).
    pub side: Side,
    /// Rotation: clockwise (the default).
    pub clockwise: bool,
}

impl Default for PresetArgs {
    fn default() -> Self {
        PresetArgs { start: 0.0, duration: PRESET_DURATION, easing: Easing::EaseOut, inward: true, amount: None, side: Side::Left, clockwise: true }
    }
}

impl Preset {
    pub const ALL: [Preset; 5] = [Preset::Position, Preset::Scale, Preset::Rotation, Preset::Size, Preset::Opacity];

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "position" | "slide" | "move" => Preset::Position,
            "scale" => Preset::Scale,
            "rotation" | "rotate" | "spin" => Preset::Rotation,
            "size" | "resize" => Preset::Size,
            "opacity" | "fade" => Preset::Opacity,
            _ => return None,
        })
    }

    pub fn id(self) -> &'static str {
        match self {
            Preset::Position => "position",
            Preset::Scale => "scale",
            Preset::Rotation => "rotation",
            Preset::Size => "size",
            Preset::Opacity => "opacity",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Preset::Position => "Position",
            Preset::Scale => "Scale",
            Preset::Rotation => "Rotation",
            Preset::Size => "Size",
            Preset::Opacity => "Opacity",
        }
    }

    /// The amount used when none is given: pixels, percent or degrees.
    pub fn default_amount(self) -> f64 {
        match self {
            Preset::Position => 200.0,
            Preset::Scale => 120.0,
            Preset::Rotation => 180.0,
            Preset::Size => 50.0,
            Preset::Opacity => 0.0,
        }
    }

    /// The tracks this preset writes on `n`: in, from the moved-away state to the layer's rest;
    /// out, from rest to away.
    pub fn tracks(self, n: &Node, a: &PresetArgs) -> Vec<(MotionField, Track)> {
        let start = snap_time(a.start);
        let end = snap_time(start + a.duration.clamp(TIME_STEP, MAX_TIMELINE));
        let amount = a.amount.filter(|v| v.is_finite()).unwrap_or(self.default_amount());
        let (inward, easing) = (a.inward, a.easing);
        let pair = |f: MotionField, away: f64, rest: f64| {
            let (from, to) = if inward { (away, rest) } else { (rest, away) };
            let k0 = Keyframe { time: start, value: MotionValue::Float(from), easing: Easing::Linear, hold: false };
            let k1 = Keyframe { time: end, value: MotionValue::Float(to), easing, hold: false };
            (f, Track { keyframes: vec![k0, k1] })
        };
        match self {
            Preset::Position => {
                let (dx, dy) = a.side.offset(amount);
                let mut v = Vec::new();
                if dx != 0.0 {
                    v.push(pair(MotionField::TranslationX, dx, 0.0));
                }
                if dy != 0.0 {
                    v.push(pair(MotionField::TranslationY, dy, 0.0));
                }
                v
            }
            Preset::Scale => {
                let k = amount.max(0.0) / 100.0;
                vec![pair(MotionField::ScaleX, k, 1.0), pair(MotionField::ScaleY, k, 1.0)]
            }
            Preset::Rotation => {
                // Rotation is counter-clockwise positive: turning clockwise into place starts
                // `amount` counter-clockwise of it, and turning clockwise away ends `amount`
                // clockwise of it.
                let turn = if a.clockwise { amount } else { -amount };
                vec![pair(MotionField::Rotation, if inward { turn } else { -turn }, 0.0)]
            }
            Preset::Size => vec![
                pair(MotionField::Width, (n.width + amount).max(0.01), n.width),
                pair(MotionField::Height, (n.height + amount).max(0.01), n.height),
            ],
            Preset::Opacity => {
                let rest = f64::from(n.opacity);
                vec![pair(MotionField::Opacity, (amount / 100.0).clamp(0.0, 1.0) * rest, rest)]
            }
        }
    }
}
