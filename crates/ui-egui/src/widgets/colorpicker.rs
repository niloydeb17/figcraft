//! The colour picker popover: paint type, saturation/brightness square, hue and alpha sliders,
//! hex and RGB fields, gradient stops, image and pattern fills and document colours.

use egui::{Align2, Color32, CornerRadius, Id, Mesh, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_color::{
    BlendMode, Color, ColorStop, Gradient, GradientKind, Hsb, ImageFilters, ImagePaint, ImageScaleMode, Paint, PaintKind, PatternAlign, PatternPaint,
    PatternTile,
};
use figcraft_doc::{NodeId, NodeKind};
use serde_json::json;

use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
use crate::popover::{Hold, Open, Popover};
use crate::theme::{Tokens, doc_c32, regular};
use crate::widgets;

#[path = "colorpicker_kit.rs"]
mod kit;

#[derive(Clone, Debug, PartialEq)]
pub enum PickerTarget {
    PageBackground,
    SelectionColor {
        targets: Vec<figcraft_engine::selection_colors::PaintTarget>,
        selection: Vec<NodeId>,
        document: u64,
    },
    Paint {
        fills: bool,
        index: usize,
        ids: Vec<NodeId>,
    },
    Effect {
        ids: Vec<NodeId>,
        index: usize,
        /// A duo noise's second colour.
        secondary: bool,
    },
    /// The fill of a character range of a text layer being edited.
    TextRange {
        id: NodeId,
        start: usize,
        end: usize,
    },
    /// A colour variable's value in one mode.
    Variable {
        id: String,
        mode: String,
    },
    /// One paint of a colour style (edited from the style picker).
    Style {
        id: String,
        index: usize,
    },
    /// An animated fill or stroke colour (the first paint) in the Motion panel: the picker shows
    /// the colour at the playhead and each choice writes a keyframe there.
    MotionColor {
        fills: bool,
        ids: Vec<NodeId>,
    },
    /// A colour field of a plugin's panel or parameters form. A drag only shows `pending`; the
    /// plugin hears of the colour when the drag ends.
    PluginField {
        plugin: String,
        control: String,
        pending: Option<String>,
    },
}

/// The paints of colour style `id`.
fn style_paints(doc: &figcraft_doc::Document, id: &str) -> Option<Vec<Paint>> {
    match &doc.style(id)?.kind {
        figcraft_doc::StyleKind::Paint { paints } => Some(paints.clone()),
        _ => None,
    }
}

/// The fills a text range paints with (its own, else the layer's).
pub fn range_fills(doc: &figcraft_doc::Document, id: NodeId, start: usize) -> Option<Vec<Paint>> {
    let n = doc.node(id)?;
    let t = n.kind.text()?;
    Some(t.style_at(start).fills.clone().unwrap_or_else(|| n.fills.clone()))
}

fn current_paint(app: &FigApp, target: &PickerTarget) -> Option<Paint> {
    let doc = app.session.doc().ok()?;
    match target {
        PickerTarget::SelectionColor { targets, selection, document } => {
            let ds = app.session.ds().ok()?;
            if ds.uid != *document || ds.selection != *selection {
                return None;
            }
            targets.first()?.paint(doc)
        }
        PickerTarget::PageBackground => match &doc.node(app.session.page().ok()?)?.kind {
            NodeKind::Page { background, .. } => Some(Paint::solid(*background)),
            _ => None,
        },
        PickerTarget::Paint { fills, index, ids } => {
            let n = doc.node(*ids.first()?)?;
            let list = if *fills { &n.fills } else { &n.strokes };
            list.get(*index).cloned()
        }
        PickerTarget::MotionColor { fills, ids } => {
            let ds = app.session.ds().ok()?;
            let id = *ids.first()?;
            let shown = doc.at_time(ds.page, ds.motion.playhead).and_then(|d| d.node(id).cloned()).or_else(|| doc.node(id).cloned())?;
            let list = if *fills { &shown.fills } else { &shown.strokes };
            list.first().cloned()
        }
        PickerTarget::Effect { ids, index, secondary } => doc.node(*ids.first()?)?.effects.get(*index)?.color(*secondary).map(Paint::solid),
        PickerTarget::TextRange { id, start, .. } => range_fills(doc, *id, *start)?.into_iter().next(),
        PickerTarget::Variable { id, mode } => match doc.variables.get_by_id(id)?.values.get(mode)? {
            figcraft_doc::VariableValue::Color(c) => {
                let mut p = Paint::solid(c.with_alpha(1.0));
                p.opacity = c.a;
                Some(p)
            }
            _ => None,
        },
        PickerTarget::Style { id, index } => style_paints(doc, id)?.get(*index).cloned(),
        PickerTarget::PluginField { plugin, control, pending } => {
            let hex = match pending {
                Some(hex) => hex.clone(),
                None => crate::plugins_ui::field_value(app, plugin, control)?,
            };
            let c = Color::from_hex(&hex)?;
            let mut p = Paint::solid(c.with_alpha(1.0));
            p.opacity = c.a;
            Some(p)
        }
    }
}

/// The command and its params that write `paint` to `target`, when the target can take it.
fn paint_command(app: &FigApp, target: &PickerTarget, paint: &Paint) -> Option<(&'static str, serde_json::Value)> {
    Some(match target {
        PickerTarget::PluginField { .. } => return None,
        PickerTarget::SelectionColor { targets, .. } => ("selection.setPaint", json!({"targets": targets, "paint": paint})),
        PickerTarget::PageBackground => {
            let c = paint.solid_color().unwrap_or(Color::DEFAULT_PAGE);
            ("page.setBackground", json!({"color": c.to_css_hex()}))
        }
        PickerTarget::Paint { fills, index, ids } => {
            (if *fills { "fill.set" } else { "stroke.set" }, json!({"ids": ids, "index": index, "paint": paint}))
        }
        PickerTarget::MotionColor { fills, ids } => {
            // A keyframe holds a solid colour: other paint kinds aren't animated.
            let c = paint.solid_color()?;
            let field = if *fills { "fills/0" } else { "strokes/0" };
            ("motion.keyframe.add", json!({"ids": ids, "field": field, "value": c.to_css_hex()}))
        }
        PickerTarget::Effect { ids, index, secondary } => {
            let mut e = ids.first().and_then(|i| app.session.doc().ok()?.node(*i)?.effects.get(*index).cloned())?;
            if let Some(c) = e.color_mut(*secondary) {
                *c = paint.solid_color().unwrap_or(Color::BLACK);
            }
            ("effect.set", json!({"ids": ids, "index": index, "effect": e}))
        }
        PickerTarget::TextRange { id, start, end } => ("text.style", json!({"id": id, "start": start, "end": end, "textFills": [paint]})),
        PickerTarget::Variable { id, mode } => {
            let c = paint.solid_color().unwrap_or(Color::BLACK);
            ("variable.set", json!({"variable": id, "mode": mode, "value": c.to_css_hex()}))
        }
        PickerTarget::Style { id, index } => {
            let mut paints = app.session.doc().ok().and_then(|d| style_paints(d, id))?;
            if let Some(slot) = paints.get_mut(*index) {
                *slot = paint.clone();
            }
            ("style.update", json!({"style": id, "value": paints}))
        }
    })
}

/// Write `paint` to the target. `live` edits are part of an ongoing drag.
fn write(app: &mut FigApp, target: &PickerTarget, paint: &Paint, live: bool) {
    if let PickerTarget::PluginField { plugin, control, .. } = target {
        let hex = paint.solid_color().unwrap_or(Color::BLACK).to_css_hex();
        let (plugin, control) = (plugin.clone(), control.clone());
        if !live {
            crate::plugins_ui::set_field(app, &plugin, &control, &hex);
        }
        app.ui.picker = Some(PickerTarget::PluginField { plugin, control, pending: live.then_some(hex) });
        return;
    }
    if live {
        let _ = app.session.begin_interaction("Color");
    }
    let Some((cmd, params)) = paint_command(app, target, paint) else { return };
    if let Err(e) = app.session.execute(cmd, &params) {
        app.toast(e.to_string());
    }
    // Drag previews (`live`) are not choices: a drag records only the colour it ends on. Not "an
    // interaction is open": editing text holds one, and its colour clicks are choices.
    if !live && let Some(c) = paint.solid_color() {
        remember_recent(app, c);
    }
}

/// Show the target's paint with `blend` on the canvas while its blend mode row is hovered.
pub(crate) fn preview_blend(app: &mut FigApp, ctx: &egui::Context, target: &PickerTarget, paint: &Paint, blend: BlendMode) {
    let mut p = paint.clone();
    p.blend = blend;
    if let Some((cmd, params)) = paint_command(app, target, &p) {
        app.preview_text(ctx, cmd, params);
    }
}

fn remember_recent(app: &mut FigApp, c: Color) {
    // By 8-bit value: a document stores colours rounded to 8 bits, so the colour picked and the one read
    // back differ by a fraction and would show as two identical swatches.
    app.session.recent_colors.retain(|x| x.to_rgba8() != c.to_rgba8());
    app.session.recent_colors.insert(0, c);
    app.session.recent_colors.truncate(16);
}

fn finish(app: &mut FigApp) {
    if let Some(PickerTarget::PluginField { plugin, control, pending: Some(hex) }) = app.ui.picker.clone() {
        crate::plugins_ui::set_field(app, &plugin, &control, &hex);
        app.ui.picker = Some(PickerTarget::PluginField { plugin, control, pending: None });
    }
    if app.session.ds().map(|d| d.interaction.is_some()).unwrap_or(false) {
        if let Some(t) = app.ui.picker.clone()
            && !matches!(t, PickerTarget::PluginField { .. })
            && let Some(c) = current_paint(app, &t).and_then(|p| p.solid_color())
        {
            remember_recent(app, c);
        }
        let _ = app.session.commit_interaction();
    }
}

/// The selected gradient stop in the picker (shared with the on-canvas stops).
pub fn stop_id() -> Id {
    Id::unique("cp_stop")
}

/// Apply a swatch colour: a solid paint's colour, or the selected gradient stop's.
fn set_color(app: &mut FigApp, target: &PickerTarget, paint: &mut Paint, c: Color, stop: Id, ui: &Ui) {
    match &mut paint.kind {
        PaintKind::Solid { color } => {
            *color = c.with_alpha(1.0);
            paint.opacity = c.a;
        }
        PaintKind::Gradient(g) => {
            let sel: usize = ui.data(|d| d.get_temp(stop)).unwrap_or(0);
            if let Some(s) = g.stops.get_mut(sel) {
                s.color = c;
            }
        }
        PaintKind::Image(_) | PaintKind::Pattern(_) => return,
    }
    write(app, target, paint, false);
}

/// The page's most used solid colours, recomputed only when the document snapshot changes.
fn page_colors(app: &FigApp, ui: &Ui) -> Vec<Color> {
    let Ok(ds) = app.session.ds() else { return vec![] };
    let key = (std::sync::Arc::as_ptr(&ds.doc) as usize, ds.page);
    let cache_id = Id::unique("cp_doc_colors");
    if let Some((k, colors)) = ui.data(|d| d.get_temp::<((usize, figcraft_doc::NodeId), Vec<Color>)>(cache_id))
        && k == key
    {
        return colors;
    }
    let colors: Vec<Color> = figcraft_engine::cmd::styles::document_colors(&ds.doc, ds.page).into_iter().take(21).map(|(c, _)| c).collect();
    ui.data_mut(|d| d.insert_temp(cache_id, (key, colors.clone())));
    colors
}

/// Colour models the value row can show.
const MODELS: [&str; 7] = ["Hex", "RGB", "HSL", "HSB", "LCH", "OKLCH", "CSS"];

/// The kit's value row: the model button, then the value fields joined by a hairline to the alpha
/// field. Returns a new colour and whether the edit is done (false while a field is being scrubbed).
fn model_fields(ui: &mut Ui, c: Color) -> Option<(Color, bool)> {
    let t = Tokens::get(ui.ctx());
    let model_id = Id::unique("cp_model");
    let model: usize = ui.data(|d| d.get_temp(model_id)).unwrap_or(0).min(MODELS.len() - 1);
    let mut out: Option<Color> = None;
    let mut done = true;
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 40.0), Sense::hover());
    let y0 = row.center().y - 12.0;
    // "LCH" and "OKLCH" need a wider button; their fields and alpha give up the room.
    let wide = matches!(MODELS.get(model).copied(), Some("LCH" | "OKLCH"));
    let name = MODELS.get(model).copied().unwrap_or("Hex");
    let button_w = if wide { 66.0 } else { 55.0 };
    let button =
        kit::outlined_dropdown(ui, Rect::from_min_size(pos2(row.min.x + kit::INSET, y0), vec2(button_w, 24.0)), Id::unique("cp_model_btn"), name);
    crate::right::menu_popup(&button).show(|ui| {
        for (i, m) in MODELS.iter().enumerate() {
            if crate::right::menu_item(ui, m, "") {
                ui.data_mut(|d| d.insert_temp(model_id, i));
                ui.close();
            }
        }
    });
    let x = row.min.x + kit::INSET + button_w + 8.0;
    let mut fields = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(pos2(x, y0), pos2(row.max.x - kit::INSET + 1.0, y0 + 24.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
    );
    let ui = &mut fields;
    ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
    // The rects of the fields drawn so far, to join them: flat where they meet.
    let mut drawn: Vec<Rect> = Vec::new();
    let place = |ui: &Ui, w: f32| Rect::from_min_size(ui.cursor().min, vec2(w, 24.0));
    // `decimals` places shown; `step` is what a scrub or arrow moves by.
    let num = |ui: &mut Ui, id: &str, v: f64, w: f32, decimals: i32, step: f64| {
        let p = 10f64.powi(decimals);
        match widgets::number_field(ui, Id::unique(("cp_f", id)), widgets::Lead::None, Some((v * p).round() / p), "", w, step) {
            Some(widgets::FieldEdit::Set(x)) => Some((x, true)),
            Some(widgets::FieldEdit::Scrub(x, end)) => Some((x, end)),
            None => None,
        }
    };
    let triple = if wide { 30.0 } else { 29.0 };
    match name {
        "RGB" => {
            let [r, g, b, _] = c.to_rgba8();
            let vals = [f64::from(r), f64::from(g), f64::from(b)];
            for (k, v) in vals.iter().enumerate() {
                drawn.push(place(ui, triple));
                if let Some((x, end)) = num(ui, ["r", "g", "b"][k.min(2)], *v, triple, 0, 1.0) {
                    done = end;
                    let mut rgb = vals;
                    if let Some(slot) = rgb.get_mut(k) {
                        *slot = x.clamp(0.0, 255.0);
                    }
                    out = Some(Color::new(rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, c.a));
                }
            }
        }
        "HSL" | "HSB" | "LCH" | "OKLCH" => {
            if let Some(spec) = Polar::of(name) {
                // The values last shown or typed stay while the colour is theirs: a chroma lowered to
                // fit sRGB, or a hue on a grey, isn't read back from the colour on the next frame.
                let kept_id = Id::unique("cp_polar");
                let rgba = c.to_rgba8();
                let vals = ui
                    .data(|d| d.get_temp::<(&'static str, [f64; 3], [u8; 4])>(kept_id))
                    .filter(|(m, _, k)| *m == name && *k == rgba)
                    .map_or_else(|| spec.values(c), |(_, v, _)| v);
                let mut kept = vals;
                for (k, (v, f)) in vals.iter().zip(spec.fields()).enumerate() {
                    drawn.push(place(ui, triple));
                    if let Some((x, end)) = num(ui, f.id, *v, triple, f.decimals, f.step) {
                        done = end;
                        let mut v3 = vals;
                        if let Some(slot) = v3.get_mut(k) {
                            *slot = if f.wraps { x.rem_euclid(360.0) } else { x.clamp(0.0, f.max) };
                        }
                        let col = spec.color(v3).with_alpha(c.a);
                        kept = v3;
                        out = Some(col);
                    }
                }
                let shown = out.unwrap_or(c).to_rgba8();
                ui.data_mut(|d| d.insert_temp(kept_id, (name, kept, shown)));
            }
        }
        "CSS" => {
            drawn.push(place(ui, 145.0));
            if let Some(text) = widgets::text_field(ui, Id::unique("cp_css"), &c.to_css(), 145.0, "") {
                out = Color::parse_css(&text);
            }
        }
        _ => {
            drawn.push(place(ui, 89.0));
            if let Some(h) = widgets::text_field(ui, Id::unique("cp_hex"), &c.to_hex(), 89.0, "") {
                out = Color::from_hex_or_name(&h).map(|h| h.with_alpha(if h.a < 1.0 { h.a } else { c.a }));
            }
        }
    }
    if name != "CSS" {
        let alpha_w = if wide { 40.0 } else { 54.0 };
        drawn.push(place(ui, alpha_w));
        let alpha = Some((f64::from(out.unwrap_or(c).a) * 100.0).round());
        // The percent sign sits dim at the end of the field.
        let dim = Id::unique("dim_percent");
        ui.data_mut(|d| d.insert_temp(dim, true));
        let edit = match widgets::number_field(ui, Id::unique("cp_a"), widgets::Lead::None, alpha, "%", alpha_w, 1.0) {
            Some(widgets::FieldEdit::Set(v)) => Some((v, true)),
            Some(widgets::FieldEdit::Scrub(v, end)) => Some((v, end)),
            None => None,
        };
        ui.data_mut(|d| d.remove::<bool>(dim));
        if let Some((v, end)) = edit {
            done = end;
            out = Some(out.unwrap_or(c).with_alpha((v / 100.0).clamp(0.0, 1.0) as f32));
        }
    }
    // The fields are one strip: square the corners where two meet.
    for pair in drawn.windows(2) {
        if let [a, b] = pair {
            for r in [Rect::from_min_max(pos2(a.max.x - 5.0, a.min.y), a.max), Rect::from_min_max(b.min, pos2(b.min.x + 5.0, b.max.y))] {
                ui.painter().rect_filled(r, CornerRadius::ZERO, t.field);
            }
        }
    }
    out.map(|c| (c, done))
}

/// One field of a [`Polar`] model: its id, decimals shown, scrub/arrow step and range (0..=max,
/// or a hue that wraps at 360).
struct PolarField {
    id: &'static str,
    decimals: i32,
    step: f64,
    max: f64,
    wraps: bool,
}

const fn field(id: &'static str, decimals: i32, step: f64, max: f64) -> PolarField {
    PolarField { id, decimals, step, max, wraps: false }
}

const fn hue(id: &'static str) -> PolarField {
    PolarField { id, decimals: 0, step: 1.0, max: 360.0, wraps: true }
}

/// The three-field models: HSL, HSB, LCH and OKLCH, shown in their CSS units (percentages for
/// saturation and lightness, OKLCH lightness in percent).
#[derive(Clone, Copy)]
enum Polar {
    Hsl,
    Hsb,
    Lch,
    Oklch,
}

impl Polar {
    fn of(name: &str) -> Option<Self> {
        Some(match name {
            "HSL" => Self::Hsl,
            "HSB" => Self::Hsb,
            "LCH" => Self::Lch,
            "OKLCH" => Self::Oklch,
            _ => return None,
        })
    }

    const HSL_FIELDS: [PolarField; 3] = [hue("hsl_h"), field("hsl_s", 0, 1.0, 100.0), field("hsl_l", 0, 1.0, 100.0)];
    const HSB_FIELDS: [PolarField; 3] = [hue("hsb_h"), field("hsb_s", 0, 1.0, 100.0), field("hsb_b", 0, 1.0, 100.0)];
    const LCH_FIELDS: [PolarField; 3] = [field("lch_l", 0, 1.0, 100.0), field("lch_c", 0, 1.0, 230.0), hue("lch_h")];
    const OKLCH_FIELDS: [PolarField; 3] = [field("oklch_l", 0, 1.0, 100.0), field("oklch_c", 2, 0.01, 0.5), hue("oklch_h")];

    const fn fields(self) -> &'static [PolarField; 3] {
        match self {
            Self::Hsl => &Self::HSL_FIELDS,
            Self::Hsb => &Self::HSB_FIELDS,
            Self::Lch => &Self::LCH_FIELDS,
            Self::Oklch => &Self::OKLCH_FIELDS,
        }
    }

    fn values(self, c: Color) -> [f64; 3] {
        let v = match self {
            Self::Hsl => {
                let x = figcraft_color::Hsl::from_color(c);
                [x.h, x.s * 100.0, x.l * 100.0]
            }
            Self::Hsb => {
                let x = Hsb::from_color(c);
                [x.h, x.s * 100.0, x.b * 100.0]
            }
            Self::Lch => {
                let x = figcraft_color::Lch::from_color(c);
                [x.l, x.c, x.h]
            }
            Self::Oklch => {
                let x = figcraft_color::Oklch::from_color(c);
                [x.l * 100.0, x.c, x.h]
            }
        };
        v.map(f64::from)
    }

    /// Opaque.
    fn color(self, v: [f64; 3]) -> Color {
        let [a, b, d] = v.map(|v| v as f32);
        match self {
            Self::Hsl => figcraft_color::Hsl { h: a, s: b / 100.0, l: d / 100.0 }.to_color(),
            Self::Hsb => Hsb { h: a, s: b / 100.0, b: d / 100.0 }.to_color(),
            Self::Lch => figcraft_color::Lch { l: a, c: b, h: d }.to_color(),
            Self::Oklch => figcraft_color::Oklch { l: a / 100.0, c: b, h: d }.to_color(),
        }
    }
}

fn hue_color(ctx: &egui::Context, h: f32) -> Color32 {
    doc_c32(ctx, Hsb { h, s: 1.0, b: 1.0 }.to_color())
}

/// Draw the reticle: the saturation/brightness square (5-point corners) with its knob, updating
/// `hsb.s` and `hsb.b` on click or drag; returns `(changed, done)`. `fill` is the colour in the knob.
fn sv_square(ui: &mut Ui, rect: Rect, hsb: &mut Hsb, fill: Color32) -> (bool, bool) {
    let t = Tokens::get(ui.ctx());
    // White → hue horizontally, then transparent → black vertically.
    let mut m = Mesh::default();
    let hc = hue_color(ui.ctx(), hsb.h);
    let corners = [(rect.left_top(), Color32::WHITE), (rect.right_top(), hc), (rect.right_bottom(), hc), (rect.left_bottom(), Color32::WHITE)];
    for (p, c) in corners {
        m.colored_vertex(p, c);
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    ui.painter().add(m);
    let mut m2 = Mesh::default();
    for (p, c) in [
        (rect.left_top(), Color32::TRANSPARENT),
        (rect.right_top(), Color32::TRANSPARENT),
        (rect.right_bottom(), Color32::BLACK),
        (rect.left_bottom(), Color32::BLACK),
    ] {
        m2.colored_vertex(p, c);
    }
    m2.add_triangle(0, 1, 2);
    m2.add_triangle(0, 2, 3);
    ui.painter().add(m2);
    kit::round_corners(ui.painter(), rect, 5.0, t.panel);
    let resp = ui.interact(rect, Id::unique("cp_sv"), Sense::click_and_drag());
    let mut changed = false;
    if (resp.dragged() || resp.clicked() || resp.drag_started())
        && let Some(p) = resp.interact_pointer_pos()
    {
        hsb.s = ((p.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        hsb.b = 1.0 - ((p.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        changed = true;
    }
    let at = pos2(rect.min.x + hsb.s * rect.width(), rect.min.y + (1.0 - hsb.b) * rect.height());
    kit::knob(ui.painter(), at, fill, Color32::WHITE);
    (changed, resp.drag_stopped() || resp.clicked())
}

/// A slider track's travel: its knob's centre runs from 8 points inside one end to 8 inside the other.
fn track_span(rect: Rect) -> (f32, f32) {
    (rect.min.x + 8.0, rect.max.x - 8.0)
}

/// What the pointer did on a slider track: the position it set (0..=1) and whether the gesture ended.
fn slider_input(ui: &mut Ui, rect: Rect, id: Id, t: f32) -> Option<(f32, bool)> {
    let (lo, hi) = track_span(rect);
    let resp = ui.interact(rect.expand2(vec2(0.0, 4.0)), id, Sense::click_and_drag());
    if (resp.dragged() || resp.clicked() || resp.drag_started())
        && let Some(p) = resp.interact_pointer_pos()
    {
        return Some((((p.x - lo) / (hi - lo)).clamp(0.0, 1.0), resp.drag_stopped() || resp.clicked()));
    }
    if resp.drag_stopped() {
        return Some((t, true));
    }
    None
}

/// Draw the hue gradient (a pill) and knob, updating `hsb.h` clamped below 360; returns
/// `(changed, done)`.
fn hue_slider(ui: &mut Ui, rect: Rect, hsb: &mut Hsb) -> (bool, bool) {
    let t = Tokens::get(ui.ctx());
    let (lo, hi) = track_span(rect);
    let n = 12;
    let mut m = Mesh::default();
    let mut stops: Vec<(f32, f32)> = vec![(rect.min.x, 0.0)];
    stops.extend((0..=n).map(|i| (lo + (hi - lo) * i as f32 / n as f32, i as f32 * 360.0 / n as f32)));
    stops.push((rect.max.x, 360.0));
    for (i, (x, h)) in stops.iter().enumerate() {
        let c = hue_color(ui.ctx(), *h);
        m.colored_vertex(pos2(*x, rect.min.y), c);
        m.colored_vertex(pos2(*x, rect.max.y), c);
        if i > 0 {
            let k = (i * 2) as u32;
            m.add_triangle(k - 2, k - 1, k);
            m.add_triangle(k - 1, k, k + 1);
        }
    }
    ui.painter().add(m);
    kit::round_corners(ui.painter(), rect, rect.height() / 2.0, t.panel);
    let at = pos2(lo + (hi - lo) * (hsb.h / 360.0).clamp(0.0, 1.0), rect.center().y);
    let moved = slider_input(ui, rect, Id::unique("cp_hue"), hsb.h / 360.0).map(|(v, done)| {
        hsb.h = (v * 360.0).clamp(0.0, 359.9);
        (true, done)
    });
    kit::knob(ui.painter(), at, hue_color(ui.ctx(), hsb.h), Color32::WHITE);
    moved.unwrap_or((false, false))
}

/// Draw the checkerboard and transparent-to-opaque alpha gradient with a knob; returns the
/// new alpha and done state, or `None` when the slider is not interacting.
fn alpha_slider(ui: &mut Ui, rect: Rect, color: Color) -> Option<(f32, bool)> {
    let t = Tokens::get(ui.ctx());
    let (lo, hi) = track_span(rect);
    widgets::checker(&ui.painter().with_clip_rect(rect), rect, 4.0);
    let mut m = Mesh::default();
    let opaque = doc_c32(ui.ctx(), color.with_alpha(1.0));
    let clear = opaque.gamma_multiply(0.0);
    for (x, c) in [(rect.min.x, clear), (lo, clear), (hi, opaque), (rect.max.x, opaque)] {
        m.colored_vertex(pos2(x, rect.min.y), c);
        m.colored_vertex(pos2(x, rect.max.y), c);
    }
    for k in [0u32, 2, 4] {
        m.add_triangle(k, k + 1, k + 2);
        m.add_triangle(k + 1, k + 2, k + 3);
    }
    ui.painter().add(m);
    kit::round_corners(ui.painter(), rect, rect.height() / 2.0, t.panel);
    let at = pos2(lo + (hi - lo) * color.a.clamp(0.0, 1.0), rect.center().y);
    let moved = slider_input(ui, rect, Id::unique("cp_alpha"), color.a);
    kit::knob(ui.painter(), at, Color32::WHITE, Color32::WHITE);
    moved
}

/// The image files the picker offers.
const IMAGE_FILES: [&str; 7] = ["png", "jpg", "jpeg", "webp", "gif", "tif", "tiff"];

fn image_pending_id() -> Id {
    Id::unique("cp_image_pending")
}

/// Whether the Image tab of `target` shows its placeholder: chosen, with no file yet.
fn image_pending(ctx: &egui::Context, target: &PickerTarget) -> bool {
    ctx.data(|d| d.get_temp::<String>(image_pending_id())).is_some_and(|k| k == format!("{target:?}"))
}

/// Show the Image tab's placeholder for `target`, or (`None`) for nothing.
fn set_image_pending(ctx: &egui::Context, target: Option<&PickerTarget>) {
    ctx.data_mut(|d| {
        d.remove::<String>(image_pending_id());
        if let Some(t) = target {
            d.insert_temp(image_pending_id(), format!("{t:?}"));
        }
    });
}

/// `image.replace`'s parameters for layers' paint `index`.
fn image_params(fills: bool, index: usize, ids: &[NodeId]) -> serde_json::Value {
    json!({"ids": ids, "target": if fills { "fill" } else { "stroke" }, "index": index})
}

/// Where a file dragged over the window would become the open picker's image, and the parameters
/// that does it: the picker, while it shows layers' image paint or the Image tab's placeholder. A
/// platform doesn't say where a dragged file is until it can: an unknown position is the picker's.
pub(crate) fn image_drop_target(app: &FigApp, ctx: &egui::Context) -> Option<(Rect, serde_json::Value)> {
    let target = app.ui.picker.as_ref()?;
    let PickerTarget::Paint { fills, index, ids } = target else { return None };
    if !image_pending(ctx, target) && !matches!(current_paint(app, target)?.kind, PaintKind::Image(_)) {
        return None;
    }
    let rect = ctx.memory(|m| m.area_rect(window_id()))?;
    ctx.input(|i| i.pointer.hover_pos()).is_none_or(|p| rect.contains(p)).then(|| (rect, image_params(*fills, *index, ids)))
}

/// The bytes of a picked `file`: its `path`, or its base64 `data` where there are no paths.
fn picked_bytes(file: &serde_json::Value) -> Option<Vec<u8>> {
    use base64::Engine as _;
    match file["path"].as_str() {
        Some(path) => figcraft_engine::file_access::read_capped(path, figcraft_doc::ImageData::MAX_BYTES).ok(),
        None => base64::engine::general_purpose::STANDARD.decode(file["data"].as_str()?).ok(),
    }
}

/// Ask for an image file for `target`: layers' paints keep their fill mode, crop and position.
fn upload_image(app: &mut FigApp, target: &PickerTarget) {
    let target = target.clone();
    app.pick_file(tr("Choose image"), &IMAGE_FILES, true, move |app, file| {
        if let PickerTarget::Paint { fills, index, ids } = &target {
            let mut params = image_params(*fills, *index, ids);
            for key in ["path", "data"] {
                if let Some(v) = file.get(key) {
                    params[key] = v.clone();
                }
            }
            app.run("image.replace", params);
            return;
        }
        let Some(bytes) = picked_bytes(&file) else { return };
        let Some(hash) = app.session.doc_mut().ok().and_then(|d| figcraft_format_add(d, bytes)) else { return };
        if let Some(mut paint) = current_paint(app, &target) {
            paint.kind = Paint::image(hash, ImageScaleMode::Fill).kind;
            write(app, &target, &paint, false);
        }
    });
}

/// The image `hash` as a texture at most `side` pixels across, kept while it stays the one shown.
fn image_texture(app: &FigApp, ctx: &egui::Context, hash: &str, side: f64) -> Option<egui::TextureHandle> {
    let id = Id::unique("cp_image_texture");
    if let Some((_, tex)) = ctx.data(|d| d.get_temp::<(String, egui::TextureHandle)>(id)).filter(|(h, _)| h == hash) {
        return Some(tex);
    }
    let doc = app.session.doc().ok()?;
    let pix = figcraft_render::images::shown(hash, doc.images.get(hash), &ImageFilters::default(), doc.color_profile, side)?;
    let tex = ctx.load_texture("cp-image", crate::render_worker::screen_image(&pix, doc.color_profile), egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (hash.to_string(), tex.clone())));
    Some(tex)
}

/// The asset well of the Image tab: a checkerboard dimmed by half, or the paint's image, with the
/// blue upload button over it (over an image, while the pointer is on the well). A file dropped on
/// it takes the image's place (`image_drop_target`).
fn image_well(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &Paint, t: &Tokens) {
    let (block, _) = ui.allocate_exact_size(vec2(kit::W, 216.0), Sense::hover());
    let well = Rect::from_min_size(pos2(block.min.x + kit::INSET, block.min.y), vec2(208.0, 208.0));
    let id = Id::unique("cp_image_well");
    let hovered = ui.rect_contains_pointer(well);
    let clip = ui.painter().with_clip_rect(well);
    widgets::checker(&clip, well, 13.0);
    let ppp = f64::from(ui.ctx().pixels_per_point());
    let mut has_image = false;
    if let PaintKind::Image(img) = &paint.kind
        && let Some(tex) = image_texture(app, ui.ctx(), &img.image, f64::from(well.width()) * ppp)
    {
        has_image = true;
        let [tw, th] = tex.size().map(|v| v.max(1) as f32);
        let k = (well.width() / tw).min(well.height() / th);
        let shown = Rect::from_center_size(well.center(), vec2(tw * k, th * k));
        clip.image(tex.id(), shown, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
    }
    let label = tr("Upload from computer");
    let galley = ui.painter().layout_no_wrap(label.to_string(), regular(11.0), Color32::WHITE);
    let button = Rect::from_center_size(well.center(), vec2(galley.size().x + 16.0, 24.0));
    let resp = ui.interact(button, id, Sense::click());
    if !has_image || hovered {
        // The page's dimming over the well (the kit's modal backdrop).
        clip.rect_filled(well, CornerRadius::ZERO, Color32::from_black_alpha(128));
    }
    kit::round_corners(ui.painter(), well, 5.0, t.panel);
    // The upload button stays over an image too, so replacing it is one click.
    let fill = if resp.hovered() { crate::theme::ACCENT } else { crate::theme::BRAND };
    ui.painter().rect_filled(button, CornerRadius::same(5), fill);
    ui.painter().galley(pos2(button.min.x + 8.0, button.center().y - galley.size().y / 2.0), galley, Color32::WHITE);
    crate::a11y::button(&resp, label);
    if resp.clicked() {
        upload_image(app, target);
    }
}

/// Switch the paint to the kind the toolbar's `i`-th button stands for: solid, gradient, pattern or
/// image; video and shader fills are not something FigCraft has.
fn switch_kind(app: &mut FigApp, ui: &Ui, target: &PickerTarget, paint: &mut Paint, i: usize) {
    if i >= 4 {
        let what = if i == 4 { tr("Video fills aren't available yet") } else { tr("Shader fills aren't available yet") };
        app.toast(what.to_string());
        return;
    }
    let base = paint.preview_color();
    let choosing = i == 3;
    set_image_pending(ui.ctx(), choosing.then_some(target).filter(|_| !matches!(paint.kind, PaintKind::Image(_))));
    if i == 1 && matches!(paint.kind, PaintKind::Gradient(_)) {
        return;
    }
    paint.kind = match i {
        0 => PaintKind::Solid { color: base.with_alpha(1.0) },
        // Nothing changes until there is a file: the tab shows the placeholder.
        3 => paint.kind.clone(),
        2 => match pattern_sources(app, target).first() {
            Some((id, _)) => Paint::pattern(id.0).kind,
            None => {
                app.toast(tr("Add a layer or component to tile first").to_string());
                paint.kind.clone()
            }
        },
        _ => PaintKind::Gradient(Gradient::default_for(GradientKind::Linear, base)),
    };
    if !choosing {
        write(app, target, paint, false);
    }
}

/// How `color` reads against the page background, as a note (the kit's contrast check).
fn contrast_note(app: &FigApp, color: Color) -> String {
    let bg = app
        .session
        .page()
        .ok()
        .and_then(|p| app.session.doc().ok()?.node(p).map(|n| n.kind.clone()))
        .and_then(|k| if let NodeKind::Page { background, .. } = k { Some(background) } else { None })
        .unwrap_or(Color::DEFAULT_PAGE);
    let ratio = color.with_alpha(1.0).contrast(bg.with_alpha(1.0));
    let grade = match ratio {
        r if r >= 7.0 => tr("AAA"),
        r if r >= 4.5 => tr("AA"),
        r if r >= 3.0 => tr("AA for large text only"),
        _ => tr("fails WCAG"),
    };
    trf("Contrast with the page background: {ratio}:1, {grade}", &[("ratio", &format!("{ratio:.1}")), ("grade", &grade)])
}

/// The kit's toolbar: the paint kinds at the left, the blend mode (and, over a solid colour, the
/// contrast check) at the right, a hairline under it.
fn toolbar(ui: &mut Ui, app: &mut FigApp, target: &PickerTarget, paint: &mut Paint, solid_color: Option<Color>) {
    let t = Tokens::get(ui.ctx());
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 41.0), Sense::hover());
    ui.painter().rect_filled(Rect::from_min_max(pos2(row.min.x, row.max.y - 1.0), row.max), CornerRadius::ZERO, t.border);
    let y = row.min.y + 8.0;
    let kinds = [
        (Icon::PaintSolid, tr("Solid")),
        (Icon::PaintGradient, tr("Gradient")),
        (Icon::PaintPattern, tr("Pattern")),
        (Icon::PaintImage, tr("Image")),
        (Icon::PaintVideo, tr("Video")),
        (Icon::PaintNoise, tr("Shader")),
    ];
    let active = match &paint.kind {
        _ if image_pending(ui.ctx(), target) => 3,
        PaintKind::Solid { .. } => 0,
        PaintKind::Gradient(_) => 1,
        PaintKind::Pattern(_) => 2,
        PaintKind::Image(_) => 3,
    };
    for (i, (icon, name)) in kinds.iter().enumerate() {
        let r = Rect::from_min_size(pos2(row.min.x + 8.0 + 28.0 * i as f32, y), vec2(24.0, 24.0));
        if kit::icon_button(ui, r, Id::unique(("cp_kind", i)), *icon, i == active, name).clicked() {
            switch_kind(app, ui, target, paint, i);
        }
    }
    let mut x = row.max.x - 8.0 - 24.0;
    if let Some(color) = solid_color {
        let r = Rect::from_min_size(pos2(x, y), vec2(24.0, 24.0));
        if kit::icon_button(ui, r, Id::unique("cp_contrast"), Icon::Contrast, false, tr("Check color contrast")).clicked() {
            let note = contrast_note(app, color);
            app.toast(note);
        }
        x -= 28.0;
    }
    let resp = kit::icon_button(ui, Rect::from_min_size(pos2(x, y), vec2(24.0, 24.0)), Id::unique("cp_blend"), Icon::Blend, false, tr("Blend mode"));
    crate::right::menu_popup(&resp).width(200.0).show(|ui| {
        let ctx = ui.ctx().clone();
        if let Some(b) = crate::right::blend_menu(ui, false, |b| preview_blend(app, &ctx, target, paint, b)) {
            paint.blend = b;
            write(app, target, paint, false);
        }
    });
}

/// The image paint's colour profile, presets and filter sliders (below the well).
fn image_extras(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, img: ImagePaint, t: &Tokens) {
    // The image's own colour profile (decoding converts it into the file's).
    let source = app.session.doc().ok().and_then(|d| figcraft_render::images::profile_name(&img.image, d.images.get(&img.image)));
    let source = match source.as_deref() {
        None => tr("Unknown — treated as sRGB"),
        Some("") => tr("Embedded profile"),
        Some(name) => name,
    };
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    let mut img = img;
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let text = trf("Color profile: {name}", &[("name", &source)]);
        ui.add(egui::Label::new(egui::RichText::new(text).font(regular(11.0)).color(t.text_secondary)).truncate());
    });
    image_presets(app, ui, target, paint, img.filters);
    for (label, val) in [
        (tr_noop("Exposure"), img.filters.exposure),
        (tr_noop("Contrast"), img.filters.contrast),
        (tr_noop("Saturation"), img.filters.saturation),
        (tr_noop("Temperature"), img.filters.temperature),
        (tr_noop("Tint"), img.filters.tint),
        (tr_noop("Highlights"), img.filters.highlights),
        (tr_noop("Shadows"), img.filters.shadows),
    ] {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            // Labels share a column so the sliders line up.
            let (lr, _) = ui.allocate_exact_size(vec2(72.0, 18.0), Sense::hover());
            ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, tr(label), regular(11.0), t.text);
            let mut v = val;
            let r = ui.add(egui::Slider::new(&mut v, -1.0..=1.0).show_value(false));
            if r.changed() {
                let f = &mut img.filters;
                match label {
                    "Exposure" => f.exposure = v,
                    "Contrast" => f.contrast = v,
                    "Saturation" => f.saturation = v,
                    "Temperature" => f.temperature = v,
                    "Tint" => f.tint = v,
                    "Highlights" => f.highlights = v,
                    _ => f.shadows = v,
                }
                paint.kind = PaintKind::Image(img.clone());
                write(app, target, paint, !r.drag_stopped());
            }
            if r.drag_stopped() {
                finish(app);
            }
        });
    }
    ui.add_space(8.0);
}

