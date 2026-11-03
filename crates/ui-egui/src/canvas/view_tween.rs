//! Discrete zooms (zoom in and out, to fit, to the selection, to a level, a zoom tool click) ease
//! from the view they leave to the one they reach, as UI3's do. Wheel, pinch, scroll and pan stay
//! direct ([`super::CanvasState`]'s `view_direct`), and so does everything with Reduce motion on.

use figcraft_doc::NodeId;
use figcraft_engine::ViewState;

use crate::FigApp;

/// How long a zoom eases, in seconds.
const DURATION: f64 = 0.28;

/// The part of a view a zoom moves: the page point at the centre and the zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Look {
    pub cx: f64,
    pub cy: f64,
    pub zoom: f64,
}

impl Look {
    fn of(v: &ViewState) -> Self {
        Look { cx: v.cx, cy: v.cy, zoom: v.zoom }
    }

    fn apply(self, v: &mut ViewState) {
        (v.cx, v.cy, v.zoom) = (self.cx, self.cy, self.zoom);
    }

    /// The look `e` (0 to 1) of the way from `self` to `to`: the zoom changes by equal ratios,
    /// about the one page point both looks show at the same place on screen, so a zoom about the
    /// pointer keeps the pointer's point still and a zoom to fit glides to its target.
    pub(crate) fn towards(self, to: Look, e: f64) -> Look {
        let zoom = self.zoom * (to.zoom / self.zoom).powf(e);
        let d = self.zoom - to.zoom;
        let axis = |a: f64, b: f64| {
            let fixed = (a * self.zoom - b * to.zoom) / d;
            fixed - (fixed - a) * self.zoom / zoom
        };
        let look = Look { cx: axis(self.cx, to.cx), cy: axis(self.cy, to.cy), zoom };
        if [look.cx, look.cy, look.zoom].iter().all(|v| v.is_finite()) { look } else { to }
    }
}

/// A zoom on its way.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tween {
    from: Look,
    to: Look,
    t0: f64,
}

/// Ease out (cubic): quick at first, settling into place.
fn ease(p: f64) -> f64 {
    1.0 - (1.0 - p).powi(3)
}

/// At the start of a canvas pass (time `now`): a zoom something made since the last pass (a
/// command, a menu, a shortcut) starts easing from where the view was; one on its way takes its
/// next step. Returns whether one is still on its way (draw again).
pub(crate) fn step(app: &mut FigApp, now: f64) -> bool {
    let direct = std::mem::take(&mut app.canvas.view_direct) || app.control_synthetic || !app.canvas.smooth_zoom || app.reduce_motion();
    let mut tween = app.canvas.tween.take();
    let last = app.canvas.last_view.take();
    let Ok(ds) = app.session.ds_mut() else { return false };
    let key: (u64, NodeId) = (ds.uid, ds.page);
    let now_look = Look::of(&ds.view);
    // The look the last pass left, on this page (another file or page shows its own view at once).
    let Some(prev) = last.filter(|(k, _)| *k == key).map(|(_, l)| l) else {
        app.canvas.last_view = Some((key, now_look));
        return false;
    };
    if now_look != prev {
        let zoomed = now_look.zoom > 0.0 && prev.zoom > 0.0 && (now_look.zoom / prev.zoom).ln().abs() > 1e-3;
        tween = (!direct && zoomed).then_some(Tween { from: prev, to: now_look, t0: now });
    }
    let mut going = false;
    if let Some(t) = tween {
        let p = if now.is_finite() && now >= t.t0 { ((now - t.t0) / DURATION).min(1.0) } else { 1.0 };
        let look = if p >= 1.0 { t.to } else { t.from.towards(t.to, ease(p)) };
        if look != now_look {
            look.apply(&mut ds.view);
            ds.revision += 1;
        }
        going = p < 1.0;
        app.canvas.tween = going.then_some(t);
    }
    app.canvas.last_view = Some((key, Look::of(&ds.view)));
    going
}
