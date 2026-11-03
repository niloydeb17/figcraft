//! Motion: keyframes on a top-level frame's timeline, ready-made animations, the timeline's
//! length and the playhead. The rules are in `figcraft_doc::motion` (and `MECHANICS.md`).

use figcraft_doc::motion::{Keyframe, MotionField, MotionValue, PRESET_DURATION, Preset, PresetArgs, Side, Track, snap_time};
use figcraft_doc::proto::Easing;
use figcraft_doc::{Document, NodeId};
use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::{boolean, color_of, get, num, req_string, req_targets, string, targets};
use crate::{Result, Session, bad, spec};

/// The motion timeline as the tab shows it (not saved, not undoable).
#[derive(Clone, Debug, PartialEq)]
pub struct MotionPlayer {
    /// The timeline panel is open.
    pub panel: bool,
    /// Seconds from the start of the timeline the canvas shows.
    pub playhead: f64,
    /// Playing: the UI moves the playhead on with the clock.
    pub playing: bool,
    /// Play again from the start at the end (the incumbent's default playback style is Loop).
    pub looping: bool,
}

impl Default for MotionPlayer {
    fn default() -> Self {
        MotionPlayer { panel: false, playhead: 0.0, playing: false, looping: true }
    }
}

impl MotionPlayer {
    /// Whether the canvas shows the animated moment (the timeline is open, or playing).
    pub fn shows_motion(&self) -> bool {
        self.panel || self.playing
    }

    /// Move the playhead on by `dt` seconds on a `length`-second timeline: at the end it stops,
    /// or with looping starts over.
    pub fn advance(&mut self, dt: f64, length: f64) {
        if !self.playing || !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let length = if length.is_finite() && length > 0.0 { length } else { figcraft_doc::motion::DEFAULT_TIMELINE };
        self.playhead += dt.min(1.0);
        if self.playhead >= length {
            if self.looping {
                self.playhead %= length;
            } else {
                self.playhead = length;
                self.playing = false;
            }
        }
    }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "view.motionTimeline",
            "Motion timeline",
            "{show? (default: toggle)} → {show}  the timeline panel under the canvas: the selected frame's tracks and keyframes, with the canvas showing the playhead's moment",
            false,
            timeline_panel
        ),
        spec!(
            "motion.play",
            "Play/Pause animation",
            "{playing? (default: toggle), loop?} → {playing, time}  play the current frame's timeline from the playhead (from the start when it's at the end)",
            false,
            play
        ),
        spec!("motion.seek", "Move playhead", "{time (seconds)} → {time, duration}  stop and show that moment of the timeline", false, seek),
        spec!(
            "motion.keyframe.add",
            "Add keyframe",
            "{ids?, field (TRANSLATION_X|TRANSLATION_Y|ROTATION|SCALE_X|SCALE_Y|OPACITY|CORNER_RADIUS|STROKE_WEIGHT|WIDTH|HEIGHT|fills/<i>|strokes/<i>), time? (seconds; default the playhead), value? (a number, or a colour for fills/strokes; default the value shown at that time), easing? (linear|easeIn|easeOut|easeInAndOut|easeInBack|easeOutBack|easeInAndOutBack|gentle|quick|bouncy|slow|hold|{type, easingFunctionCubicBezier?, easingFunctionSpring?})} → {ids, field, time}  add or replace a keyframe on layers inside a top-level frame",
            true,
            add_keyframe
        ),
        spec!(
            "motion.keyframe.set",
            "Edit keyframe",
            "{ids?, field, time (the keyframe's position), newTime?, value?, easing?} → {ids, field, time}  move a keyframe or change its value or easing",
            true,
            set_keyframe
        ),
        spec!(
            "motion.keyframe.remove",
            "Delete keyframe",
            "{ids?, field? (default every track), time? (default the playhead)} → {ids, removed}",
            true,
            remove_keyframe
        ),
        spec!("motion.track.remove", "Remove animation track", "{ids?, field} → {ids}  remove every keyframe of one field", true, remove_track),
        spec!("motion.clear", "Remove animation", "{ids?} → {ids}  remove every keyframe from the layers", true, clear),
        spec!(
            "motion.preset",
            "Apply animation style",
            "{ids?, preset (position|scale|rotation|size|opacity), timing? (in|out; default in), start? (seconds; default the playhead), duration? (seconds; default 0.5), easing? (default easeOut), amount? (position and size: px, default 200 and 50; scale and opacity: %, default 120 and 0; rotation: degrees, default 180), direction? (position: left|right|top|bottom|top_left|top_right|bottom_left|bottom_right, default left; rotation: clockwise|counterClockwise, default clockwise)} → {ids, fields, start, end}  write a ready-made animation's keyframes",
            true,
            preset
        ),
        spec!(
            "motion.timeline.duration",
            "Set timeline length",
            "{id? (a top-level frame or a layer in one; default the selection), seconds} → {id, duration}",
            true,
            set_duration
        ),
        spec!(
            "motion.get",
            "Inspect animation",
            "{ids?} → {timeline: {id, duration} | null, layers: [{id, name, tracks: {field: [{time, value, easing, hold}]}}]}",
            false,
            inspect
        ),
    ]
}