/// The Image tab: the fill mode and a 90° turn above the asset well.
fn image_section(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, t: &Tokens) {
    let img = if let PaintKind::Image(i) = &paint.kind { Some(i.clone()) } else { None };
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 48.0), Sense::hover());
    let label = img.as_ref().map_or_else(|| tr("Fill").to_string(), |i| tr(&format!("{:?}", i.scale_mode)).to_string());
    let button = Rect::from_min_size(pos2(row.min.x + kit::INSET, row.center().y - 12.0), vec2(96.0, 24.0));
    let drop = kit::outlined_dropdown(ui, button, Id::unique("cp_fill_mode"), &label);
    if let Some(mut im) = img.clone() {
        crate::right::menu_popup(&drop).show(|ui| {
            for (l, m) in [
                (tr("Fill"), ImageScaleMode::Fill),
                (tr("Fit"), ImageScaleMode::Fit),
                (tr("Crop"), ImageScaleMode::Crop),
                (tr("Tile"), ImageScaleMode::Tile),
            ] {
                if crate::right::menu_item(ui, l, "") {
                    im.scale_mode = m;
                    paint.kind = PaintKind::Image(im.clone());
                    write(app, target, paint, false);
                    ui.close();
                }
            }
        });
    }
    let turn = Rect::from_min_size(pos2(row.max.x - 8.0 - 24.0, row.center().y - 12.0), vec2(24.0, 24.0));
    if kit::icon_button(ui, turn, Id::unique("cp_image_turn"), Icon::RotateGradient, false, tr("Rotate 90°")).clicked()
        && let Some(mut im) = img.clone()
    {
        im.rotation = (im.rotation + 1) % 4;
        paint.kind = PaintKind::Image(im);
        write(app, target, paint, false);
    }
    image_well(app, ui, target, paint, t);
    if let Some(im) = img {
        image_extras(app, ui, target, paint, im, t);
    }
}

