//! FigCraft's ready-made animations: twelve that bring a layer in (Enter), twelve that move it and
//! bring it back to rest so they can repeat (Cycle) and twelve that take it away (Exit).
//!
//! The names are the ones the incumbent's Add animation picker shows (a public listing); what each
//! does is FigCraft's own design, worked out from what its name says (Slam drops hard and squashes,
//! Orbit circles, Breathe swells slowly), not taken from the incumbent's library. Keyframes are
//! written here relative to the layer's resting state (offsets in pixels and degrees, scale as a
//! factor of 1, opacity as a fraction of the layer's own) and laid on its tracks from the playhead
//! by `motion.style`. Several fields
//! moving on different curves at once is what gives each its character.

use super::{Keyframe, MotionField, MotionValue, Track, snap_time};
use crate::Node;
use crate::proto::Easing;

/// Which part of a layer's life an animation is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Enter,
    Cycle,
    Exit,
}

impl Group {
    pub const ALL: [Group; 3] = [Group::Enter, Group::Cycle, Group::Exit];

    pub fn id(self) -> &'static str {
        match self {
            Group::Enter => "enter",
            Group::Cycle => "cycle",
            Group::Exit => "exit",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::Enter => "Enter",
            Group::Cycle => "Cycle",
            Group::Exit => "Exit",
        }
    }
}

/// A ready-made animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub id: &'static str,
    pub name: &'static str,
    pub group: Group,
    /// A sentence on what it does.
    pub about: &'static str,
}

const fn style(id: &'static str, name: &'static str, group: Group, about: &'static str) -> Style {
    Style { id, name, group, about }
}

/// Every ready-made animation, Enter first, then Cycle, then Exit.
pub const STYLES: [Style; 36] = [
    style("burst", "Burst", Group::Enter, "Bursts out from nothing past full size, then settles"),
    style("ignite", "Ignite", Group::Enter, "Flickers on like a struck match as it grows into place"),
    style("sweep", "Sweep", Group::Enter, "Sweeps in from the left, tilted, and levels out"),
    style("slam", "Slam", Group::Enter, "Drops hard from above and squashes on landing before it steadies"),
    style("descend", "Descend", Group::Enter, "Comes down gently from above as it fades in"),
    style("magnify", "Magnify", Group::Enter, "Grows from small to full size as it fades in"),
    style("glide", "Glide", Group::Enter, "Glides in smoothly from the right"),
    style("drop", "Drop", Group::Enter, "Falls from above and bounces before it rests"),
    style("reveal", "Reveal", Group::Enter, "Opens downwards from a thin line"),
    style("rise", "Rise", Group::Enter, "Rises up into place as it fades in"),
    style("tip", "Tip", Group::Enter, "Tips upright into place, overshooting a little"),
    style("feather", "Feather", Group::Enter, "Floats down and fades in slowly, like a feather"),
    style("crank", "Crank", Group::Cycle, "Turns round in four quarter-turn steps, pausing at each"),
    style("press", "Press", Group::Cycle, "Pushes in like a button, then springs back"),
    style("pop", "Pop", Group::Cycle, "Swells quickly and settles back"),
    style("scoot", "Scoot", Group::Cycle, "Scoots over to the right, waits, and slides back"),
    style("spin", "Spin", Group::Cycle, "Spins once all the way round"),
    style("flip", "Flip", Group::Cycle, "Flips over side to side and back"),
    style("tick", "Tick", Group::Cycle, "Ticks a few degrees over and back, like a clock hand"),
    style("creep", "Creep", Group::Cycle, "Creeps forward in two slow steps, then slides back"),
    style("float", "Float", Group::Cycle, "Bobs gently up and down, leaning a little"),
    style("orbit", "Orbit", Group::Cycle, "Circles round a point just above it"),
    style("breathe", "Breathe", Group::Cycle, "Swells a little and shrinks back slowly"),
    style("pulse", "Pulse", Group::Cycle, "Beats outwards and back, dimming a touch"),
    style("launch", "Launch", Group::Exit, "Crouches, then shoots up out of sight"),
    style("vanish", "Vanish", Group::Exit, "Shrinks to nothing and is gone"),
    style("flick", "Flick", Group::Exit, "Pulls back, then is flicked away to the right, turning"),
    style("swipe", "Swipe", Group::Exit, "Is swiped off to the left"),
    style("poof", "Poof", Group::Exit, "Puffs out larger as it disappears"),
    style("deflate", "Deflate", Group::Exit, "Sags and spreads, then shrinks away like a balloon"),
    style("fling", "Fling", Group::Exit, "Is thrown up and over to the right, spinning"),
    style("splat", "Splat", Group::Exit, "Hops, lands and flattens wide as it fades"),
    style("evaporate", "Evaporate", Group::Exit, "Drifts up and fades away like steam"),
    style("recede", "Recede", Group::Exit, "Shrinks into the distance as it fades"),
    style("sink", "Sink", Group::Exit, "Sinks down and fades away"),
    style("tumble", "Tumble", Group::Exit, "Tumbles over and falls away"),
];

