//! The pointer on the canvas: each engine cursor ([`figcraft_engine::tools::Cursor`]) as one of
//! the system's cursors, or as a small glyph FigCraft draws at the pointer with the system's
//! hidden (a rotate cursor turned to its corner, the pen's states, the eyedropper, the comment
//! pin). The glyphs are original, drawn from lines and the code-drawn icons: no cursor images.

use egui::{Color32, CursorIcon, Painter, Pos2, Shape, Stroke, pos2, vec2};
use figcraft_engine::tools::{Cursor, PenCursor};

use crate::icons::{self, Icon};

/// A cursor FigCraft draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Glyph {
    /// A curved double arrow about the direction (degrees, 0 = right, 90 = down) from the box's
    /// centre to the corner.
    Rotate(f32),
    Pen(PenCursor),
    Eyedropper,
    Comment,
}

/// What the pointer shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shown {
    System(CursorIcon),
    Drawn(Glyph),
}

/// The system's two-headed arrow nearest a resize along `degrees` (0 = horizontal, 90 = vertical;
/// screen angles, so 45 runs from the top left to the bottom right).
pub fn resize_icon(degrees: f64) -> CursorIcon {
    let a = degrees.rem_euclid(180.0);
    if !(22.5..157.5).contains(&a) {
        CursorIcon::ResizeHorizontal
    } else if a < 67.5 {
        CursorIcon::ResizeNwSe
    } else if a < 112.5 {
        CursorIcon::ResizeVertical
    } else {
        CursorIcon::ResizeNeSw
    }
}

/// What shows for the engine's `cursor`.
pub fn of(cursor: Cursor) -> Shown {
    use Shown::{Drawn, System};
    match cursor {
        Cursor::Default => System(CursorIcon::Default),
        Cursor::Crosshair => System(CursorIcon::Crosshair),
        Cursor::Move => System(CursorIcon::Move),
        Cursor::Resize(a) if a.is_finite() => System(resize_icon(a)),
        Cursor::Resize(_) => System(CursorIcon::Default),
        Cursor::Rotate(a) => Drawn(Glyph::Rotate(if a.is_finite() { a.rem_euclid(360.0) as f32 } else { 0.0 })),
        Cursor::Grab => System(CursorIcon::Grab),
        Cursor::Grabbing => System(CursorIcon::Grabbing),
        Cursor::Text => System(CursorIcon::Text),
        Cursor::Pen(p) => Drawn(Glyph::Pen(p)),
        Cursor::Copy => System(CursorIcon::Copy),
        Cursor::Pointer => System(CursorIcon::PointingHand),
        Cursor::Eyedropper => Drawn(Glyph::Eyedropper),
        Cursor::Comment => Drawn(Glyph::Comment),
        Cursor::NotAllowed => System(CursorIcon::NotAllowed),
    }
}

/// What shows on the canvas: panning (Space or the middle button: the open hand, closed while a
/// button is down) and the zoom tool (Z; `Some(true)`: Alt, zooming out) come before the tool's
/// cursor.
pub fn on_canvas(panning: bool, button_down: bool, zoom_out: Option<bool>, cursor: Cursor) -> Shown {
    match (panning, zoom_out) {
        (true, _) => Shown::System(if button_down { CursorIcon::Grabbing } else { CursorIcon::Grab }),
        (false, Some(out)) => Shown::System(if out { CursorIcon::ZoomOut } else { CursorIcon::ZoomIn }),
        (false, None) => of(cursor),
    }
}

impl Shown {
    /// The system cursor to ask for (none under a drawn one).
    pub fn icon(self) -> CursorIcon {
        match self {
            Shown::System(i) => i,
            Shown::Drawn(_) => CursorIcon::None,
        }
    }