/// Whether the gradient's colour area (for the selected stop) is open (temp data).
fn editor_id() -> Id {
    Id::unique("cp_stop_editor")
}

/// The gradient bar with its stops, the kind and the stop list. Returns the selected stop's colour
/// (what the colour fields then edit).
fn gradient_section(
    app: &mut FigApp,
    ui: &mut Ui,
    target: &PickerTarget,
    paint: &mut Paint,
    mut g: Gradient,
    t: &Tokens,
    sel_stop_id: Id,
) -> Option<Color> {
    let mut sel: usize = ui.data(|d| d.get_temp(sel_stop_id)).unwrap_or(0).min(g.stops.len().saturating_sub(1));
    let mut changed = false;
    let mut done = false;
    // The kind, and flip / rotate.
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 48.0), Sense::hover());
    let kinds = [
        (GradientKind::Linear, tr("Linear")),
        (GradientKind::Radial, tr("Radial")),
        (GradientKind::Angular, tr("Angular")),
        (GradientKind::Diamond, tr("Diamond")),
    ];
    let current = kinds.iter().find(|(k, _)| *k == g.kind).map_or("", |(_, l)| *l);
    let drop = kit::outlined_dropdown(
        ui,
        Rect::from_min_size(pos2(row.min.x + kit::INSET, row.center().y - 12.0), vec2(96.0, 24.0)),
        Id::unique("cp_grad_kind"),
        current,
    );
    let mut new_kind = None;
    crate::right::menu_popup(&drop).show(|ui| {
        for (k, l) in kinds {
            if crate::right::menu_check(ui, k == g.kind, l, "") {
                new_kind = Some(k);
                ui.close();
            }
        }
    });
    if let Some(kind) = new_kind {
        g = Gradient { kind, ..Gradient::default_for(kind, paint.preview_color()).with_stops(g.stops.clone()) };
        changed = true;
        done = true;
    }
    let rotate = Rect::from_min_size(pos2(row.max.x - 8.0 - 24.0, row.center().y - 12.0), vec2(24.0, 24.0));
    if kit::icon_button(ui, rotate.translate(vec2(-28.0, 0.0)), Id::unique("cp_grad_flip"), Icon::FlipGradient, false, tr("Flip")).clicked() {
        for s in &mut g.stops {
            s.position = 1.0 - s.position;
        }
        changed = true;
        done = true;
    }
    if kit::icon_button(ui, rotate, Id::unique("cp_grad_rotate"), Icon::RotateGradient, false, tr("Rotate 90°")).clicked() {
        let (fx, fy) = g.from;
        let rot = |(x, y): (f64, f64)| (fx - (y - fy), fy + (x - fx));
        g.to = rot(g.to);
        g.width = rot(g.width);
        changed = true;
        done = true;
    }
    // The bar, 32 high, with the stops hanging over it.
    let (block, _) = ui.allocate_exact_size(vec2(kit::W, 48.0), Sense::hover());
    let bar = Rect::from_min_size(pos2(block.min.x + kit::INSET, block.min.y + 16.0), vec2(208.0, 32.0));
    widgets::checker(&ui.painter().with_clip_rect(bar), bar, 8.0);
    let n = 52;
    for i in 0..n {
        let a = i as f32 / n as f32;
        let b = (i + 1) as f32 / n as f32;
        let r = Rect::from_min_max(pos2(bar.min.x + a * bar.width(), bar.min.y), pos2(bar.min.x + b * bar.width(), bar.max.y));
        ui.painter().rect_filled(r, CornerRadius::ZERO, doc_c32(ui.ctx(), g.sample((a + b) / 2.0)));
    }
    kit::round_corners(ui.painter(), bar, 5.0, t.panel);
    // The bar first, so the stops on it take their own clicks.
    let bresp = ui.interact(bar, Id::unique("cp_bar"), Sense::click());
    let mut order: Vec<usize> = (0..g.stops.len()).collect();
    order.sort_by_key(|a| a == &sel);
    for i in order {
        let Some(s) = g.stops.get(i).cloned() else { continue };
        let x = bar.min.x + s.position * bar.width();
        let r = Rect::from_min_size(pos2(x - 12.0, bar.min.y - 16.0), vec2(24.0, 24.0));
        let resp = ui.interact(r.expand2(vec2(0.0, 3.0)), Id::unique(("cp_stopk", i)), Sense::click_and_drag());
        let fill = if i == sel { crate::theme::BRAND } else { t.field };
        let p = ui.painter();
        p.rect_filled(r.translate(vec2(0.0, 1.0)).expand(0.5), CornerRadius::same(6), Color32::from_black_alpha(38));
        p.add(egui::Shape::convex_polygon(
            vec![pos2(x - 6.0, r.max.y - 1.0), pos2(x + 6.0, r.max.y - 1.0), pos2(x, r.max.y + 6.0)],
            fill,
            Stroke::NONE,
        ));
        p.rect_filled(r, CornerRadius::same(5), fill);
        kit::paint_swatch(ui, Rect::from_center_size(r.center(), vec2(14.0, 14.0)), &Paint::solid(s.color));
        if resp.clicked() || resp.drag_started() {
            sel = i;
        }
        if resp.dragged()
            && let Some(p) = resp.interact_pointer_pos()
            && let Some(st) = g.stops.get_mut(i)
        {
            st.position = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
            changed = true;
        }
        if resp.drag_stopped() {
            done = true;
        }
    }
    // A click on the bar adds a stop.
    if bresp.clicked()
        && let Some(p) = bresp.interact_pointer_pos()
    {
        let pos = ((p.x - bar.min.x) / bar.width()).clamp(0.0, 1.0);
        g.stops.push(ColorStop { position: pos, color: g.sample(pos) });
        sel = g.stops.len() - 1;
        changed = true;
        done = true;
    }
    // "Stops" and the add button.
    let (head, _) = ui.allocate_exact_size(vec2(kit::W, 40.0), Sense::hover());
    ui.painter().text(pos2(head.min.x + kit::INSET, head.center().y + 4.0), Align2::LEFT_CENTER, tr("Stops"), crate::theme::semibold(11.0), t.text);
    let add = Rect::from_min_size(pos2(head.max.x - 8.0 - 24.0, head.center().y - 8.0), vec2(24.0, 24.0));
    if kit::icon_button(ui, add, Id::unique("cp_stop_add"), Icon::Plus, false, tr("Add stop")).clicked() {
        // In the middle of the widest gap between stops.
        let mut at: Vec<f32> = g.stops.iter().map(|s| s.position).collect();
        at.push(0.0);
        at.push(1.0);
        at.sort_by(f32::total_cmp);
        let pos = at.windows(2).map(|w| (w[1] - w[0], (w[0] + w[1]) / 2.0)).max_by(|a, b| a.0.total_cmp(&b.0)).map_or(0.5, |(_, m)| m);
        g.stops.push(ColorStop { position: pos, color: g.sample(pos) });
        sel = g.stops.len() - 1;
        changed = true;
        done = true;
    }
    // The stops, left to right.
    ui.add_space(4.0);
    let mut order: Vec<usize> = (0..g.stops.len()).collect();
    order.sort_by(|a, b| g.stops.get(*a).zip(g.stops.get(*b)).map_or(std::cmp::Ordering::Equal, |(x, y)| x.position.total_cmp(&y.position)));
    let can_remove = g.stops.len() > 2;
    let mut remove = None;
    for i in order {
        let Some(stop) = g.stops.get(i).cloned() else { continue };
        let (rr, _) = ui.allocate_exact_size(vec2(kit::W, 32.0), Sense::hover());
        let row_resp = ui.interact(rr, Id::unique(("cp_stop_row", i)), Sense::click());
        if i == sel {
            ui.painter().rect_filled(rr, CornerRadius::ZERO, t.selected_row);
        }
        if row_resp.clicked() {
            sel = i;
        }
        let y = rr.center().y - 12.0;
        // Position.
        let pos_rect = Rect::from_min_size(pos2(rr.min.x + kit::INSET, y), vec2(48.0, 24.0));
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(pos_rect));
        let pos = widgets::number_field(
            &mut cell,
            Id::unique(("cp_stop_pos", i)),
            widgets::Lead::None,
            Some((f64::from(stop.position) * 100.0).round()),
            "%",
            48.0,
            1.0,
        );
        if let Some(e) = pos {
            let (v, end) = match e {
                widgets::FieldEdit::Set(v) => (v, true),
                widgets::FieldEdit::Scrub(v, end) => (v, end),
            };
            if let Some(st) = g.stops.get_mut(i) {
                st.position = (v / 100.0).clamp(0.0, 1.0) as f32;
            }
            sel = i;
            changed = true;
            done = end;
        }
        // Colour (a variable's chip while one is bound), then its opacity.
        let color_rect = Rect::from_min_size(pos2(rr.min.x + 72.0, y), vec2(128.0, 24.0));
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(color_rect));
        let mut edit_stop = |app_ui: &mut Ui| {
            let sw = Rect::from_min_size(pos2(color_rect.min.x + 4.0, y + 4.0), vec2(16.0, 16.0));
            if kit::swatch(app_ui, sw, Id::unique(("cp_stop_sw", i)), &Paint::solid(stop.color), tr("Color")).clicked() {
                // The stop's swatch opens (or closes) the colour area under the list.
                let open = app_ui.data(|d| d.get_temp::<bool>(editor_id())).unwrap_or(false);
                app_ui.data_mut(|d| d.insert_temp(editor_id(), !(open && sel == i)));
                sel = i;
            }
            let mut hex_cell =
                app_ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(color_rect.min.x + 24.0, y), vec2(56.0, 24.0))));
            if let Some(text) = widgets::text_field(&mut hex_cell, Id::unique(("cp_stop_hex", i)), &stop.color.to_hex(), 56.0, "")
                && let Some(c) = Color::from_hex_or_name(&text)
                && let Some(st) = g.stops.get_mut(i)
            {
                st.color = c.with_alpha(st.color.a);
                sel = i;
                changed = true;
                done = true;
            }
            let mut alpha_cell =
                app_ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(color_rect.max.x - 48.0, y), vec2(48.0, 24.0))));
            let dim = Id::unique("dim_percent");
            alpha_cell.data_mut(|d| d.insert_temp(dim, true));
            let a = widgets::number_field(
                &mut alpha_cell,
                Id::unique(("cp_stop_a", i)),
                widgets::Lead::None,
                Some((f64::from(stop.color.a) * 100.0).round()),
                "%",
                48.0,
                1.0,
            );
            alpha_cell.data_mut(|d| d.remove::<bool>(dim));
            if let Some(e) = a {
                let (v, end) = match e {
                    widgets::FieldEdit::Set(v) => (v, true),
                    widgets::FieldEdit::Scrub(v, end) => (v, end),
                };
                if let Some(st) = g.stops.get_mut(i) {
                    st.color = st.color.with_alpha((v / 100.0).clamp(0.0, 1.0) as f32);
                }
                sel = i;
                changed = true;
                done = end;
            }
        };
        if let PickerTarget::Paint { fills, index, ids } = target {
            let field = format!("{}/{index}/stops/{i}/color", if *fills { "fills" } else { "strokes" });
            crate::variable_ui::bindable(app, &mut cell, ids, &field, figcraft_doc::VariableType::Color, 128.0, |_, ui| edit_stop(ui));
        } else {
            edit_stop(&mut cell);
        }
        // Delete.
        let minus = Rect::from_min_size(pos2(rr.max.x - 8.0 - 24.0, y), vec2(24.0, 24.0));
        if kit::icon_button(ui, minus, Id::unique(("cp_stop_del", i)), Icon::Minus, false, tr("Remove stop")).clicked() {
            if can_remove {
                remove = Some(i);
            } else {
                app.toast(tr("A gradient needs at least two stops").to_string());
            }
        }
    }
    if let Some(i) = remove {
        g.stops.remove(i);
        sel = 0;
        changed = true;
        done = true;
    }
    ui.add_space(12.0);
    let sel = sel.min(g.stops.len().saturating_sub(1));
    ui.data_mut(|d| d.insert_temp(sel_stop_id, sel));
    let edit_color = g.stops.get(sel).map(|s| s.color);
    if changed {
        paint.kind = PaintKind::Gradient(g);
        write(app, target, paint, !done);
    }
    if done {
        finish(app);
    }
    edit_color
}