/// The top-level frame whose timeline the tab shows: the selection's, else the page's first
/// animated one, else its first frame.
pub fn current_frame(s: &Session) -> Option<NodeId> {
    let ds = s.ds().ok()?;
    let doc = &ds.doc;
    ds.selection
        .iter()
        .find_map(|i| doc.top_level_frame(*i))
        .or_else(|| doc.animated_in(ds.page).into_iter().find_map(|i| doc.top_level_frame(i)))
        .or_else(|| doc.top_frames(ds.page).into_iter().next())
}

/// The current timeline's length (the default with no frame).
pub fn current_length(s: &Session) -> f64 {
    match (current_frame(s), s.doc()) {
        (Some(f), Ok(doc)) => doc.timeline_duration(f),
        _ => figcraft_doc::motion::DEFAULT_TIMELINE,
    }
}

fn timeline_panel(s: &mut Session, p: &Value) -> Result<Value> {
    let ds = s.ds_mut()?;
    let show = boolean(p, "show").unwrap_or(!ds.motion.panel);
    ds.motion.panel = show;
    if !show {
        ds.motion.playing = false;
    }
    Ok(json!({"show": show}))
}

fn play(s: &mut Session, p: &Value) -> Result<Value> {
    let length = current_length(s);
    let ds = s.ds_mut()?;
    let playing = boolean(p, "playing").unwrap_or(!ds.motion.playing);
    if let Some(l) = boolean(p, "loop") {
        ds.motion.looping = l;
    }
    if playing && ds.motion.playhead >= length - 1e-9 {
        ds.motion.playhead = 0.0;
    }
    ds.motion.playing = playing;
    Ok(json!({"playing": playing, "time": ds.motion.playhead}))
}

fn seek(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.seek";
    let t = num(p, "time").ok_or_else(|| bad(cmd, "`time`: seconds from the start of the timeline"))?;
    let length = current_length(s);
    let ds = s.ds_mut()?;
    ds.motion.playhead = snap_time(t).min(length);
    ds.motion.playing = false;
    Ok(json!({"time": ds.motion.playhead, "duration": length}))
}

