//! Layers lifted off the canvas while they're dragged. A move that so far has only shifted the
//! selection ([`figcraft_engine::Interaction::moved_by`]) leaves the rest of the page as it was:
//! the rest is rendered once without the selection, the selection once on its own (on a worker of
//! its own), and the canvas slides that image with the pointer. The layers then keep up with their
//! outline however long the page takes to render: a section of a few thousand layers, rendered
//! anew for every step of the drag, trailed its outline by several frames.
//!
//! Layers drawn above the selection would end up under it, so a drag whose layers pass under
//! others is rendered as before.

use std::sync::Arc;

use egui::{Color32, Pos2, Rect, pos2};
use figcraft_doc::{Document, NodeId};
use figcraft_geom::Rect as PageRect;
use figcraft_render::View;

use super::presented_image_rect;
use crate::FigApp;
use crate::render_worker::{Job, Worker};

/// A lifted image reaches this far past the view (in views) each way, so a drag can go on a while
/// before the image has to be rendered again.
const REACH: f64 = 0.5;

#[derive(Default)]
pub(crate) struct Lift {
    /// Renders the lifted layers; kept from one drag to the next.
    worker: Option<Worker>,
    drag: Option<Drag>,
}

struct Drag {
    /// The document the drag started from, and the layers it moves.
    doc: Arc<Document>,
    ids: Vec<NodeId>,
    uid: u64,
    page: NodeId,
    /// What the layers can paint, and what layers drawn above them can, at the start.
    bounds: PageRect,
    above: Vec<PageRect>,
    /// How far the layers have moved (page units); kept once the drag is over until the canvas
    /// shows its result.
    offset: (f64, f64),
    /// The drag is lifted this frame.
    active: bool,
    /// The area of the last image asked for, and of the one shown.
    asked: Option<View>,
    texture: Option<egui::TextureHandle>,
    shown: Option<View>,
}

impl Lift {
    /// Whether the canvas image shows the page under a lifted drag (rendered without its layers):
    /// the lifted image goes over it until the canvas has an image with them.
    fn showing(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.active || d.texture.is_some())
    }
}

/// The layers lifted this frame and the document the rest of the page is rendered from, when a
/// drag only moves the selection over layers drawn below it.
pub(super) fn update(app: &mut FigApp) -> Option<(Arc<Document>, Vec<NodeId>)> {
    let now = lifted(app);
    let lift = &mut app.canvas.lift;
    match (now, lift.drag.as_mut()) {
        (Some((doc, ids, uid, page, offset)), Some(d)) if Arc::ptr_eq(&d.doc, &doc) && d.ids == ids && d.uid == uid && d.page == page => {
            d.offset = offset;
            d.active = covered(d, offset).is_none_or(|c| !c);
            d.active.then(|| (d.doc.clone(), d.ids.clone()))
        }
        (Some((doc, ids, uid, page, offset)), _) => {
            let Some(bounds) = paint_bounds(&doc, &ids) else {
                lift.drag = None;
                return None;
            };
            let above = above(&doc, page, &ids);
            let mut d = Drag { doc, ids, uid, page, bounds, above, offset, active: true, asked: None, texture: None, shown: None };
            d.active = covered(&d, offset).is_none_or(|c| !c);
            let out = d.active.then(|| (d.doc.clone(), d.ids.clone()));
            lift.drag = Some(d);
            out
        }
        (None, Some(d)) => {
            // Over (or no longer a plain move): the layers stay where the drag left them until
            // the canvas has them, unless it was cancelled back to where it started.
            if d.active
                && let Ok(ds) = app.session.ds()
                && Arc::ptr_eq(&ds.doc, &d.doc)
            {
                d.offset = (0.0, 0.0);
            }
            d.active = false;
            None
        }
        (None, None) => None,
    }
}

/// A drag in progress: the document it started from, its layers, the file and page, how far
/// they've moved.
type Lifted = (Arc<Document>, Vec<NodeId>, u64, NodeId, (f64, f64));