/// The kit's colour area for `color`: the reticle, the eyedropper with the hue and alpha sliders,
/// and the value row. Returns true when the eyedropper took over (the rest of the frame is skipped).
fn color_editor(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, color: Color, t: &Tokens, sel_stop_id: Id) -> bool {
    let mut hsb = ui
        .data(|d| d.get_temp::<(f32, f32, f32, [u8; 4])>(Id::unique("cp_hsb")))
        .filter(|(_, _, _, rgba)| *rgba == color.to_rgba8())
        .map(|(h, s, b, _)| Hsb { h, s, b })
        .unwrap_or_else(|| Hsb::from_color(color));
    let (block, _) = ui.allocate_exact_size(vec2(kit::W, 224.0), Sense::hover());
    let square = Rect::from_min_size(pos2(block.min.x + kit::INSET, block.min.y + kit::INSET), vec2(208.0, 208.0));
    let knob_fill = doc_c32(ui.ctx(), hsb.to_color());
    let (c1, d1) = sv_square(ui, square, &mut hsb, knob_fill);
    // The eyedropper at the left, the hue and alpha sliders beside it.
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 60.0), Sense::hover());
    let pick = Rect::from_min_size(pos2(row.min.x + kit::INSET, row.min.y + 22.0), vec2(24.0, 24.0));
    if kit::icon_button(ui, pick, Id::unique("cp_pick"), Icon::Eyedropper, false, tr("Pick color (I)")).clicked() {
        app.run("edit.pickColor", json!({"apply": false}));
        return true;
    }
    let hue_rect = Rect::from_min_size(pos2(row.min.x + 52.0, row.min.y + 12.0), vec2(172.0, 16.0));
    let alpha_rect = Rect::from_min_size(pos2(row.min.x + 52.0, row.min.y + 40.0), vec2(172.0, 16.0));
    let (c2, d2) = hue_slider(ui, hue_rect, &mut hsb);
    let new_alpha = alpha_slider(ui, alpha_rect, color);
    let mut nc = hsb.to_color().with_alpha(color.a);
    let mut done = d1 || d2;
    let mut changed = c1 || c2;
    if let Some((a, d)) = new_alpha {
        nc = nc.with_alpha(a);
        changed = true;
        done |= d;
    }
    // Colour model fields (Hex, RGB, HSL, HSB, LCH, OKLCH or CSS) + alpha.
    if let Some((c, d)) = model_fields(ui, nc) {
        nc = c;
        hsb = Hsb::from_color(nc);
        changed = true;
        done |= d;
    }
    ui.data_mut(|d| d.insert_temp(Id::unique("cp_hsb"), (hsb.h, hsb.s, hsb.b, nc.to_rgba8())));
    // A Display P3 colour sRGB can't show: the canvas (sRGB) shows the nearest one.
    if app.session.doc().is_ok_and(|d| d.color_profile.conversion_to(figcraft_color::ColorProfile::Srgb).clips(nc)) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            gamut_warning(ui, t);
        });
    }
    ui.add_space(8.0);
    if changed {
        match &mut paint.kind {
            PaintKind::Solid { color } => {
                *color = nc.with_alpha(1.0);
                paint.opacity = nc.a;
            }
            PaintKind::Gradient(g) => {
                let sel: usize = ui.data(|d| d.get_temp(sel_stop_id)).unwrap_or(0);
                if let Some(s) = g.stops.get_mut(sel) {
                    s.color = nc;
                }
            }
            PaintKind::Image(_) | PaintKind::Pattern(_) => {}
        }
        write(app, target, paint, !done);
    }
    if done {
        finish(app);
    }
    false
}