/// Parse an easing: a name (`easeOut`, `EASE_OUT`, `ease-out`, `hold`…), a public easing object
/// (`{type: "CUSTOM_CUBIC_BEZIER", easingFunctionCubicBezier: {x1, y1, x2, y2}}`,
/// `{type: "CUSTOM_SPRING", easingFunctionSpring: {bounce}}`) or FigCraft's own form. `(easing,
/// hold)`.
pub fn easing_of(v: &Value) -> Option<(Easing, bool)> {
    let name = match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => match o.get("type").and_then(Value::as_str) {
            Some(t) => t.to_string(),
            None => return serde_json::from_value::<Easing>(v.clone()).ok().map(|e| (e, false)),
        },
        _ => return None,
    };
    let norm: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let e = match norm.as_str() {
        "hold" | "step" => return Some((Easing::Linear, true)),
        "linear" => Easing::Linear,
        "easein" => Easing::EaseIn,
        "easeout" => Easing::EaseOut,
        "easeinandout" | "easeinout" => Easing::EaseInOut,
        "easeinback" => Easing::EaseInBack,
        "easeoutback" => Easing::EaseOutBack,
        "easeinandoutback" | "easeinoutback" => Easing::EaseInOutBack,
        "gentle" => Easing::Gentle,
        "quick" => Easing::Quick,
        "bouncy" => Easing::Bouncy,
        "slow" => Easing::Slow,
        "customcubicbezier" | "custombezier" => {
            let b = v.get("easingFunctionCubicBezier").unwrap_or(v);
            let f = |k: &str, d: f64| b.get(k).and_then(Value::as_f64).filter(|x| x.is_finite()).unwrap_or(d);
            Easing::CustomBezier { x1: f("x1", 0.42).clamp(0.0, 1.0), y1: f("y1", 0.0), x2: f("x2", 0.58).clamp(0.0, 1.0), y2: f("y2", 1.0) }
        }
        "customspring" => {
            let sp = v.get("easingFunctionSpring");
            match sp.and_then(|s| s.get("bounce")).and_then(Value::as_f64).filter(|b| b.is_finite()) {
                Some(bounce) => spring_from_bounce(bounce),
                None => sp.and_then(|s| serde_json::from_value::<Easing>(json!({"customSpring": s})).ok()).unwrap_or(Easing::CUSTOM_SPRING),
            }
        }
        _ => return None,
    };
    Some((e, false))
}

/// A spring for a public `bounce` (0: settles with no overshoot, 1: the most oscillation): unit
/// mass and stiffness 100, damping from critical (20) down. Estimated mapping.
pub fn spring_from_bounce(bounce: f64) -> Easing {
    let b = bounce.clamp(0.0, 1.0);
    Easing::CustomSpring { mass: 1.0, stiffness: 100.0, damping: 20.0 * (1.0 - 0.9 * b) }
}

/// The easing's public name (`EASE_OUT`, `HOLD`, …).
pub fn easing_name(e: Easing, hold: bool) -> &'static str {
    if hold {
        return "HOLD";
    }
    match e {
        Easing::Linear => "LINEAR",
        Easing::EaseIn => "EASE_IN",
        Easing::EaseOut => "EASE_OUT",
        Easing::EaseInOut => "EASE_IN_AND_OUT",
        Easing::EaseInBack => "EASE_IN_BACK",
        Easing::EaseOutBack => "EASE_OUT_BACK",
        Easing::EaseInOutBack => "EASE_IN_AND_OUT_BACK",
        Easing::Gentle => "GENTLE",
        Easing::Quick => "QUICK",
        Easing::Bouncy => "BOUNCY",
        Easing::Slow => "SLOW",
        Easing::CustomBezier { .. } => "CUSTOM_CUBIC_BEZIER",
        Easing::CustomSpring { .. } => "CUSTOM_SPRING",
    }
}

fn field(cmd: &str, p: &Value) -> Result<MotionField> {
    let f = req_string(cmd, p, "field")?;
    MotionField::parse(&f).ok_or_else(|| bad(cmd, format!("`field`: `{f}` can't be animated (TRANSLATION_X, TRANSLATION_Y, ROTATION, SCALE_X, SCALE_Y, OPACITY, CORNER_RADIUS, STROKE_WEIGHT, WIDTH, HEIGHT, fills/<i>, strokes/<i>)")))
}

fn opt_easing(cmd: &str, p: &Value) -> Result<Option<(Easing, bool)>> {
    match get(p, "easing") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => easing_of(v).map(Some).ok_or_else(|| {
            bad(cmd, "`easing`: linear, easeIn, easeOut, easeInAndOut, the Back variants, gentle, quick, bouncy, slow, hold or a custom curve")
        }),
    }
}

