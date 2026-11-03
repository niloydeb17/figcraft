//! The Motion panel: the right panel while the Motion timeline is open (Prototype mode). Like the
//! incumbent's, it lists the selected layer's animatable properties as they are at the playhead,
//! each with a keyframe button (◇) at the end of its row, under an Animations section that adds
//! ready-made animations. Editing a value writes a keyframe at the playhead; the button adds one
//! there with the value shown, or removes the one already there.

use egui::{Color32, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use figcraft_color::PaintKind;
use figcraft_doc::motion::{MotionField, MotionValue, TIME_STEP};
use figcraft_doc::{Node, NodeId};
use serde_json::json;

use super::menu::row;
use crate::FigApp;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::Tokens;
use crate::widgets::{self, FieldEdit, Lead};

/// A keyframe button's state for one row.
#[derive(Clone, Copy, PartialEq)]
enum KeyMark {
    /// Not animated.
    None,
    /// Animated, with no keyframe at the playhead.
    Animated,
    /// A keyframe sits at the playhead.
    Here,
}

pub(crate) fn panel(app: &mut FigApp, ui: &mut Ui) {
    let Ok(ds) = app.session.ds() else { return };
    let t = Tokens::get(ui.ctx());
    let doc = ds.doc.clone();
    let page = ds.page;
    let playhead = ds.motion.playhead;
    let sel: Vec<NodeId> = ds.selection.clone();
    let Some(id) = sel.first().copied() else {
        hint(ui, &t, tr("Select a layer inside a frame to animate it"));
        return;
    };
    let Some(rest) = doc.node(id).cloned() else { return };
    let frame = doc.top_level_frame(id);
    if frame == Some(id) || frame.is_none() {
        // The frame itself holds the timeline and isn't animated, but its look still edits here
        // as in the Design panel (opacity, corner radius, fills and strokes).
        heading(ui, &t, &rest.name);
        widgets::separator(ui);
        super::paints::appearance(app, ui, &doc, &sel);
        widgets::separator(ui);
        super::paints::paints(app, ui, &doc, &sel, true);
        widgets::separator(ui);
        super::paints::paints(app, ui, &doc, &sel, false);
        widgets::separator(ui);
        hint(ui, &t, tr("Select a layer inside the frame to animate it"));
        return;
    }
    // The layer as it looks at the playhead.
    let shown = doc.at_time(page, playhead).and_then(|d| d.node(id).cloned()).unwrap_or_else(|| rest.clone());
    heading(ui, &t, &rest.name);
    widgets::separator(ui);

    // Animations: the ready-made animations.
    let (button, _) = widgets::section_header_resp(ui, tr("Animations"), &[(Icon::Plus, tr("Add animation"))], false);
    let open_id = Id::unique("motion_panel_styles");
    if button == Some(0) {
        ui.data_mut(|d| {
            let open = d.get_temp::<bool>(open_id).unwrap_or(false);
            d.insert_temp(open_id, !open);
        });
    }
    if ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false) {
        for (label, preset, timing) in STYLES {
            row(ui, 28.0, |ui| {
                if ui.add(egui::Button::new(egui::RichText::new(tr(label)).size(11.0)).frame(false)).clicked() {
                    app.run("motion.preset", json!({"ids": sel, "preset": preset, "timing": timing}));
                    ui.data_mut(|d| d.insert_temp(open_id, false));
                }
            });
        }
    }
    animated_list(app, ui, &t, &rest, id);
    widgets::separator(ui);

    let col = widgets::col_width(ui.available_width());
    let mark = |f: MotionField| key_mark(&rest, f, playhead);
    // Transform.
    widgets::section_header(ui, tr("Transform"), &[], false);
    let (rx, ry) = (rest.transform.as_coeffs()[4], rest.transform.as_coeffs()[5]);
    let (sx, sy) = (shown.transform.as_coeffs()[4], shown.transform.as_coeffs()[5]);
    let tx = num(&shown, &rest, MotionField::TranslationX, playhead);
    let ty = num(&shown, &rest, MotionField::TranslationY, playhead);
    widgets::sublabels(ui, tr("Position"), None);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::unique("motion_pos_x"), Lead::Text("X"), Some(round(rx + tx)), "", col - 14.0, 1.0);
        set_on_edit(app, &sel, &rest, MotionField::TranslationX, e, |v| v - rx, Some(("x", |v| v)));
        let e = widgets::number_field(ui, Id::unique("motion_pos_y"), Lead::Text("Y"), Some(round(ry + ty)), "", col - 14.0, 1.0);
        set_on_edit(app, &sel, &rest, MotionField::TranslationY, e, |v| v - ry, Some(("y", |v| v)));
        key_button(
            app,
            ui,
            &sel,
            &[MotionField::TranslationX, MotionField::TranslationY],
            mark(MotionField::TranslationX).max(mark(MotionField::TranslationY)),
            "pos",
        );
    });
    let _ = (sx, sy);
    let scx = num(&shown, &rest, MotionField::ScaleX, playhead);
    let scy = num(&shown, &rest, MotionField::ScaleY, playhead);
    widgets::sublabels(ui, tr("Scale"), None);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::unique("motion_scale_x"), Lead::Text("W"), Some(round(scx * 100.0)), "%", col - 14.0, 1.0);
        set_on_edit(app, &sel, &rest, MotionField::ScaleX, e, |v| v / 100.0, None);
        let e = widgets::number_field(ui, Id::unique("motion_scale_y"), Lead::Text("H"), Some(round(scy * 100.0)), "%", col - 14.0, 1.0);
        set_on_edit(app, &sel, &rest, MotionField::ScaleY, e, |v| v / 100.0, None);
        key_button(app, ui, &sel, &[MotionField::ScaleX, MotionField::ScaleY], mark(MotionField::ScaleX).max(mark(MotionField::ScaleY)), "scale");
    });
    let rot = num(&shown, &rest, MotionField::Rotation, playhead);
    let rest_rot = figcraft_geom::rotation_degrees(rest.transform);
    widgets::sublabels(ui, tr("Rotation"), None);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(
            ui,
            Id::unique("motion_rot"),
            Lead::Icon(Icon::Angle, tr("Rotation")),
            Some(round(rest_rot + rot)),
            "°",
            col - 14.0,
            1.0,
        );
        set_on_edit(app, &sel, &rest, MotionField::Rotation, e, |v| v - rest_rot, Some(("rotation", |v| v)));
        key_button(app, ui, &sel, &[MotionField::Rotation], mark(MotionField::Rotation), "rot");
    });
    widgets::separator(ui);

    // Layout: the size.
    widgets::section_header(ui, tr("Layout"), &[], false);
    widgets::sublabels(ui, tr("Dimensions"), None);
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::unique("motion_w"), Lead::Text("W"), Some(round(shown.width)), "", col - 14.0, 1.0);
        set_on_edit(app, &sel, &rest, MotionField::Width, e, |v| v, Some(("width", |v| v.max(0.0))));
        key_button(app, ui, &sel, &[MotionField::Width], mark(MotionField::Width), "w");
        let e = widgets::number_field(ui, Id::unique("motion_h"), Lead::Text("H"), Some(round(shown.height)), "", col - 14.0, 1.0);
        set_on_edit(app, &sel, &rest, MotionField::Height, e, |v| v, Some(("height", |v| v.max(0.0))));
        key_button(app, ui, &sel, &[MotionField::Height], mark(MotionField::Height), "h");
    });
    widgets::separator(ui);

    // Appearance.
    widgets::section_header(ui, tr("Appearance"), &[], false);
    widgets::sublabels(ui, tr("Opacity"), Some(tr("Corner radius")));
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(
            ui,
            Id::unique("motion_opacity"),
            Lead::Icon(Icon::Opacity, tr("Opacity")),
            Some(round(f64::from(shown.opacity) * 100.0)),
            "%",
            col - 14.0,
            1.0,
        );
        set_on_edit(app, &sel, &rest, MotionField::Opacity, e, |v| (v / 100.0).clamp(0.0, 1.0), Some(("opacity", |v| (v / 100.0).clamp(0.0, 1.0))));
        key_button(app, ui, &sel, &[MotionField::Opacity], mark(MotionField::Opacity), "opacity");
        let r = shown.corners.radii.first().copied().unwrap_or(0.0);
        let e = widgets::number_field(
            ui,
            Id::unique("motion_radius"),
            Lead::Icon(Icon::CornerRadius, tr("Corner radius")),
            Some(round(r)),
            "",
            col - 14.0,
            1.0,
        );
        set_on_edit(app, &sel, &rest, MotionField::CornerRadius, e, |v| v.max(0.0), Some(("cornerRadius", |v| v.max(0.0))));
        key_button(app, ui, &sel, &[MotionField::CornerRadius], mark(MotionField::CornerRadius), "radius");
    });
    widgets::separator(ui);

    // Fill and stroke: the first solid paint's colour.
    for (title, paints, field, key) in
        [(tr("Fill"), &shown.fills, MotionField::Fill(0), "fill"), (tr("Stroke"), &shown.strokes, MotionField::Stroke(0), "stroke")]
    {
        let Some(paint) = paints.first() else { continue };
        let PaintKind::Solid { color } = paint.kind else { continue };
        widgets::section_header(ui, title, &[], false);
        let fills = key == "fill";
        let animated = mark(field) != KeyMark::None;
        row(ui, 32.0, |ui| {
            paint_row(app, ui, &t, &sel, paint, color, fills, animated, col * 2.0 - 14.0);
            key_button(app, ui, &sel, &[field], mark(field), key);
        });
        widgets::separator(ui);
    }
    crate::panels::motion_timeline::picked_section(app, ui);
}