/// The style called `id` (or named `id`, in any case).
pub fn find(id: &str) -> Option<Style> {
    let want = id.trim().to_ascii_lowercase();
    let want_id = want.replace([' ', '_'], "-");
    STYLES.iter().copied().find(|s| s.id == want_id || s.name.to_ascii_lowercase() == want)
}

/// How a style reads by its length: under half a second "Fast", up to a second "Medium", "Slow"
/// after that.
pub fn speed(duration: f64) -> &'static str {
    if duration <= 0.45 {
        "Fast"
    } else if duration <= 1.0 {
        "Medium"
    } else {
        "Slow"
    }
}

// Curves (CSS-style cubic béziers), chosen for these animations.
const OUT: Easing = Easing::CustomBezier { x1: 0.22, y1: 1.0, x2: 0.36, y2: 1.0 };
const OUT_SOFT: Easing = Easing::CustomBezier { x1: 0.25, y1: 0.46, x2: 0.45, y2: 0.94 };
const IN: Easing = Easing::CustomBezier { x1: 0.55, y1: 0.06, x2: 0.68, y2: 0.19 };
const IN_HARD: Easing = Easing::CustomBezier { x1: 0.6, y1: 0.04, x2: 0.98, y2: 0.34 };
const IN_OUT: Easing = Easing::CustomBezier { x1: 0.45, y1: 0.0, x2: 0.55, y2: 1.0 };
const OVERSHOOT: Easing = Easing::CustomBezier { x1: 0.34, y1: 1.4, x2: 0.64, y2: 1.0 };
const LINEAR: Easing = Easing::Linear;

/// One field's keyframes: (seconds from the start, value relative to rest, the curve into it).
type Keys = Vec<(f64, f64, Easing)>;