/// The drag this frame.
fn lifted(app: &FigApp) -> Option<Lifted> {
    let ds = app.session.ds().ok()?;
    let it = ds.interaction.as_ref()?;
    let (dx, dy) = it.moved_by?;
    let usable = dx.is_finite() && dy.is_finite() && !it.selection.is_empty() && it.selection == ds.selection;
    // Pixel preview's magnified pixels and live text drawn from another document stay as they are.
    if !usable || ds.view.pixel_preview_scale().is_some() || app.preedit_view().is_some() {
        return None;
    }
    Some((it.doc.clone(), it.selection.clone(), ds.uid, ds.page, (dx, dy)))
}

fn paint_bounds(doc: &Document, ids: &[NodeId]) -> Option<PageRect> {
    ids.iter().filter_map(|i| figcraft_render::draw::paint_bounds(doc, *i)).reduce(|a, b| a.union(b))
}

/// What layers drawn above `ids` can paint: those after each of them (and after each of their
/// ancestors) in their parent, other than the moved layers themselves.
fn above(doc: &Document, page: NodeId, ids: &[NodeId]) -> Vec<PageRect> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for id in ids {
        let mut at = *id;
        while at != page {
            let Some(parent) = doc.node(at).and_then(|n| n.parent) else { break };
            let Some(p) = doc.node(parent) else { break };
            for c in p.children.iter().skip_while(|c| **c != at).skip(1) {
                if ids.contains(c) || seen.contains(c) {
                    continue;
                }
                seen.push(*c);
                if doc.node(*c).is_some_and(|n| n.visible) {
                    out.extend(figcraft_render::draw::paint_bounds(doc, *c));
                }
            }
            at = parent;
        }
    }
    out
}

/// Whether a layer drawn above the moved ones overlaps them where they were or are now; `None`
/// when that can't be told.
fn covered(d: &Drag, (dx, dy): (f64, f64)) -> Option<bool> {
    let b = d.bounds;
    let now = PageRect::new(b.x0 + dx, b.y0 + dy, b.x1 + dx, b.y1 + dy);
    if !(now.x0.is_finite() && now.y0.is_finite() && now.x1.is_finite() && now.y1.is_finite()) {
        return None;
    }
    let hits = |a: &PageRect, r: &PageRect| a.x0 < r.x1 && r.x0 < a.x1 && a.y0 < r.y1 && r.y0 < a.y1;
    Some(d.above.iter().any(|a| hits(a, &b) || hits(a, &now)))
}