/// The ready-made animations offered under Animations.
const STYLES: [(&str, &str, &str); 10] = [
    (crate::i18n::tr_noop("Position: slide in"), "position", "in"),
    (crate::i18n::tr_noop("Position: slide out"), "position", "out"),
    (crate::i18n::tr_noop("Scale: scale in"), "scale", "in"),
    (crate::i18n::tr_noop("Scale: scale out"), "scale", "out"),
    (crate::i18n::tr_noop("Rotation: rotate in"), "rotation", "in"),
    (crate::i18n::tr_noop("Rotation: rotate out"), "rotation", "out"),
    (crate::i18n::tr_noop("Size: resize in"), "size", "in"),
    (crate::i18n::tr_noop("Size: resize out"), "size", "out"),
    (crate::i18n::tr_noop("Opacity: fade in"), "opacity", "in"),
    (crate::i18n::tr_noop("Opacity: fade out"), "opacity", "out"),
];

fn heading(ui: &mut Ui, t: &Tokens, name: &str) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
    let shown = widgets::elide_font(ui, name, w - 32.0, egui::FontId::proportional(13.0));
    ui.painter().text(pos2(r.left() + 16.0, r.center().y), egui::Align2::LEFT_CENTER, shown, egui::FontId::proportional(13.0), t.text);
}