/// The keyframes of style `id`, field by field, relative to rest: position and rotation as
/// offsets (0 at rest), scale as a factor and opacity as a fraction of the layer's (1 at rest).
fn keys(id: &str) -> Vec<(MotionField, Keys)> {
    use MotionField::{Opacity as O, Rotation as R, ScaleX as SX, ScaleY as SY, TranslationX as X, TranslationY as Y};
    let both = |k: Keys| vec![(SX, k.clone()), (SY, k)];
    let fade_in = |end: f64| (O, vec![(0.0, 0.0, LINEAR), (end, 1.0, OUT_SOFT)]);
    let fade_out = |from: f64, end: f64| (O, vec![(0.0, 1.0, LINEAR), (from, 1.0, LINEAR), (end, 0.0, IN)]);
    let mut v: Vec<(MotionField, Keys)> = match id {
        // Enter: each ends at rest.
        "burst" => {
            let mut v = both(vec![(0.0, 0.0, LINEAR), (0.18, 1.2, OUT), (0.3, 0.95, IN_OUT), (0.4, 1.0, IN_OUT)]);
            v.push(fade_in(0.12));
            v
        }
        "ignite" => {
            let mut v = both(vec![(0.0, 0.9, LINEAR), (0.35, 1.0, OUT)]);
            v.push((O, vec![(0.0, 0.0, LINEAR), (0.06, 0.6, LINEAR), (0.12, 0.2, LINEAR), (0.25, 1.0, OUT_SOFT)]));
            v
        }
        "sweep" => vec![fade_in(0.25), (X, vec![(0.0, -160.0, LINEAR), (0.5, 0.0, OUT)]), (R, vec![(0.0, 12.0, LINEAR), (0.5, 0.0, OUT)])],
        "slam" => vec![
            (O, vec![(0.0, 0.0, LINEAR), (0.08, 1.0, LINEAR)]),
            (Y, vec![(0.0, -200.0, LINEAR), (0.25, 0.0, IN_HARD)]),
            (SX, vec![(0.0, 1.0, LINEAR), (0.25, 1.0, LINEAR), (0.32, 1.15, OUT_SOFT), (0.42, 0.97, IN_OUT), (0.5, 1.0, IN_OUT)]),
            (SY, vec![(0.0, 1.0, LINEAR), (0.25, 1.0, LINEAR), (0.32, 0.8, OUT_SOFT), (0.42, 1.05, IN_OUT), (0.5, 1.0, IN_OUT)]),
        ],
        "descend" => vec![fade_in(0.5), (Y, vec![(0.0, -60.0, LINEAR), (0.7, 0.0, OUT)])],
        "magnify" => {
            let mut v = both(vec![(0.0, 0.3, LINEAR), (0.6, 1.0, OUT)]);
            v.push(fade_in(0.4));
            v
        }
        "glide" => vec![fade_in(0.6), (X, vec![(0.0, 80.0, LINEAR), (0.8, 0.0, OUT)])],
        "drop" => vec![
            (O, vec![(0.0, 0.0, LINEAR), (0.1, 1.0, LINEAR)]),
            (Y, vec![(0.0, -160.0, LINEAR), (0.35, 0.0, IN), (0.5, -24.0, OUT_SOFT), (0.62, 0.0, IN), (0.7, -6.0, OUT_SOFT), (0.78, 0.0, IN)]),
        ],
        "reveal" => vec![(SY, vec![(0.0, 0.0, LINEAR), (0.45, 1.0, OUT)]), fade_in(0.3)],
        "rise" => vec![fade_in(0.4), (Y, vec![(0.0, 40.0, LINEAR), (0.6, 0.0, OUT)])],
        "tip" => vec![(R, vec![(0.0, 45.0, LINEAR), (0.5, 0.0, OVERSHOOT)]), fade_in(0.2)],
        "feather" => vec![
            (O, vec![(0.0, 0.0, LINEAR), (1.2, 1.0, IN_OUT)]),
            (Y, vec![(0.0, -12.0, LINEAR), (1.2, 0.0, OUT_SOFT)]),
            (R, vec![(0.0, 4.0, LINEAR), (0.6, -2.0, IN_OUT), (1.2, 0.0, IN_OUT)]),
        ],
        // Cycle: each starts and ends at rest (a whole turn counts as rest).
        "crank" => vec![(
            R,
            vec![
                (0.0, 0.0, LINEAR),
                (0.25, -90.0, IN_OUT),
                (0.4, -90.0, LINEAR),
                (0.65, -180.0, IN_OUT),
                (0.8, -180.0, LINEAR),
                (1.05, -270.0, IN_OUT),
                (1.2, -270.0, LINEAR),
                (1.45, -360.0, IN_OUT),
            ],
        )],
        "press" => both(vec![(0.0, 1.0, LINEAR), (0.15, 0.9, OUT_SOFT), (0.4, 1.0, OVERSHOOT)]),
        "pop" => both(vec![(0.0, 1.0, LINEAR), (0.12, 1.2, OUT), (0.35, 1.0, IN_OUT)]),
        "scoot" => vec![(X, vec![(0.0, 0.0, LINEAR), (0.2, 16.0, OUT), (0.5, 16.0, LINEAR), (0.7, 0.0, IN_OUT)])],
        "spin" => vec![(R, vec![(0.0, 0.0, LINEAR), (0.8, -360.0, IN_OUT)])],
        "flip" => vec![(SX, vec![(0.0, 1.0, LINEAR), (0.3, -1.0, IN_OUT), (0.6, 1.0, IN_OUT)])],
        "tick" => vec![(R, vec![(0.0, 0.0, LINEAR), (0.06, -8.0, OUT), (0.5, -8.0, LINEAR), (0.56, 0.0, OUT), (1.0, 0.0, LINEAR)])],
        "creep" => vec![(X, vec![(0.0, 0.0, LINEAR), (0.5, 8.0, IN_OUT), (1.0, 16.0, IN_OUT), (1.6, 0.0, IN_OUT)])],
        "float" => vec![
            (Y, vec![(0.0, 0.0, LINEAR), (1.0, -8.0, IN_OUT), (2.0, 0.0, IN_OUT)]),
            (R, vec![(0.0, 0.0, LINEAR), (1.0, 2.0, IN_OUT), (2.0, 0.0, IN_OUT)]),
        ],
        "orbit" => vec![
            (X, vec![(0.0, 0.0, LINEAR), (0.4, 10.0, OUT_SOFT), (0.8, 0.0, IN_OUT), (1.2, -10.0, OUT_SOFT), (1.6, 0.0, IN_OUT)]),
            (Y, vec![(0.0, 0.0, LINEAR), (0.4, -10.0, IN_OUT), (0.8, -20.0, OUT_SOFT), (1.2, -10.0, IN_OUT), (1.6, 0.0, OUT_SOFT)]),
        ],
        "breathe" => both(vec![(0.0, 1.0, LINEAR), (1.2, 1.06, IN_OUT), (2.4, 1.0, IN_OUT)]),
        "pulse" => {
            let mut v = both(vec![(0.0, 1.0, LINEAR), (0.3, 1.1, OUT_SOFT), (0.6, 1.0, IN_OUT)]);
            v.push((O, vec![(0.0, 1.0, LINEAR), (0.3, 0.8, OUT_SOFT), (0.6, 1.0, IN_OUT)]));
            v
        }
        // Exit: each starts at rest.
        "launch" => vec![
            (Y, vec![(0.0, 0.0, LINEAR), (0.12, 10.0, OUT_SOFT), (0.5, -400.0, IN_HARD)]),
            (SY, vec![(0.0, 1.0, LINEAR), (0.12, 0.9, OUT_SOFT), (0.3, 1.1, IN_OUT)]),
            fade_out(0.3, 0.5),
        ],
        "vanish" => {
            let mut v = both(vec![(0.0, 1.0, LINEAR), (0.3, 0.0, IN)]);
            v.push(fade_out(0.0, 0.25));
            v
        }
        "flick" => vec![
            (X, vec![(0.0, 0.0, LINEAR), (0.1, -10.0, OUT_SOFT), (0.35, 300.0, IN_HARD)]),
            (R, vec![(0.0, 0.0, LINEAR), (0.35, -30.0, IN)]),
            fade_out(0.2, 0.35),
        ],
        "swipe" => vec![(X, vec![(0.0, 0.0, LINEAR), (0.4, -300.0, IN)]), fade_out(0.2, 0.4)],
        "poof" => {
            let mut v = both(vec![(0.0, 1.0, LINEAR), (0.25, 1.25, OUT)]);
            v.push(fade_out(0.0, 0.25));
            v
        }
        "deflate" => vec![
            (SX, vec![(0.0, 1.0, LINEAR), (0.4, 1.1, IN_OUT), (0.6, 0.0, IN)]),
            (SY, vec![(0.0, 1.0, LINEAR), (0.4, 0.6, IN_OUT), (0.6, 0.0, IN)]),
            fade_out(0.4, 0.6),
        ],
        "fling" => vec![
            (X, vec![(0.0, 0.0, LINEAR), (0.45, 400.0, IN_HARD)]),
            (Y, vec![(0.0, 0.0, LINEAR), (0.2, -60.0, OUT_SOFT), (0.45, 120.0, IN)]),
            (R, vec![(0.0, 0.0, LINEAR), (0.45, -90.0, IN)]),
            fade_out(0.3, 0.45),
        ],
        "splat" => vec![
            (Y, vec![(0.0, 0.0, LINEAR), (0.12, -20.0, OUT_SOFT), (0.3, 0.0, IN)]),
            (SX, vec![(0.0, 1.0, LINEAR), (0.12, 0.95, OUT_SOFT), (0.3, 1.0, IN), (0.4, 1.5, OUT)]),
            (SY, vec![(0.0, 1.0, LINEAR), (0.12, 1.1, OUT_SOFT), (0.3, 1.0, IN), (0.4, 0.3, OUT)]),
            fade_out(0.4, 0.6),
        ],
        "evaporate" => {
            let mut v = both(vec![(0.0, 1.0, LINEAR), (1.0, 1.05, OUT_SOFT)]);
            v.push((Y, vec![(0.0, 0.0, LINEAR), (1.0, -40.0, OUT_SOFT)]));
            v.push((O, vec![(0.0, 1.0, LINEAR), (1.0, 0.0, IN_OUT)]));
            v
        }
        "recede" => {
            let mut v = both(vec![(0.0, 1.0, LINEAR), (0.6, 0.7, IN_OUT)]);
            v.push((O, vec![(0.0, 1.0, LINEAR), (0.6, 0.0, IN_OUT)]));
            v
        }
        "sink" => vec![(Y, vec![(0.0, 0.0, LINEAR), (0.6, 60.0, IN)]), fade_out(0.2, 0.6)],
        "tumble" => vec![
            (R, vec![(0.0, 0.0, LINEAR), (0.7, -180.0, IN)]),
            (Y, vec![(0.0, 0.0, LINEAR), (0.7, 200.0, IN_HARD)]),
            (X, vec![(0.0, 0.0, LINEAR), (0.7, 40.0, LINEAR)]),
            fade_out(0.4, 0.7),
        ],
        _ => Vec::new(),
    };
    v.retain(|(_, k)| !k.is_empty());
    v
}

impl Style {
    /// How long it runs (seconds).
    pub fn duration(&self) -> f64 {
        keys(self.id).iter().flat_map(|(_, k)| k.iter().map(|(t, ..)| *t)).fold(0.0, f64::max)
    }

    /// Its tracks on `n`, starting at `start` seconds. Offsets (position, rotation: counter-clockwise
    /// positive) and scale factors are the tracks' own values; opacity is a fraction of the
    /// layer's.
    pub fn tracks(&self, n: &Node, start: f64) -> Vec<(MotionField, Track)> {
        let rest_opacity = f64::from(n.opacity);
        keys(self.id)
            .into_iter()
            .map(|(f, ks)| {
                let keyframes = ks
                    .into_iter()
                    .enumerate()
                    .map(|(i, (t, v, easing))| {
                        let value = if f == MotionField::Opacity { v * rest_opacity } else { v };
                        let easing = if i == 0 { Easing::Linear } else { easing };
                        Keyframe { time: snap_time(start + t), value: MotionValue::Float(value), easing, hold: false }
                    })
                    .collect();
                (f, Track { keyframes })
            })
            .collect()
    }
}