/// The swatch set under the colour area: a hairline, the set's menu ("On this page" or "Recent"),
/// and its swatches, 16 across 24 apart, three rows showing.
fn swatch_section(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, sel_stop_id: Id, t: &Tokens) {
    let page = page_colors(app, ui);
    let recent: Vec<Color> = app.session.recent_colors.clone();
    if page.is_empty() && recent.is_empty() {
        return;
    }
    let (head, _) = ui.allocate_exact_size(vec2(kit::W, 36.0), Sense::hover());
    ui.painter().rect_filled(Rect::from_min_size(head.min, vec2(kit::W, 1.0)), CornerRadius::ZERO, t.border);
    let set_id = Id::unique("cp_swatch_set");
    let mut set: usize = ui.data(|d| d.get_temp(set_id)).unwrap_or(0);
    if set == 0 && page.is_empty() {
        set = 1;
    } else if set == 1 && recent.is_empty() {
        set = 0;
    }
    let label = if set == 0 { tr("On this page") } else { tr("Recent") };
    let drop = kit::outlined_dropdown(
        ui,
        Rect::from_min_size(pos2(head.min.x + kit::INSET, head.min.y + 12.0), vec2(208.0, 24.0)),
        Id::unique("cp_swatch_set_btn"),
        label,
    );
    crate::right::menu_popup(&drop).show(|ui| {
        for (i, name) in [tr("On this page"), tr("Recent")].into_iter().enumerate() {
            if crate::right::menu_check(ui, set == i, name, "") {
                ui.data_mut(|d| d.insert_temp(set_id, i));
                ui.close();
            }
        }
    });
    let (area, _) = ui.allocate_exact_size(vec2(kit::W, 88.0), Sense::hover());
    let view = Rect::from_min_size(pos2(area.min.x + 12.0, area.min.y + 4.0), vec2(216.0, 84.0));
    let list = if set == 0 { page } else { recent };
    let mut scroll = ui.new_child(egui::UiBuilder::new().max_rect(view));
    egui::ScrollArea::vertical().id_salt("cp_swatches").max_height(84.0).auto_shrink([false, false]).show(&mut scroll, |ui| {
        let rows = list.len().div_ceil(9);
        let (grid, _) = ui.allocate_exact_size(vec2(216.0, 4.0 + kit::PITCH * rows as f32), Sense::hover());
        for (i, c) in list.iter().enumerate() {
            let at = pos2(grid.min.x + 4.0 + kit::PITCH * (i % 9) as f32, grid.min.y + 4.0 + kit::PITCH * (i / 9) as f32);
            let r = Rect::from_min_size(at, vec2(kit::SWATCH, kit::SWATCH));
            let resp = kit::swatch(ui, r, Id::unique(("cp_swatch", set, i)), &Paint::solid(*c), tr("Color")).on_hover_text(c.to_hex());
            if resp.clicked() {
                set_color(app, target, paint, *c, sel_stop_id, ui);
            }
        }
    });
}