/// Ask for the lifted layers' image when the one asked for doesn't cover what the view needs,
/// and take a finished one.
pub(super) fn render(app: &mut FigApp, ctx: &egui::Context, ppp: f32, max_texture_side: u32) {
    let Ok(ds) = app.session.ds() else { return };
    let (zoom, origin) = (ds.view.zoom, ds.view.origin());
    let rect = app.canvas.rect;
    let skipped = app.canvas.tex_skipped;
    let lift = &mut app.canvas.lift;
    if !lift.drag.as_ref().is_some_and(|d| d.active || skipped) {
        // The canvas shows the layers again: let the image go.
        lift.drag = None;
        return;
    }
    let worker = lift.worker.get_or_insert_with(|| Worker::new(ctx.clone()));
    let Some(d) = lift.drag.as_mut() else { return };
    if let Some(done) = worker.poll()
        && !done.coarse
        && d.asked.as_ref().is_some_and(|a| a.scale.to_bits() == done.view.scale.to_bits())
    {
        match &mut d.texture {
            Some(t) => t.set(done.image, egui::TextureOptions::LINEAR),
            None => d.texture = Some(ctx.load_texture("figcraft-lifted", done.image, egui::TextureOptions::LINEAR)),
        }
        d.shown = Some(done.view);
    }
    if !d.active {
        return;
    }
    let scale = zoom * f64::from(ppp);
    if !(scale.is_finite() && scale > 0.0 && zoom > 0.0) {
        return;
    }
    // The view, in page units, back where the layers were: the part of them it shows.
    let (vw, vh) = (f64::from(rect.width()) / zoom, f64::from(rect.height()) / zoom);
    let (x0, y0) = (origin.0 - d.offset.0, origin.1 - d.offset.1);
    let needed = PageRect::new(x0, y0, x0 + vw, y0 + vh).intersect(d.bounds);
    if needed.width() <= 0.0 || needed.height() <= 0.0 {
        return;
    }
    let covers = |v: &View| {
        let r = v.page_rect();
        let eps = 1.0 / scale;
        v.scale.to_bits() == scale.to_bits()
            && r.x0 <= needed.x0 + eps
            && r.y0 <= needed.y0 + eps
            && r.x1 >= needed.x1 - eps
            && r.y1 >= needed.y1 - eps
    };
    if d.asked.as_ref().is_some_and(covers) {
        return;
    }
    // Room to drag on, within the texture size.
    let side = f64::from(max_texture_side) / scale;
    let reach_x = (vw * REACH).min(((side - needed.width()) / 2.0).max(0.0));
    let reach_y = (vh * REACH).min(((side - needed.height()) / 2.0).max(0.0));
    let area = needed.inflate(reach_x, reach_y).intersect(d.bounds);
    let (w, h) = ((area.width() * scale).ceil(), (area.height() * scale).ceil());
    if !(w.is_finite() && h.is_finite() && w >= 1.0 && h >= 1.0) {
        return;
    }
    let view = View { origin: (area.x0, area.y0), scale, width: (w as u32).min(max_texture_side), height: (h as u32).min(max_texture_side) };
    worker.submit(Job {
        doc: d.doc.clone(),
        page: d.page,
        view: view.clone(),
        outline: ds.view.outlines,
        outline_extras: figcraft_render::Outline { hidden: ds.view.outline_hidden, bounds: ds.view.outline_bounds, light: false },
        outlined: if ds.view.outlines_selection { d.ids.clone() } else { vec![] },
        skip: vec![],
        solo: None,
        lifted: d.ids.clone(),
        proof: ds.view.proof_srgb && !d.doc.color_profile.is_srgb(),
        dots: None,
        uid: d.uid,
        zoom_preview: false,
        at: web_time::Instant::now(),
    });
    d.asked = Some(view);
}

/// Draw the lifted layers where the drag has them, over the canvas image of the rest.
pub(super) fn paint(app: &FigApp, painter: &egui::Painter, ppp: f32, shown: Option<(u64, NodeId)>) {
    let lift = &app.canvas.lift;
    if !lift.showing() || !(lift.drag.as_ref().is_some_and(|d| d.active) || app.canvas.tex_skipped) {
        return;
    }
    let Some(d) = lift.drag.as_ref() else { return };
    let (Some(tex), Some(v)) = (&d.texture, &d.shown) else { return };
    if shown != Some((d.uid, d.page)) {
        return;
    }
    let zoom = app.session.ds().map(|ds| ds.view.zoom).unwrap_or(1.0);
    let tl: Pos2 = app.to_screen(v.origin.0 + d.offset.0, v.origin.1 + d.offset.1);
    let r = presented_image_rect(tl, v, zoom, ppp, false);
    painter.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

/// Whether a drag is lifted this frame (tests).
#[cfg(test)]
pub(crate) fn active(app: &FigApp) -> bool {
    app.canvas.lift.drag.as_ref().is_some_and(|d| d.active)
}

/// Whether the lifted layers' image has arrived (tests).
#[cfg(test)]
pub(crate) fn has_image(app: &FigApp) -> bool {
    app.canvas.lift.drag.as_ref().is_some_and(|d| d.texture.is_some())
}

/// Whether anything of a drag is kept (tests).
#[cfg(test)]
pub(crate) fn kept(app: &FigApp) -> bool {
    app.canvas.lift.drag.is_some()
}