    /// A name for it (`ui.inspect`): the CSS cursor names where there is one ("default",
    /// "nwse-resize", "copy"), and "rotate" (with its angle), "pen", "pen-close", "pen-add",
    /// "pen-bend", "eyedropper" and "comment" for the drawn ones.
    pub fn name(self) -> String {
        match self {
            Shown::System(i) => match i {
                CursorIcon::Default => "default",
                CursorIcon::Crosshair => "crosshair",
                CursorIcon::Move => "move",
                CursorIcon::ResizeHorizontal => "ew-resize",
                CursorIcon::ResizeVertical => "ns-resize",
                CursorIcon::ResizeNwSe => "nwse-resize",
                CursorIcon::ResizeNeSw => "nesw-resize",
                CursorIcon::ResizeColumn => "col-resize",
                CursorIcon::ResizeRow => "row-resize",
                CursorIcon::Grab => "grab",
                CursorIcon::Grabbing => "grabbing",
                CursorIcon::Text => "text",
                CursorIcon::Copy => "copy",
                CursorIcon::PointingHand => "pointer",
                CursorIcon::NotAllowed => "not-allowed",
                CursorIcon::ZoomIn => "zoom-in",
                CursorIcon::ZoomOut => "zoom-out",
                _ => "other",
            }
            .to_string(),
            Shown::Drawn(Glyph::Rotate(a)) => format!("rotate {}", a.round()),
            Shown::Drawn(Glyph::Pen(PenCursor::New)) => "pen".into(),
            Shown::Drawn(Glyph::Pen(PenCursor::Close)) => "pen-close".into(),
            Shown::Drawn(Glyph::Pen(PenCursor::Add)) => "pen-add".into(),
            Shown::Drawn(Glyph::Pen(PenCursor::Bend)) => "pen-bend".into(),
            Shown::Drawn(Glyph::Eyedropper) => "eyedropper".into(),
            Shown::Drawn(Glyph::Comment) => "comment".into(),
        }
    }

    /// Ask for the system cursor and draw the glyph, if it is one, at `at` over everything else.
    pub fn show(self, ctx: &egui::Context, at: Pos2) {
        ctx.set_cursor_icon(self.icon());
        if let Shown::Drawn(g) = self {
            paint(&ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("canvas_cursor"))), at, g);
        }
    }
}

/// The UI3 kit's cursors (private use), shown instead when [`crate::canvas::CanvasState`]'s
/// `figma_cursors` is on.
#[rustfmt::skip]
#[allow(clippy::approx_constant, clippy::excessive_precision, clippy::unreadable_literal)]
mod figma;

use figma::FigCursor;

/// The kit's cursor for a system cursor, and how far to turn it (degrees, clockwise).
pub(crate) fn kit_icon(icon: CursorIcon) -> Option<(&'static FigCursor, f32)> {
    use CursorIcon as C;
    Some(match icon {
        C::Default => (&figma::BLACK, 0.0),
        C::Crosshair | C::Cell => (&figma::CROSSHAIR, 0.0),
        C::Move | C::AllScroll => (&figma::MOVE, 0.0),
        C::ResizeHorizontal | C::ResizeEast | C::ResizeWest => (&figma::RESIZE, 0.0),
        C::ResizeVertical | C::ResizeNorth | C::ResizeSouth => (&figma::RESIZE, 90.0),
        // An auto layout padding or gap bar (`handles`): the arrows through a bar.
        C::ResizeColumn => (&figma::RESIZE_PADDING, 0.0),
        C::ResizeRow => (&figma::RESIZE_PADDING, 90.0),
        C::ResizeNwSe | C::ResizeNorthWest | C::ResizeSouthEast => (&figma::RESIZE, 45.0),
        C::ResizeNeSw | C::ResizeNorthEast | C::ResizeSouthWest => (&figma::RESIZE, 135.0),
        C::Grab => (&figma::HAND_NEW, 0.0),
        C::Grabbing => (&figma::HAND_PRESS, 0.0),
        C::Text | C::VerticalText => (&figma::TEXT, 0.0),
        C::Copy | C::Alias => (&figma::DUPLICATE, 0.0),
        C::PointingHand => (&figma::CLICK, 0.0),
        C::NotAllowed | C::NoDrop => (&figma::NOT_ALLOWED, 0.0),
        C::ZoomIn => (&figma::ZOOM_IN, 0.0),
        C::ZoomOut => (&figma::ZOOM_OUT, 0.0),
        _ => return None,
    })
}