/// The Libraries tab's "All libraries" filter: an outlined dropdown button in `rect`.
pub(crate) fn library_filter_button(ui: &mut Ui, rect: Rect, label: &str) -> egui::Response {
    kit::outlined_dropdown(ui, rect, Id::unique("cp_library_filter_btn"), label)
}

/// A 24-point icon button of the Libraries tab; true when clicked.
pub(crate) fn library_button(ui: &mut Ui, rect: Rect, id: Id, icon: Icon, tip: &str) -> bool {
    kit::icon_button(ui, rect, id, icon, false, tip).clicked()
}

/// The swatch of a style in the Libraries tab (a colour style's last paint).
pub(crate) fn library_swatch(ui: &Ui, rect: Rect, style: &figcraft_doc::Style) {
    if let figcraft_doc::StyleKind::Paint { paints } = &style.kind {
        let p = paints.last().cloned().unwrap_or_else(|| Paint::solid(Color::TRANSPARENT));
        kit::paint_swatch(ui, rect, &p);
    }
}

/// The swatch of a colour in the Libraries tab.
pub(crate) fn library_swatch_paint(ui: &Ui, rect: Rect, paint: &Paint) {
    kit::paint_swatch(ui, rect, paint);
}

/// Where the style picker's Custom tab returns: the picker it replaced (temp data).
pub fn return_id() -> Id {
    Id::unique("cp_return")
}

/// The colour picker window's area id. A fixed id (not `Window::new`'s, which egui keys by
/// `Id::unique(Some(title))`) so the style picker can tell a click inside the picker from one outside.
pub fn window_id() -> Id {
    Id::unique("color_picker")
}

/// Draw the colour picker for the current target: close it when there is no target or the
/// target's selection no longer matches, otherwise apply eyedropper, paint-kind, colour and
/// gradient-stop edits live, positioned beside a plugin field when applicable.
pub fn show(app: &mut FigApp, ctx: &egui::Context) {
    let Some(target) = app.ui.picker.clone() else {
        // Dev Mode's Copy colors: the sampled colour goes to the clipboard.
        if app.ui.dev.copy_colors {
            if let Some(c) = app.session.tool.picked.take() {
                let hex = c.to_css_hex();
                crate::services::egui_copy_text(ctx, &hex);
                app.session.message = Some(format!("Copied {hex}"));
                app.ui.dev.copy_colors = false;
            } else if app.session.tool.tool != figcraft_engine::ToolId::Eyedropper {
                app.ui.dev.copy_colors = false;
            }
            return;
        }
        // A colour sampled for a picker that has closed has no paint to go to.
        app.session.tool.picked = None;
        if app.session.tool.pick_only {
            app.run("tool.move", json!({}));
        }
        return;
    };
    // A layer's picker belongs to the selection it was opened for: once that changes, close it
    // rather than keep editing layers that are no longer selected.
    let stale = match &target {
        PickerTarget::Paint { ids, .. } | PickerTarget::Effect { ids, .. } | PickerTarget::MotionColor { ids, .. } => *ids != app.session.selection(),
        PickerTarget::TextRange { id, .. } => !app.session.selection().contains(id),
        _ => false,
    };
    let current = if stale { None } else { current_paint(app, &target) };
    let Some(mut paint) = current else {
        if matches!(target, PickerTarget::SelectionColor { .. }) {
            finish(app);
        }
        app.ui.picker = None;
        app.ui.picker_popover = None;
        return;
    };
    let t = Tokens::get(ctx);
    let screen = ctx.content_rect();
    // A plugin's colour field: beside its window, which sits where the picker otherwise would.
    let beside = match &target {
        PickerTarget::PluginField { plugin, .. } => crate::plugins_ui::field_window(app, ctx, plugin).map(|r| pos2(r.min.x - 248.0, r.min.y)),
        _ => None,
    };
    let open = beside.map(Open::At).unwrap_or_else(|| {
        let top = app.ui.picker_button.map_or(140.0, |r| r.min.y);
        let edge = app.ui.right_panel_rect.map_or(screen.max.x - app.ui.right_width, |r| r.min.x);
        Open::Anchor(pos2(edge - 8.0, top))
    });
    // The colour picker's own header — title 16 in, the X 8 in from the right — is the shared
    // popover's chrome now; the picker keeps its body. Its area id is [`window_id`], the one the
    // style picker looks up to tell a press in here from one outside.
    let mut pop = app.ui.picker_popover.take().unwrap_or_else(|| Popover::with_id(window_id(), tr("Custom")).pad(0.0));
    // Sampling a colour from the canvas — the eyedropper this picker's own button started, or the
    // colour it just sampled — is the press, not a press outside: neither may close the picker.
    let picking = app.session.tool.pick_only || app.session.tool.picked.is_some();
    // Nor may a press on the open gradient's handles or stops on the canvas: they are this picker's
    // controls, and the canvas took the press before the picker drew (#1852).
    let own = picking || crate::handles::gradient_dragging(ctx);
    pop = pop.close_on_outside(!own);
    // Only the opening control is held out: any other sidebar or canvas press dismisses it.
    let mut held: Vec<Rect> = app.ui.picker_button.into_iter().collect();
    held.extend(crate::variable_ui::picker_rect(ctx));
    // The header's tabs and "+": a layer's paint can switch to the file's styles and variables
    // (the style picker) or make a style from itself.
    let layer_paint = matches!(target, PickerTarget::Paint { .. });
    let tabs = if layer_paint { vec![tr("Custom").to_string(), tr("Libraries").to_string()] } else { vec![tr("Custom").to_string()] };
    pop.header_tabs(tabs, 0, layer_paint.then(|| tr("New style").to_string()));
    let closed = pop.show(ctx, open, Hold { rects: &held, escape: false }, |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
        // The eyedropper's colour goes to this picker's paint (a stroke, an effect, a gradient stop...).
        if let Some(c) = app.session.tool.picked.take() {
            set_color(app, &target, &mut paint, c, stop_id(), ui);
        }
        let has_toolbar = !matches!(
            target,
            PickerTarget::Effect { .. } | PickerTarget::PageBackground | PickerTarget::Variable { .. } | PickerTarget::PluginField { .. }
        );
        let is_image = matches!(paint.kind, PaintKind::Image(_));
        if is_image {
            set_image_pending(ui.ctx(), None);
        }
        // Until a file is chosen the Image tab shows its placeholder, not the paint it will replace.
        let placeholder = image_pending(ui.ctx(), &target);
        let mut edit_color: Option<Color> = match &paint.kind {
            PaintKind::Solid { color } if !placeholder => Some(color.with_alpha(color.a * paint.opacity)),
            _ => None,
        };
        if has_toolbar {
            toolbar(ui, app, &target, &mut paint, edit_color);
        }
        let sel_stop_id = stop_id();
        if placeholder || is_image {
            image_section(app, ui, &target, &mut paint, &t);
        } else if let PaintKind::Gradient(g) = paint.kind.clone() {
            edit_color = gradient_section(app, ui, &target, &mut paint, g, &t, sel_stop_id);
            // The colour area stays shut until a stop's swatch opens it: the tab is the kit's.
            if !ui.data(|d| d.get_temp::<bool>(editor_id())).unwrap_or(false) {
                edit_color = None;
            }
            if edit_color.is_some() {
                let (line, _) = ui.allocate_exact_size(vec2(kit::W, 1.0), Sense::hover());
                ui.painter().rect_filled(line, CornerRadius::ZERO, t.border);
            }
        } else if let PaintKind::Pattern(pt) = paint.kind.clone() {
            pattern_section(app, ui, &target, &mut paint, pt, &t);
        }
        if let Some(color) = edit_color {
            if color_editor(app, ui, &target, &mut paint, color, &t, sel_stop_id) {
                return;
            }
            swatch_section(app, ui, &target, &mut paint, sel_stop_id, &t);
        }
    });
    if layer_paint {
        if pop.tab_clicked == Some(1) {
            ctx.data_mut(|d| d.insert_temp(return_id(), target.clone()));
            let slot = match &target {
                PickerTarget::Paint { fills: false, .. } => crate::style_picker::StyleSlot::Stroke,
                _ => crate::style_picker::StyleSlot::Fill,
            };
            set_image_pending(ctx, None);
            crate::style_picker::open_for_selection(app, slot, None);
            return;
        }
        if pop.plus_clicked {
            let slot = match &target {
                PickerTarget::Paint { fills: false, .. } => crate::style_picker::StyleSlot::Stroke,
                _ => crate::style_picker::StyleSlot::Fill,
            };
            set_image_pending(ctx, None);
            crate::style_picker::create_from_selection(app, slot);
            return;
        }
    }
    if closed {
        set_image_pending(ctx, None);
        close(app);
    } else if app.ui.picker.is_some() {
        // Kept only while the picker is open: a body that closed it (the eyedropper) doesn't leave
        // a stale place for the next picker to inherit.
        app.ui.picker_popover = Some(pop);
    }
}