fn opt_value(cmd: &str, p: &Value, f: MotionField) -> Result<Option<MotionValue>> {
    let Some(v) = get(p, "value").filter(|v| !v.is_null()) else { return Ok(None) };
    let raw = if f.is_color() {
        color_of(v.get("value").unwrap_or(v)).map(MotionValue::Color)
    } else {
        v.as_f64().or_else(|| v.get("value").and_then(Value::as_f64)).map(MotionValue::Float)
    };
    raw.and_then(|r| f.clean(r)).map(Some).ok_or_else(|| bad(cmd, if f.is_color() { "`value`: a colour" } else { "`value`: a finite number" }))
}

/// The layers given, each checked to be animatable: inside a top-level frame, not one itself.
fn animatable(cmd: &str, s: &Session, p: &Value) -> Result<Vec<NodeId>> {
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc()?;
    for i in &ids {
        match doc.top_level_frame(*i) {
            None => return Err(bad(cmd, "only layers inside a top-level frame can be animated (the frame holds the timeline)")),
            Some(f) if f == *i => return Err(bad(cmd, "top-level frames hold the timeline and aren't animated: animate the layers inside")),
            Some(_) => {}
        }
    }
    Ok(ids)
}

/// The value `id`'s field shows at `t` (its track's value, or its value at rest).
fn shown_value(doc: &Document, id: NodeId, f: MotionField, t: f64) -> Option<MotionValue> {
    let n = doc.node(id)?;
    n.motion.as_ref().and_then(|m| m.track(f)).and_then(|tr| tr.sample(t)).or_else(|| f.rest_value(n))
}

fn playhead(s: &Session) -> f64 {
    s.ds().map(|ds| ds.motion.playhead).unwrap_or(0.0)
}

/// Make the timeline of the frame holding `id` long enough for `end` (it never shortens).
fn extend_timeline(doc: &mut Document, id: NodeId, end: f64) -> Result<()> {
    let Some(frame) = doc.top_level_frame(id) else { return Ok(()) };
    let set = doc.node(frame).and_then(|n| n.motion.as_ref()).and_then(|m| m.duration);
    if let Some(d) = set
        && d < end
    {
        doc.get_mut(frame)?.motion.get_or_insert_with(Default::default).duration = Some(end.min(figcraft_doc::motion::MAX_TIMELINE));
    }
    Ok(())
}

/// Drop a layer's motion when nothing is left in it.
fn tidy(doc: &mut Document, id: NodeId) -> Result<()> {
    let n = doc.get_mut(id)?;
    if let Some(m) = n.motion.as_mut() {
        m.tracks.retain(|_, t| !t.keyframes.is_empty());
        if m.is_empty() {
            n.motion = None;
        }
    }
    Ok(())
}

fn add_keyframe(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.keyframe.add";
    let ids = animatable(cmd, s, p)?;
    let f = field(cmd, p)?;
    let t = snap_time(num(p, "time").unwrap_or_else(|| playhead(s)));
    let value = opt_value(cmd, p, f)?;
    let (easing, hold) = opt_easing(cmd, p)?.unwrap_or((Easing::EaseOut, false));
    let doc = s.doc_mut()?;
    let mut done = Vec::new();
    for id in ids {
        let Some(v) = value.or_else(|| shown_value(doc, id, f, t)) else {
            if f.is_color() {
                return Err(bad(cmd, format!("layer {id} has no solid paint at that index")));
            }
            continue;
        };
        let n = doc.get_mut(id)?;
        let track = n.motion.get_or_insert_with(Default::default).track_mut(f);
        if track.set(Keyframe { time: t, value: v, easing, hold }).is_none() {
            return Err(bad(cmd, "this track has too many keyframes"));
        }
        extend_timeline(doc, id, t)?;
        done.push(id);
    }
    Ok(json!({"ids": done, "field": f.key(), "time": t}))
}