/// The kit's cursor for what shows; `cursor` is the engine's, for a resize's exact angle. With the
/// Scale tool (`scale`) a resize shows the scale arrows.
pub(crate) fn kit(shown: Shown, cursor: Option<Cursor>, scale: bool) -> Option<(&'static FigCursor, f32)> {
    let resize = if scale { &figma::RESIZE_SCALE } else { &figma::RESIZE };
    match (shown, cursor) {
        (Shown::System(_), Some(Cursor::Resize(a))) if a.is_finite() => Some((resize, a.rem_euclid(180.0) as f32)),
        (Shown::System(i), _) => kit_icon(i),
        // The kit's rotate arrows sit round a top right corner (-45°).
        (Shown::Drawn(Glyph::Rotate(a)), _) => Some((&figma::ROTATE, a + 45.0)),
        (Shown::Drawn(Glyph::Pen(p)), _) => Some((
            match p {
                PenCursor::New => &figma::PEN,
                PenCursor::Close => &figma::PEN_SNAP,
                PenCursor::Add => &figma::PEN_INSERT,
                PenCursor::Bend => &figma::BEND,
            },
            0.0,
        )),
        (Shown::Drawn(Glyph::Eyedropper), _) => Some((&figma::DROPPER, 0.0)),
        (Shown::Drawn(Glyph::Comment), _) => Some((&figma::COMMENTS_NEXT_PIN, 0.0)),
    }
}

/// Show the kit's cursor for `shown` at `at` (the system's hidden), or `shown` itself where the kit
/// has none.
pub fn show_kit(ctx: &egui::Context, at: Pos2, shown: Shown, cursor: Option<Cursor>, scale: bool) {
    match kit(shown, cursor, scale) {
        Some((c, turn)) => {
            ctx.set_cursor_icon(CursorIcon::None);
            paint_kit(&layer(ctx), at, c, turn);
        }
        None => shown.show(ctx, at),
    }
}

/// Over the canvas, the cursor something asked for this frame (a ruler guide's arrows, a gradient
/// stop's hand) as the kit draws it.
pub fn restyle(ctx: &egui::Context, at: Pos2) {
    let icon = ctx.output(|o| o.cursor_icon);
    if let Some((c, turn)) = kit_icon(icon) {
        ctx.set_cursor_icon(CursorIcon::None);
        paint_kit(&layer(ctx), at, c, turn);
    }
}

fn layer(ctx: &egui::Context) -> Painter {
    ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("canvas_cursor")))
}

/// Draw `c` with its hot spot at `at`, turned `turn` degrees about it: the shadow casters' soft
/// shadow (nine faint copies round a point 1 below), then the shapes.
fn paint_kit(painter: &Painter, at: Pos2, c: &FigCursor, turn: f32) {
    let (sin, cos) = if turn.is_finite() { turn.to_radians().sin_cos() } else { (0.0, 1.0) };
    let hot = vec2(c.hot[0], c.hot[1]);
    let place = |x: f32, y: f32| {
        let d = vec2(x, y) - hot;
        at + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    };
    let px = 1.0 / painter.ctx().pixels_per_point();
    let shadow = Color32::from_black_alpha(14);
    let blur = [(0.0, 0.0), (1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7), (-0.7, -0.7)];
    for l in c.layers.iter().filter(|l| l.shadow) {
        for (dx, dy) in blur {
            mesh(painter, l, shadow, |x, y| place(x, y) + vec2(dx, 1.0 + dy), None);
        }
    }
    for l in c.layers {
        let color = Color32::from_rgb(l.rgb[0], l.rgb[1], l.rgb[2]);
        mesh(painter, l, color, place, Some(Stroke::new(px * 0.5, color)));
    }
}

/// One shape, its points through `place`, with a hairline round its rings to soften the edges.
fn mesh(painter: &Painter, l: &figma::Layer, color: Color32, place: impl Fn(f32, f32) -> Pos2, edge: Option<Stroke>) {
    let at = |i: usize| place(l.verts.get(i * 2).copied().unwrap_or(0.0), l.verts.get(i * 2 + 1).copied().unwrap_or(0.0));
    let n = l.verts.len() / 2;
    let mut m = egui::Mesh::default();
    for i in 0..n {
        m.colored_vertex(at(i), color);
    }
    m.indices = l.tris.iter().map(|&t| u32::from(t)).filter(|&t| (t as usize) < n).collect();
    m.indices.truncate(m.indices.len() / 3 * 3);
    painter.add(Shape::mesh(m));
    if let Some(stroke) = edge {
        let mut start = 0usize;
        for &k in l.rings {
            let k = usize::from(k);
            painter.add(Shape::closed_line((start..(start + k).min(n)).map(&at).collect(), stroke));
            start += k;
        }
    }
}