/// Escape with the picker open closes it (the selection stays); false when it was not open, so the
/// key goes on to the binding that deselects.
pub fn close_if_open(app: &mut FigApp) -> bool {
    if app.ui.picker.is_none() {
        return false;
    }
    close(app);
    true
}

/// The colour picker closes (its X, a press outside it, Escape, or another popover took over):
/// pending edits commit and the picker with its popover go.
pub fn close(app: &mut FigApp) {
    finish(app);
    app.ui.picker = None;
    app.ui.picker_popover = None;
    app.ui.picker_button = None;
    app.ui.picker_button_id = None;
}

/// The colour picker's note under the fields for a colour outside sRGB.
fn gamut_warning(ui: &mut Ui, t: &Tokens) {
    use crate::a11y::Tip;
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        icons::paint(ui.painter(), r, Icon::GamutWarning, t.icon_secondary);
        widgets::label(ui, "Outside sRGB", t.text_secondary);
    })
    .response
    .tip("sRGB screens and files can't show this Display P3 color: the canvas shows the nearest sRGB color, P3 exports keep it");
}

/// Layers a pattern can tile: the file's components and the current page's top-level layers, but
/// the painted layers, what they contain and what contains them (at most 64).
fn pattern_sources(app: &FigApp, target: &PickerTarget) -> Vec<(NodeId, String)> {
    let Ok(doc) = app.session.doc() else { return Vec::new() };
    let painted = match target {
        PickerTarget::Paint { ids, .. } => ids.clone(),
        PickerTarget::SelectionColor { targets, .. } => targets.iter().map(|t| t.id).collect(),
        PickerTarget::TextRange { id, .. } => vec![*id],
        _ => Vec::new(),
    };
    let top = app.session.page().ok().and_then(|p| doc.node(p)).map(|p| p.children.clone()).unwrap_or_default();
    let comps = doc.nodes().filter(|n| matches!(n.kind, NodeKind::Component { .. })).map(|n| n.id);
    let mut out: Vec<(NodeId, String)> = Vec::new();
    for id in comps.chain(top) {
        let related = painted.iter().any(|t| doc.is_ancestor(*t, id) || doc.is_ancestor(id, *t));
        if !related
            && !out.iter().any(|(o, _)| *o == id)
            && let Some(n) = doc.node(id)
        {
            out.push((id, n.name.clone()));
        }
        if out.len() >= 64 {
            break;
        }
    }
    out
}

/// The pattern tab: the source in a grey well, then tile type, scale, spacing and alignment.
fn pattern_section(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, mut pt: PatternPaint, t: &Tokens) {
    use widgets::{FieldEdit, Lead};
    // Some(live): changed, as part of a drag when live.
    let mut changed: Option<bool> = None;
    let name = app.session.doc().ok().and_then(|d| d.node(NodeId(pt.source)).map(|n| n.name.clone())).unwrap_or_else(|| tr("Missing layer").into());
    let sources = pattern_sources(app, target);
    // The source well and the button that picks the source.
    let (block, _) = ui.allocate_exact_size(vec2(kit::W, 224.0), Sense::hover());
    let well = Rect::from_min_size(pos2(block.min.x + kit::INSET, block.min.y + 8.0), vec2(208.0, 208.0));
    ui.painter().rect_filled(well, CornerRadius::same(5), Color32::from_gray(0x80));
    ui.painter().rect_stroke(well, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
    let shown = widgets::elide(ui, &name, 130.0);
    let galley = ui.painter().layout_no_wrap(shown, regular(11.0), t.text);
    let button = Rect::from_center_size(well.center(), vec2(16.0 + 20.0 + galley.size().x, 24.0));
    let resp = ui.interact(button, Id::unique("cp_pattern_source"), Sense::click());
    crate::a11y::combo(&resp, tr("Pattern source"), &name, crate::a11y::menu_open(&resp));
    ui.painter().rect_filled(button.translate(vec2(0.0, 1.0)), CornerRadius::same(5), Color32::from_black_alpha(30));
    ui.painter().rect_filled(button, CornerRadius::same(5), if resp.hovered() { t.field } else { t.panel });
    icons::paint(ui.painter(), Rect::from_center_size(pos2(button.min.x + 18.0, button.center().y), vec2(16.0, 16.0)), Icon::SelectSource, t.icon);
    ui.painter().galley(pos2(button.min.x + 28.0, button.center().y - galley.size().y / 2.0), galley, t.text);
    crate::right::menu_popup(&resp).show(|ui| {
        for (id, label) in &sources {
            if crate::right::menu_check(ui, id.0 == pt.source, label, "") {
                pt.source = id.0;
                changed = Some(false);
                ui.close();
            }
        }
    });
    // A hairline, then the rows: a label at 16, the control from 88 on, 136 wide.
    let (line, _) = ui.allocate_exact_size(vec2(kit::W, 9.0), Sense::hover());
    ui.painter().rect_filled(Rect::from_min_size(pos2(line.min.x, line.min.y + 8.0), vec2(kit::W, 1.0)), CornerRadius::ZERO, t.border);
    ui.add_space(8.0);
    let caption = |ui: &Ui, row: Rect, text: &str| {
        ui.painter().text(pos2(row.min.x + kit::INSET, row.min.y + 16.0), Align2::LEFT_CENTER, text, regular(11.0), t.text_secondary);
    };
    let control = |row: Rect| Rect::from_min_size(pos2(row.min.x + 88.0, row.min.y + 4.0), vec2(136.0, 24.0));
    // Tile type: grid, horizontal or vertical hexagons.
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 32.0), Sense::hover());
    caption(ui, row, tr("Tile type"));
    let set = control(row);
    ui.painter().rect_filled(set, CornerRadius::same(5), t.field);
    let tiles = [
        (PatternTile::Grid, Icon::TileGrid, tr("Grid")),
        (PatternTile::HorizontalHex, Icon::TileHex, tr("Horizontal hex")),
        (PatternTile::VerticalHex, Icon::TileHexVertical, tr("Vertical hex")),
    ];
    let cell_w = set.width() / tiles.len() as f32;
    for (i, (tile, icon, label)) in tiles.iter().enumerate() {
        let r = Rect::from_min_size(pos2(set.min.x + cell_w * i as f32, set.min.y), vec2(cell_w, 24.0));
        let on = *tile == pt.tile;
        let resp = ui.interact(r, Id::unique(("cp_pattern_tile", i)), Sense::click());
        if on {
            ui.painter().rect_filled(r, CornerRadius::same(5), t.panel);
        } else if resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), *icon, t.icon);
        let resp = crate::a11y::Tip::tip(resp, label);
        if on {
            crate::a11y::toggle(&resp, label, true);
        }
        if resp.clicked() && !on {
            pt.tile = *tile;
            changed = Some(false);
        }
    }
    let field = |ui: &mut Ui, rect: Rect, key: &str, lead: Lead, value: f64, suffix: &str| {
        let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        match widgets::number_field(&mut cell, Id::unique(key), lead, Some(value), suffix, 136.0, 1.0) {
            Some(FieldEdit::Set(v)) => Some((v, false)),
            Some(FieldEdit::Scrub(v, end)) => Some((v, !end)),
            None => None,
        }
    };
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 32.0), Sense::hover());
    caption(ui, row, tr("Scale"));
    if let Some((v, live)) = field(ui, control(row), "cp_pattern_scale", Lead::Icon(Icon::Scale, tr("Pattern scale")), pt.scale * 100.0, "%") {
        let (lo, hi) = PatternPaint::SCALE_RANGE;
        pt.scale = (v / 100.0).clamp(lo, hi);
        changed = Some(live);
    }
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 32.0), Sense::hover());
    caption(ui, row, tr("Spacing"));
    if let Some((v, live)) = field(ui, control(row), "cp_pattern_gap_x", Lead::Text("X"), pt.spacing.0, "") {
        pt.spacing.0 = v.clamp(0.0, PatternPaint::MAX_SPACING);
        changed = Some(live);
    }
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 32.0), Sense::hover());
    if let Some((v, live)) = field(ui, control(row), "cp_pattern_gap_y", Lead::Text("Y"), pt.spacing.1, "") {
        pt.spacing.1 = v.clamp(0.0, PatternPaint::MAX_SPACING);
        changed = Some(live);
    }
    // Alignment: a 3 × 3 field. A pattern aligns at its start, centre or end, which are the
    // diagonal's cells; the others say so.
    let (row, _) = ui.allocate_exact_size(vec2(kit::W, 72.0), Sense::hover());
    caption(ui, row, tr("Alignment"));
    let grid = Rect::from_min_size(pos2(row.min.x + 88.0, row.min.y + 4.0), vec2(136.0, 65.0));
    ui.painter().rect_filled(grid, CornerRadius::same(5), t.field);
    let aligns = [(tr("Start"), PatternAlign::Start), (tr("Center"), PatternAlign::Center), (tr("End"), PatternAlign::End)];
    let on = aligns.iter().position(|(_, a)| *a == pt.align).unwrap_or(0);
    let cell = vec2((grid.width() - 2.0) / 3.0, 21.0);
    for r in 0..3usize {
        for c in 0..3usize {
            let rect = Rect::from_min_size(pos2(grid.min.x + 1.0 + cell.x * c as f32, grid.min.y + 1.0 + cell.y * r as f32), cell);
            let resp = ui.interact(rect, Id::unique(("cp_pattern_align", r, c)), Sense::click());
            if c == r && on == c {
                ui.painter().rect_filled(Rect::from_center_size(rect.center(), vec2(10.0, 10.0)), CornerRadius::same(2), crate::theme::ACCENT);
            } else {
                ui.painter().rect_filled(
                    Rect::from_center_size(rect.center(), vec2(2.0, 2.0)),
                    CornerRadius::same(1),
                    t.icon_secondary.gamma_multiply(0.6),
                );
            }
            if c == r {
                let label = aligns.get(c).map_or("", |(l, _)| *l);
                let resp = crate::a11y::Tip::tip(resp, label);
                if resp.clicked() && on != c {
                    pt.align = aligns.get(c).map_or(pt.align, |(_, a)| *a);
                    changed = Some(false);
                }
            } else if resp.clicked() {
                app.toast(tr("A pattern aligns at its start, center or end").to_string());
            }
        }
    }
    if let Some(live) = changed {
        paint.kind = PaintKind::Pattern(pt);
        write(app, target, paint, live);
        if !live {
            finish(app);
        }
    }
}