fn set_keyframe(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.keyframe.set";
    let ids = animatable(cmd, s, p)?;
    let f = field(cmd, p)?;
    let t = snap_time(num(p, "time").ok_or_else(|| bad(cmd, "`time`: the keyframe's position (seconds)"))?);
    let new_time = num(p, "newTime").map(snap_time);
    let value = opt_value(cmd, p, f)?;
    let easing = opt_easing(cmd, p)?;
    let doc = s.doc_mut()?;
    let mut done = Vec::new();
    for id in ids {
        let n = doc.get_mut(id)?;
        let Some(track) = n.motion.as_mut().and_then(|m| m.tracks.get_mut(&f.key())) else { continue };
        let Some(i) = track.keyframes.iter().position(|k| (k.time - t).abs() < figcraft_doc::motion::TIME_STEP / 2.0) else { continue };
        let Some(mut k) = track.keyframes.get(i).copied() else { continue };
        if let Some(v) = value {
            k.value = v;
        }
        if let Some((e, h)) = easing {
            k.easing = e;
            k.hold = h;
        }
        track.keyframes.remove(i);
        if let Some(nt) = new_time {
            k.time = nt;
        }
        track.set(k);
        let end = k.time;
        extend_timeline(doc, id, end)?;
        done.push(id);
    }
    if done.is_empty() {
        return Err(bad(cmd, format!("no {} keyframe at {t} s", f.key())));
    }
    Ok(json!({"ids": done, "field": f.key(), "time": new_time.unwrap_or(t)}))
}

fn remove_keyframe(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.keyframe.remove";
    let ids = req_targets(cmd, s, p)?;
    let only = match string(p, "field") {
        Some(f) => Some(MotionField::parse(&f).ok_or_else(|| bad(cmd, format!("`field`: `{f}` can't be animated")))?),
        None => None,
    };
    let t = snap_time(num(p, "time").unwrap_or_else(|| playhead(s)));
    let doc = s.doc_mut()?;
    let mut removed = 0usize;
    let mut out = Vec::new();
    for id in ids {
        let n = doc.get_mut(id)?;
        let Some(m) = n.motion.as_mut() else { continue };
        let mut any = false;
        for (k, track) in m.tracks.iter_mut() {
            if only.is_some_and(|f| f.key() != *k) {
                continue;
            }
            if track.remove_at(t) {
                removed += 1;
                any = true;
            }
        }
        if any {
            tidy(doc, id)?;
            out.push(id);
        }
    }
    if removed == 0 {
        return Err(bad(cmd, format!("no keyframe at {t} s")));
    }
    Ok(json!({"ids": out, "removed": removed}))
}

fn remove_track(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.track.remove";
    let ids = req_targets(cmd, s, p)?;
    let f = field(cmd, p)?;
    let doc = s.doc_mut()?;
    let mut out = Vec::new();
    for id in ids {
        if doc.get_mut(id)?.motion.as_mut().and_then(|m| m.tracks.remove(&f.key())).is_some() {
            tidy(doc, id)?;
            out.push(id);
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, format!("nothing selected animates {}", f.key())));
    }
    Ok(json!({"ids": out}))
}

fn clear(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.clear";
    let ids = req_targets(cmd, s, p)?;
    let doc = s.doc_mut()?;
    let mut out = Vec::new();
    for id in ids {
        let n = doc.get_mut(id)?;
        if let Some(m) = n.motion.as_mut()
            && !m.tracks.is_empty()
        {
            m.tracks.clear();
            tidy(doc, id)?;
            out.push(id);
        }
    }
    if out.is_empty() {
        return Err(bad(cmd, "nothing selected is animated"));
    }
    Ok(json!({"ids": out}))
}