fn hint(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.add_space(16.0);
    row(ui, 40.0, |ui| {
        ui.label(egui::RichText::new(text).size(11.0).color(t.text_secondary));
    });
}

/// The layer's animated properties, one row each with a remove button.
fn animated_list(app: &mut FigApp, ui: &mut Ui, t: &Tokens, n: &Node, id: NodeId) {
    let Some(m) = n.motion.as_ref() else { return };
    for (f, track) in m.fields() {
        row(ui, 28.0, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width() - 28.0, 24.0), Sense::hover());
            crate::panels::motion_timeline::keyframe_glyph(ui.painter(), pos2(r.left() + 8.0, r.center().y), t.icon_secondary, false);
            let text = format!("{} · {} {}", f.label(), track.keyframes.len(), tr("keyframes"));
            ui.painter().text(pos2(r.left() + 22.0, r.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(11.0), t.text);
            if icons::button(ui, Icon::Minus, 24.0, false, tr("Remove animation")).clicked() {
                app.run("motion.track.remove", json!({"ids": [id], "field": f.key()}));
            }
        });
    }
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// A number field's animated value at the playhead (the field's neutral or rest value without a
/// track).
fn num(shown: &Node, rest: &Node, f: MotionField, t: f64) -> f64 {
    let _ = shown;
    rest.motion
        .as_ref()
        .and_then(|m| m.track(f))
        .and_then(|tr| tr.sample(t))
        .and_then(MotionValue::as_f64)
        .or_else(|| f.rest_value(rest).and_then(MotionValue::as_f64))
        .unwrap_or(0.0)
}

fn key_mark(n: &Node, f: MotionField, t: f64) -> KeyMark {
    match n.motion.as_ref().and_then(|m| m.track(f)) {
        None => KeyMark::None,
        Some(tr) if tr.keyframes.iter().any(|k| (k.time - t).abs() < TIME_STEP * 500.0) => KeyMark::Here,
        Some(_) => KeyMark::Animated,
    }
}

impl KeyMark {
    fn max(self, o: KeyMark) -> KeyMark {
        let rank = |k: KeyMark| match k {
            KeyMark::None => 0,
            KeyMark::Animated => 1,
            KeyMark::Here => 2,
        };
        if rank(o) > rank(self) { o } else { self }
    }
}

