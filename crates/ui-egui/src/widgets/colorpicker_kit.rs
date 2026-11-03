//! The colour picker's drawing parts, in the measurements of the UI3 kit's colour picker: 24-point
//! icon buttons, the outlined dropdown, 16-point swatches, the reticle and slider knobs, and the
//! rounded ends of gradient tracks.

use egui::{Align2, Color32, CornerRadius, Id, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_color::Paint;

use crate::a11y::{self, Tip};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular};
use crate::widgets;

/// The picker's width, and the inset of its content from both sides.
pub const W: f32 = 240.0;
pub const INSET: f32 = 16.0;
/// A swatch's side and the pitch of the swatch grid.
pub const SWATCH: f32 = 16.0;
pub const PITCH: f32 = 24.0;

/// Fill the corners of `rect` that lie outside its corners rounded by `r` with `fill`, so a mesh
/// (a gradient) drawn into a square rect shows with rounded corners on a flat background.
pub fn round_corners(painter: &Painter, rect: Rect, r: f32, fill: Color32) {
    let steps = 8;
    for (corner, centre, a0) in [
        (rect.left_top(), pos2(rect.min.x + r, rect.min.y + r), std::f32::consts::PI),
        (rect.right_top(), pos2(rect.max.x - r, rect.min.y + r), std::f32::consts::PI * 1.5),
        (rect.right_bottom(), pos2(rect.max.x - r, rect.max.y - r), 0.0),
        (rect.left_bottom(), pos2(rect.min.x + r, rect.max.y - r), std::f32::consts::PI * 0.5),
    ] {
        let mut mesh = Mesh::default();
        mesh.colored_vertex(corner, fill);
        for i in 0..=steps {
            let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
            mesh.colored_vertex(centre + vec2(a.cos(), a.sin()) * r, fill);
        }
        for i in 0..steps as u32 {
            mesh.add_triangle(0, i + 1, i + 2);
        }
        painter.add(Shape::mesh(mesh));
    }
}

/// A reticle or slider knob at `at`: a 16-point disc with a 4-point white ring and a hairline on
/// both sides of the ring, over a soft shadow, `fill` inside.
pub fn knob(painter: &Painter, at: Pos2, fill: Color32, ring: Color32) {
    painter.circle_filled(at + vec2(0.0, 3.0), 10.5, Color32::from_black_alpha(10));
    painter.circle_filled(at + vec2(0.0, 1.0), 9.5, Color32::from_black_alpha(14));
    painter.circle_filled(at, 9.0, Color32::from_black_alpha(51));
    painter.circle_filled(at, 8.0, ring);
    painter.circle_filled(at, 4.0, Color32::from_black_alpha(51));
    painter.circle_filled(at, 3.0, fill);
}

/// A 24-point icon button with the kit's glyph at its own size: raised on the field colour while
/// `on`, the hover colour under the pointer. `tip` names it for screen readers and the tooltip.
pub fn icon_button(ui: &mut Ui, rect: Rect, id: Id, icon: Icon, on: bool, tip: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(rect, id, Sense::click());
    if on {
        ui.painter().rect_filled(rect, CornerRadius::same(5), t.field);
    } else if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, CornerRadius::same(5), t.pressed);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(5), t.hover);
    }
    let centre = rect.center();
    icons::paint(ui.painter(), Rect::from_center_size(centre, vec2(16.0, 16.0)), icon, t.icon);
    if icon == Icon::PaintSolid {
        // The solid paint's mark is its frame with a tinted square inside.
        ui.painter().rect_filled(Rect::from_center_size(centre, vec2(6.0, 6.0)), CornerRadius::ZERO, t.icon.gamma_multiply(0.3));
    }
    if tip.is_empty() {
        return resp;
    }
    let resp = resp.tip(tip);
    if on {
        a11y::toggle(&resp, tip, true);
    }
    resp
}

/// The dropdown with a hairline outline and the kit's chevron (the colour model, the gradient kind,
/// the image's fill mode, the swatch set). Opens a menu on click like [`widgets::dropdown`].
pub fn outlined_dropdown(ui: &mut Ui, rect: Rect, id: Id, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(rect, id, Sense::click());
    a11y::combo(&resp, "", label, a11y::menu_open(&resp));
    ui.painter().rect_filled(rect, CornerRadius::same(5), t.panel);
    let line = if resp.hovered() { t.field_hover_border } else { t.border };
    ui.painter().rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, line), StrokeKind::Inside);
    let shown = widgets::elide(ui, label, rect.width() - 8.0 - 24.0);
    ui.painter().text(pos2(rect.min.x + 8.0, rect.center().y), Align2::LEFT_CENTER, &shown, regular(11.0), t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.max.x - 12.0, rect.center().y), vec2(16.0, 16.0)), Icon::Chevron, t.icon_secondary);
    if shown != label { resp.on_hover_text(label) } else { resp }
}

/// A 16-point swatch with 3.2-point corners: the colour's left half opaque and its right half with
/// its alpha over a checkerboard, a hairline round it. A paint that isn't a colour shows its mark.
pub fn swatch(ui: &mut Ui, rect: Rect, id: Id, paint: &Paint, name: &str) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let value = if a11y::active(&resp.ctx) { widgets::paint_name(paint) } else { String::new() };
    a11y::color(&resp, name, &value);
    paint_swatch(ui, rect, paint);
    resp
}

/// [`swatch`]'s drawing alone.
pub fn paint_swatch(ui: &Ui, rect: Rect, paint: &Paint) {
    let t = Tokens::get(ui.ctx());
    let panel = ui.painter();
    let clip = panel.with_clip_rect(rect);
    // The corners are the rect's own: draw the contents into a square, then take the corners back.
    widgets::paint_swatch(&clip, rect, paint);
    round_corners(&panel.with_clip_rect(rect.expand(1.0)), rect, 3.2, t.panel);
    panel.rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.0, Color32::from_black_alpha(26)), StrokeKind::Inside);
}