fn preset(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.preset";
    let ids = animatable(cmd, s, p)?;
    let name = req_string(cmd, p, "preset")?;
    let preset = Preset::parse(&name).ok_or_else(|| bad(cmd, format!("`preset`: `{name}` isn't one of position, scale, rotation, size, opacity")))?;
    let inward = match string(p, "timing").as_deref().map(str::to_ascii_lowercase).as_deref() {
        None | Some("in") => true,
        Some("out") => false,
        Some(other) => return Err(bad(cmd, format!("`timing`: in or out (got `{other}`)"))),
    };
    let start = snap_time(num(p, "start").unwrap_or_else(|| playhead(s)));
    let duration = num(p, "duration").filter(|d| d.is_finite() && *d > 0.0).unwrap_or(PRESET_DURATION);
    let (easing, _) = opt_easing(cmd, p)?.unwrap_or((Easing::EaseOut, false));
    let amount = num(p, "amount").filter(|a| a.is_finite());
    let direction = string(p, "direction");
    let (side, clockwise) = match (preset, direction.as_deref()) {
        (_, None) => (Side::Left, true),
        (Preset::Rotation, Some(d)) => match d.to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
            "clockwise" | "cw" => (Side::Left, true),
            "counterclockwise" | "ccw" | "anticlockwise" => (Side::Left, false),
            _ => return Err(bad(cmd, format!("`direction`: clockwise or counterClockwise (got `{d}`)"))),
        },
        (_, Some(d)) => (
            Side::parse(d).ok_or_else(|| {
                bad(cmd, format!("`direction`: left, right, top, bottom, top_left, top_right, bottom_left or bottom_right (got `{d}`)"))
            })?,
            true,
        ),
    };
    let args = PresetArgs { start, duration, easing, inward, amount, side, clockwise };
    let doc = s.doc_mut()?;
    let mut fields = Vec::new();
    let mut end = start;
    for id in &ids {
        let tracks = {
            let n = doc.node(*id).ok_or(crate::EngineError::NoNode(*id))?;
            preset.tracks(n, &args)
        };
        let n = doc.get_mut(*id)?;
        let m = n.motion.get_or_insert_with(Default::default);
        for (f, t) in tracks {
            let track: &mut Track = m.track_mut(f);
            for k in t.keyframes {
                end = end.max(k.time);
                track.set(k);
            }
            if !fields.contains(&f.key()) {
                fields.push(f.key());
            }
        }
        extend_timeline(doc, *id, end)?;
    }
    Ok(json!({"ids": ids, "fields": fields, "start": start, "end": end}))
}

fn set_duration(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "motion.timeline.duration";
    let secs = num(p, "seconds").filter(|v| v.is_finite()).ok_or_else(|| bad(cmd, "`seconds`: the timeline's length"))?;
    let secs = secs.clamp(figcraft_doc::motion::MIN_TIMELINE, figcraft_doc::motion::MAX_TIMELINE);
    let start = targets(s, p).first().copied().or_else(|| current_frame(s)).ok_or_else(|| bad(cmd, "select a top-level frame or a layer in one"))?;
    let doc = s.doc_mut()?;
    let frame = doc.top_level_frame(start).ok_or_else(|| bad(cmd, "select a top-level frame or a layer in one"))?;
    doc.get_mut(frame)?.motion.get_or_insert_with(Default::default).duration = Some(snap_time(secs).max(figcraft_doc::motion::MIN_TIMELINE));
    let length = doc.timeline_duration(frame);
    if let Ok(ds) = s.ds_mut() {
        ds.motion.playhead = ds.motion.playhead.min(length);
    }
    Ok(json!({"id": frame, "duration": length}))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p);
    let doc = s.doc()?;
    let frame = ids.iter().find_map(|i| doc.top_level_frame(*i)).or_else(|| current_frame(s));
    let ids = if ids.is_empty() { frame.map(|f| doc.animated_in(f)).unwrap_or_default() } else { ids };
    let layers: Vec<Value> = ids
        .iter()
        .filter_map(|i| doc.node(*i))
        .map(|n| {
            let tracks: serde_json::Map<String, Value> = n
                .motion
                .as_ref()
                .map(|m| {
                    m.fields()
                        .into_iter()
                        .map(|(f, t)| {
                            let keys: Vec<Value> = t
                                .keyframes
                                .iter()
                                .map(|k| json!({"time": k.time, "value": k.value, "easing": easing_name(k.easing, k.hold)}))
                                .collect();
                            (f.key(), Value::Array(keys))
                        })
                        .collect()
                })
                .unwrap_or_default();
            json!({"id": n.id, "name": n.name, "tracks": tracks})
        })
        .collect();
    let timeline = frame.map(|f| json!({"id": f, "duration": doc.timeline_duration(f)})).unwrap_or(Value::Null);
    Ok(json!({"timeline": timeline, "layers": layers}))
}