/// What the image preset row was asked to do.
enum PresetAction {
    Apply(String),
    Delete(String),
}

/// A saved preset in the presets menu: click applies it, the trailing button deletes it.
fn saved_preset_row(ui: &mut Ui, name: &str) -> Option<PresetAction> {
    let t = Tokens::get(ui.ctx());
    let w = ui.available_width().max(180.0);
    let (r, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let del = Rect::from_center_size(pos2(r.max.x - 14.0, r.center().y), vec2(20.0, 20.0));
    let del_resp = ui.interact(del, ui.scope_id().with(("cp_preset_del", name)), Sense::click()).tip(tr("Delete preset"));
    let hot = resp.hovered() || del_resp.hovered();
    if hot {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.menu_hover);
    }
    let col = if hot { Color32::WHITE } else { t.text };
    ui.painter().text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, name, regular(11.5), col);
    if hot {
        icons::paint(ui.painter(), del.shrink(4.0), Icon::Close, col);
    }
    if del_resp.clicked() {
        Some(PresetAction::Delete(name.to_string()))
    } else if resp.clicked() {
        Some(PresetAction::Apply(name.to_string()))
    } else {
        None
    }
}

/// The image paint's adjustment presets: a menu of the built-in presets and the file's saved ones
/// (named after the one in effect), "Save as preset…" and Reset.
fn image_presets(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, filters: ImageFilters) {
    let saved: Vec<(String, ImageFilters)> =
        app.session.doc().map(|d| d.image_presets.iter().map(|p| (p.name.clone(), p.filters)).collect()).unwrap_or_default();
    let built_in = ImageFilters::PRESETS.iter().filter(|(id, ..)| *id != "reset");
    let current = built_in
        .clone()
        .map(|(_, label, f)| (tr(label).to_string(), *f))
        .chain(saved.iter().cloned())
        .find(|(_, f)| *f == filters)
        .map(|(name, _)| name)
        .unwrap_or_else(|| if filters.is_identity() { tr("No preset").into() } else { tr("Custom").into() });
    let name_id = Id::unique("cp_preset_name");
    let mut naming: Option<String> = ui.data(|d| d.get_temp(name_id));
    let mut action = None;
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let resp = widgets::dropdown(ui, &current, 152.0, None).on_hover_text(tr("Adjustment presets"));
        crate::right::menu_popup(&resp).show(|ui| {
            for (id, label, _) in built_in {
                if crate::right::menu_item(ui, tr(label), "") {
                    action = Some(PresetAction::Apply(id.to_string()));
                    ui.close();
                }
            }
            ui.separator();
            for (name, _) in &saved {
                if let Some(a) = saved_preset_row(ui, name) {
                    action = Some(a);
                    ui.close();
                }
            }
            if !saved.is_empty() {
                ui.separator();
            }
            if crate::right::menu_item(ui, tr("Save as preset…"), "") {
                naming = Some(format!("Preset {}", saved.len() + 1));
                ui.close();
            }
        });
        if widgets::button(ui, tr("Reset"), false, !filters.is_identity()).on_hover_text(tr("Reset adjustments")).clicked() {
            action = Some(PresetAction::Apply("reset".into()));
        }
    });
    if let Some(name) = naming.as_mut() {
        let mut save = false;
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            let r = ui.add(egui::TextEdit::singleline(name).font(regular(11.0)).desired_width(136.0));
            crate::widgets::select_all_on_focus(ui, &r, name);
            if !r.has_focus() && !crate::a11y::field_focused(ui.ctx()) {
                r.request_focus();
            }
            save = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            save |= widgets::button(ui, tr("Save"), true, !name.trim().is_empty()).clicked();
        });
        if save && !name.trim().is_empty() {
            app.run("image.savePreset", json!({"name": name.trim(), "filters": filters}));
            naming = None;
        }
    }
    ui.data_mut(|d| match &naming {
        Some(n) => {
            d.insert_temp(name_id, n.clone());
        }
        None => d.remove::<String>(name_id),
    });
    match action {
        Some(PresetAction::Delete(name)) => {
            app.run("image.deletePreset", json!({"name": name}));
        }
        Some(PresetAction::Apply(name)) => match target {
            // Layer paints go through the command; other targets (a text range, a style) get the
            // preset's values written like any edit.
            PickerTarget::Paint { fills, index, ids } => {
                app.run("image.adjust", json!({"ids": ids, "target": if *fills { "fill" } else { "stroke" }, "index": index, "preset": name}));
            }
            _ => {
                let preset = app.session.doc().ok().and_then(|d| figcraft_engine::cmd::image::find_preset(d, &name));
                if let (Some(f), PaintKind::Image(img)) = (preset, &mut paint.kind) {
                    img.filters = f;
                    write(app, target, paint, false);
                }
            }
        },
        None => {}
    }
}

fn figcraft_format_add(doc: &mut figcraft_doc::Document, bytes: Vec<u8>) -> Option<String> {
    // Images are keyed by content hash; the engine's image.place uses the same scheme.
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut s = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut s);
        format!("img{:016x}{:08x}", s.finish(), bytes.len())
    };
    doc.images.insert(hash.clone(), figcraft_doc::ImageData { bytes: std::sync::Arc::new(bytes), width: w, height: h });
    Some(hash)
}

trait WithStops {
    fn with_stops(self, stops: Vec<ColorStop>) -> Self;
}

impl WithStops for Gradient {
    fn with_stops(mut self, stops: Vec<ColorStop>) -> Self {
        if stops.len() >= 2 {
            self.stops = stops;
        }
        self
    }
}

#[cfg(test)]
mod selection_color_tests {
    use super::*;
    use crate::NoServices;
    use figcraft_engine::{Session, selection_colors::collect};

    fn setup() -> (FigApp, NodeId, NodeId) {
        let mut s = Session::new();
        let a = NodeId(s.execute("shape.rectangle", &json!({"width":40,"height":40,"props":{"fill":"#ff0000"}})).unwrap()["id"].as_u64().unwrap());
        let b = NodeId(s.execute("shape.rectangle", &json!({"width":40,"height":40,"props":{"fill":"#0000ff"}})).unwrap()["id"].as_u64().unwrap());
        s.execute("select.set", &json!({"ids":[a,b]})).unwrap();
        let mut app = FigApp::new(s, Box::new(NoServices));
        let targets = collect(app.session.doc().unwrap(), &[a, b])[0].targets.clone();
        crate::selection_colors::toggle(&mut app, targets, None);
        (app, a, b)
    }

    #[test]
    fn picker_live_changes_keep_frozen_targets_and_commit_once() {
        let (mut app, a, b) = setup();
        let target = app.ui.picker.clone().unwrap();
        let before = app.session.undo_depth();
        for color in ["#0000ff", "#00ff00"] {
            write(&mut app, &target, &Paint::solid(Color::from_hex(color).unwrap()), true);
        }
        assert!(app.session.recent_colors.is_empty(), "previews must not be recorded");
        finish(&mut app);
        assert_eq!(app.session.recent_colors, vec![Color::from_hex("#00ff00").unwrap()]);
        assert_eq!(app.session.undo_depth().0, before.0 + 1);
        assert_eq!(current_paint(&app, &target).unwrap().solid_color().unwrap().to_hex(), "00FF00");
        assert_eq!(app.session.doc().unwrap().node(b).unwrap().fills[0].solid_color().unwrap().to_hex(), "0000FF");
        app.session.undo().unwrap();
        assert_eq!(app.session.doc().unwrap().node(a).unwrap().fills[0].solid_color().unwrap().to_hex(), "FF0000");
    }

    #[test]
    fn writing_a_variable_keeps_its_alpha() {
        // The picker holds a colour variable's alpha as the paint opacity; writing it back applies it once.
        let (mut app, _, _) = setup();
        app.session.execute("variable.createCollection", &json!({"name":"T"})).unwrap();
        let v = app.session.execute("variable.create", &json!({"collection":"T","name":"tint","type":"color","value":"#0D6EFD80"})).unwrap();
        let var = app.session.doc().unwrap().variables.get(v["id"].as_str().unwrap()).unwrap().clone();
        let target = PickerTarget::Variable { id: var.id.clone(), mode: var.values.keys().next().unwrap().clone() };
        for _ in 0..2 {
            let paint = current_paint(&app, &target).unwrap();
            write(&mut app, &target, &paint, false);
        }
        let c = current_paint(&app, &target).unwrap().solid_color().unwrap();
        assert_eq!(c.to_css_hex().to_uppercase(), "#0D6EFD80");
    }

    #[test]
    fn picker_does_not_follow_selection_or_document_switches() {
        let (mut app, _, _) = setup();
        let target = app.ui.picker.clone().unwrap();
        app.session.execute("select.none", &json!({})).unwrap();
        assert!(current_paint(&app, &target).is_none());
        let (mut app, _, _) = setup();
        let target = app.ui.picker.clone().unwrap();
        app.session.execute("file.new", &json!({})).unwrap();
        assert!(current_paint(&app, &target).is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layers_drag_tests::Ui;
    use figcraft_engine::Session;

    #[test]
    fn dragging_records_only_the_final_colour_in_recent_colors() {
        let mut u = Ui::new(Session::new());
        let target = PickerTarget::PageBackground;
        u.app.ui.picker = Some(target.clone());
        for hex in ["#110000", "#220000", "#330000"] {
            let c = Color::from_hex(hex).unwrap();
            write(&mut u.app, &target, &Paint::solid(c), true);
        }
        assert!(u.app.session.recent_colors.is_empty(), "previews must not be recorded: {:?}", u.app.session.recent_colors);
        finish(&mut u.app);
        let last = Color::from_hex("#330000").unwrap();
        assert_eq!(u.app.session.recent_colors, vec![last]);
    }

    #[test]
    fn a_drag_released_with_a_final_write_records_one_colour() {
        let mut u = Ui::new(Session::new());
        let target = PickerTarget::PageBackground;
        u.app.ui.picker = Some(target.clone());
        // Not a whole 8-bit value: the page stores it rounded, so recording both would add two near-identical swatches.
        let c = Color::new(0.3, 0.0, 0.0, 1.0);
        write(&mut u.app, &target, &Paint::solid(Color::new(0.1, 0.0, 0.0, 1.0)), true);
        write(&mut u.app, &target, &Paint::solid(c), false);
        finish(&mut u.app);
        assert_eq!(u.app.session.recent_colors.len(), 1, "{:?}", u.app.session.recent_colors);
    }

    #[test]
    fn a_click_outside_a_drag_still_records_the_colour() {
        let mut u = Ui::new(Session::new());
        let target = PickerTarget::PageBackground;
        u.app.ui.picker = Some(target.clone());
        let c = Color::from_hex("#445566").unwrap();
        write(&mut u.app, &target, &Paint::solid(c), false);
        assert_eq!(u.app.session.recent_colors, vec![c]);
    }
}

#[cfg(test)]
#[path = "colorpicker_image_tests.rs"]
mod image_tests;