/// Dark lines over a light edge: readable on any page and artwork.
const INK: Color32 = Color32::from_rgb(0x1E, 0x1E, 0x1E);
const EDGE: Color32 = Color32::WHITE;

/// A line through `pts` with the light edge under it.
fn line(painter: &Painter, pts: &[Pos2]) {
    painter.add(Shape::line(pts.to_vec(), Stroke::new(3.5, EDGE)));
    painter.add(Shape::line(pts.to_vec(), Stroke::new(1.5, INK)));
}

/// `icon` in a 16 px box at `min`, with the light edge around it.
fn icon(painter: &Painter, min: Pos2, icon: Icon) {
    let r = egui::Rect::from_min_size(min, vec2(16.0, 16.0));
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
        icons::paint(painter, r.translate(d), icon, EDGE);
    }
    icons::paint(painter, r, icon, INK);
}

/// Points of the arc of radius `r` about `c` from angle `a0` to `a1` (radians).
fn arc(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    (0..=12).map(|i| a0 + (a1 - a0) * i as f32 / 12.0).map(|a| c + vec2(a.cos(), a.sin()) * r).collect()
}

/// Draw `glyph` with its hot spot at `at`.
pub fn paint(painter: &Painter, at: Pos2, glyph: Glyph) {
    match glyph {
        Glyph::Rotate(degrees) => {
            // An arc bulging away from the box, through the pointer, with an arrowhead at each end.
            let (dir, r, half) = (degrees.to_radians(), 11.0, 0.8);
            let c = at - vec2(dir.cos(), dir.sin()) * r;
            let pts = arc(c, r, dir - half, dir + half);
            line(painter, &pts);
            for (end, sign) in [(dir - half, -1.0), (dir + half, 1.0)] {
                let tip = c + vec2(end.cos(), end.sin()) * r;
                // Along the arc at its end, pointing onwards.
                let t = vec2(-end.sin(), end.cos()) * sign;
                let n = vec2(end.cos(), end.sin());
                line(painter, &[tip - t * 4.5 + n * 3.5, tip + t * 1.0, tip - t * 4.5 - n * 3.5]);
            }
        }
        Glyph::Pen(state) => {
            // A cross open at the centre (the point it lands on stays visible), and what a click
            // does at its lower right.
            for (a, b) in [
                (vec2(-8.0, 0.0), vec2(-2.5, 0.0)),
                (vec2(2.5, 0.0), vec2(8.0, 0.0)),
                (vec2(0.0, -8.0), vec2(0.0, -2.5)),
                (vec2(0.0, 2.5), vec2(0.0, 8.0)),
            ] {
                line(painter, &[at + a, at + b]);
            }
            let b = at + vec2(10.0, 10.0);
            match state {
                PenCursor::New => {}
                PenCursor::Close => {
                    let mut ring = arc(b, 3.0, 0.0, std::f32::consts::TAU);
                    ring.push(pos2(b.x + 3.0, b.y));
                    line(painter, &ring);
                }
                PenCursor::Add => {
                    line(painter, &[b - vec2(3.5, 0.0), b + vec2(3.5, 0.0)]);
                    line(painter, &[b - vec2(0.0, 3.5), b + vec2(0.0, 3.5)]);
                }
                PenCursor::Bend => line(painter, &arc(b + vec2(-3.5, 3.5), 7.0, -std::f32::consts::FRAC_PI_2, 0.0)),
            }
        }
        // The dropper's tip and the pin's tail, at the bottom left of their icons, are the hot spots.
        Glyph::Eyedropper => icon(painter, at - vec2(2.5, 13.5), Icon::Eyedropper),
        Glyph::Comment => icon(painter, at - vec2(2.5, 13.5), Icon::Comment),
    }
}