/// A layer property and how a field's shown value becomes its value.
type Direct = (&'static str, fn(f64) -> f64);

/// An edited value. On an animated field it's written as a keyframe at the playhead (`to_track`
/// turns the field's shown value into the track's); on a field that isn't animated it changes the
/// layer itself through `direct` (the property and the shown value turned into its value), as the
/// Design panel would. Fields with no property of their own (scale) always take a keyframe.
fn set_on_edit(
    app: &mut FigApp,
    ids: &[NodeId],
    rest: &Node,
    f: MotionField,
    e: Option<FieldEdit>,
    to_track: impl Fn(f64) -> f64,
    direct: Option<Direct>,
) {
    let v = match e {
        Some(FieldEdit::Set(v)) | Some(FieldEdit::Scrub(v, true)) => v,
        _ => return,
    };
    if !v.is_finite() {
        return;
    }
    let animated = rest.motion.as_ref().is_some_and(|m| m.track(f).is_some());
    if !animated && let Some((prop, to_prop)) = direct {
        let value = to_prop(v);
        if value.is_finite() {
            app.set_props(ids, json!({ prop: value }));
        }
        return;
    }
    let value = to_track(v);
    if value.is_finite() {
        app.run("motion.keyframe.add", json!({"ids": ids, "field": f.key(), "value": value}));
    }
}

/// The paint row of Fill or Stroke (the first paint, a solid colour): the swatch opens the colour
/// picker and the hex field takes a colour. An animated colour takes a keyframe at the playhead;
/// otherwise the paint itself changes.
#[allow(clippy::too_many_arguments)]
fn paint_row(
    app: &mut FigApp,
    ui: &mut Ui,
    t: &Tokens,
    ids: &[NodeId],
    paint: &figcraft_color::Paint,
    color: figcraft_color::Color,
    fills: bool,
    animated: bool,
    width: f32,
) {
    let (r, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    ui.painter().rect_filled(r, 5.0, t.field);
    let sw = Rect::from_center_size(pos2(r.left() + 14.0, r.center().y), vec2(14.0, 14.0));
    widgets::paint_swatch(ui.painter(), sw, paint);
    let key = if fills { "fill" } else { "stroke" };
    let swatch = ui.interact(sw.expand(3.0), Id::unique("motion_swatch").with(key), Sense::click());
    // An animated colour's picker writes keyframes at the playhead; otherwise it edits the paint.
    let target = if animated {
        crate::colorpicker::PickerTarget::MotionColor { fills, ids: ids.to_vec() }
    } else {
        crate::colorpicker::PickerTarget::Paint { fills, index: 0, ids: ids.to_vec() }
    };
    app.picker_control(target, &swatch);
    let text_r = Rect::from_min_max(pos2(sw.right() + 8.0, r.top()), pos2(r.right() - 4.0, r.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_r));
    if let Some(h) = widgets::text_field_inline(
        &mut child,
        Id::unique("motion_hex").with(key),
        &color.to_hex(),
        text_r.shrink2(vec2(0.0, 2.0)),
        egui::FontId::proportional(11.0),
    ) && let Some(nc) = figcraft_color::Color::from_hex_or_name(&h)
    {
        let field = if fills { MotionField::Fill(0) } else { MotionField::Stroke(0) };
        if animated {
            app.run("motion.keyframe.add", json!({"ids": ids, "field": field.key(), "value": nc.to_css_hex()}));
        } else {
            app.run(if fills { "fill.set" } else { "stroke.set" }, json!({"ids": ids, "index": 0, "color": nc.to_css_hex()}));
        }
    }
}

/// The ◇ at the end of a row: adds a keyframe at the playhead for `fields` (the value shown), or
/// removes the one there. Filled blue with a keyframe at the playhead, a dotted diamond when the
/// field is animated elsewhere.
fn key_button(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], fields: &[MotionField], mark: KeyMark, key: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let tip = if mark == KeyMark::Here { tr("Remove keyframe") } else { tr("Add keyframe") };
    let resp = ui.interact(r, Id::unique("motion_key_button").with(key), Sense::click()).union(resp).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 5.0, t.hover);
    }
    diamond(ui.painter(), r.center(), mark, &t);
    if resp.clicked() {
        for f in fields {
            if mark == KeyMark::Here {
                app.run("motion.keyframe.remove", json!({"ids": ids, "field": f.key()}));
            } else {
                app.run("motion.keyframe.add", json!({"ids": ids, "field": f.key()}));
            }
        }
    }
}

fn diamond(p: &egui::Painter, c: Pos2, mark: KeyMark, t: &Tokens) {
    let r = 5.0;
    let pts = vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)];
    match mark {
        KeyMark::Here => {
            p.add(egui::Shape::convex_polygon(pts, crate::theme::BRAND, Stroke::NONE));
        }
        KeyMark::Animated => {
            p.add(egui::Shape::closed_line(pts, Stroke::new(1.0, crate::theme::BRAND)));
            p.circle_filled(c, 1.5, crate::theme::BRAND);
        }
        KeyMark::None => {
            p.add(egui::Shape::closed_line(pts, Stroke::new(1.0, t.icon)));
        }
    }
    let _ = Color32::TRANSPARENT;
}
