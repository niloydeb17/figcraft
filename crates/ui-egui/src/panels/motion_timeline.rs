//! The motion timeline under the canvas: play/pause, the playhead, the current frame's animated
//! layers, each with a bar over the span its animation covers and a row per animated field (an
//! outlined bar named for the field, with a tick at each keyframe), ready-made animations and the
//! selected keyframe's value and easing. Everything it changes goes through the `motion.*`
//! commands; the canvas shows the playhead's moment (`canvas::render::motion_doc`).

use egui::{Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use figcraft_doc::NodeId;
use figcraft_doc::motion::{MotionField, MotionValue};
use figcraft_doc::proto::Easing;
use figcraft_engine::cmd::motion::{current_frame, easing_name};
use serde_json::json;

use crate::FigApp;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::Tokens;

/// The panel's height (logical points).
pub const HEIGHT: f32 = 220.0;
/// The names column when the left sidebar is closed (it lines up with the sidebar otherwise).
pub const NAMES_W: f32 = 240.0;
// The incumbent's timeline measurements (its timeline theme and renderer; see MECHANICS.md).
/// The ruler: a marker strip with the tick dashes over a row of labels.
const RULER_H: f32 = 48.0;
const MARKER_H: f32 = 14.0;
const DASH_H: f32 = 7.0;
/// Space between the ruler and the first row.
const ROWS_GAP: f32 = 8.0;
/// A track row and a layer-name row.
const ROW_H: f32 = 24.0;
/// Zero seconds sits this far into the track area.
const ORIGIN: f32 = 18.0;
/// Room after the timeline's end, so its last label and keyframes aren't cut (and the close
/// button sits clear of them).
const END_PAD: f32 = 48.0;
/// The ruler aims for a label every this many pixels (rounded to 1, 2 or 5 × 10ⁿ seconds).
const LABEL_SPACING: f32 = 200.0;
/// A bar leaves this much of its row above and below it.
const BAR_INSET: f32 = 2.5;
/// A bar's corners.
const BAR_RADIUS: u8 = 5;
/// A keyframe tick: this wide and tall; the ticks at a bar's ends sit this far inside it.
const TICK_W: f32 = 1.5;
const TICK_H: f32 = 9.0;
const TICK_INSET: f32 = 7.0;
/// The narrowest a bar is drawn (a field with one keyframe), and the width a keyframe's tick
/// takes clicks over.
const BAR_MIN: f32 = 2.0 * TICK_INSET + 4.0;
const TICK_HIT: f32 = 14.0;

/// The timeline's colours: the incumbent's light theme, FigCraft's tokens in the dark one.
struct Look {
    bg: Color32,
    overlay: Color32,
    row_selected: Color32,
    row_hover: Color32,
    separator: Color32,
    ruler_text: Color32,
    ruler_stroke: Color32,
    /// A layer's bar: on a selected layer, and otherwise; its ticks.
    bar: Color32,
    bar_idle: Color32,
    bar_tick: Color32,
    /// A field's bar: its fill, outline and name (and ticks) on a selected layer, and otherwise.
    clip_fill: Color32,
    clip_fill_idle: Color32,
    clip_stroke: Color32,
    clip_stroke_idle: Color32,
    clip_text: Color32,
    clip_text_idle: Color32,
    /// A picked keyframe's tick, and one under the pointer.
    key_selected: Color32,
    key_hover: Color32,
    brand: Color32,
}

fn look(t: &Tokens) -> Look {
    let black = |a: f32| Color32::from_black_alpha((a * 255.0).round() as u8);
    let blue = |a: f32| Color32::from_rgba_unmultiplied(13, 153, 255, (a * 255.0).round() as u8);
    if t.dark {
        let white = |a: f32| Color32::from_white_alpha((a * 255.0).round() as u8);
        return Look {
            bg: t.panel,
            overlay: t.bg,
            row_selected: blue(0.24),
            row_hover: white(0.06),
            separator: white(0.1),
            ruler_text: white(0.5),
            ruler_stroke: white(0.1),
            bar: crate::theme::BRAND,
            bar_idle: white(0.28),
            bar_tick: Color32::WHITE,
            clip_fill: blue(0.14),
            clip_fill_idle: white(0.04),
            clip_stroke: crate::theme::BRAND,
            clip_stroke_idle: white(0.2),
            clip_text: Color32::from_rgb(0x7c, 0xc4, 0xff),
            clip_text_idle: white(0.55),
            key_selected: Color32::WHITE,
            key_hover: Color32::from_rgb(0x7c, 0xc4, 0xff),
            brand: crate::theme::BRAND,
        };
    }
    Look {
        bg: Color32::WHITE,
        overlay: Color32::from_rgb(0xf5, 0xf5, 0xf5),
        row_selected: blue(0.16),
        row_hover: black(0.04),
        separator: black(0.1),
        ruler_text: black(0.5),
        ruler_stroke: Color32::from_black_alpha(0x1a),
        bar: Color32::from_rgb(0x00, 0x7b, 0xe5),
        bar_idle: black(0.28),
        bar_tick: Color32::WHITE,
        clip_fill: Color32::from_rgb(0xf0, 0xf7, 0xfe),
        clip_fill_idle: Color32::from_rgb(0xf7, 0xf7, 0xf7),
        clip_stroke: Color32::from_rgb(0x0d, 0x99, 0xff),
        clip_stroke_idle: black(0.2),
        clip_text: Color32::from_rgb(0x00, 0x7b, 0xe5),
        clip_text_idle: black(0.5),
        key_selected: Color32::from_rgb(0x00, 0x4f, 0x99),
        key_hover: Color32::from_rgb(0x0d, 0x99, 0xff),
        brand: Color32::from_rgb(0x0d, 0x99, 0xff),
    }
}

/// A ruler step: `raw` seconds rounded down to 1, 2 or 5 × 10ⁿ.
fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(raw.log10().floor());
    let r = raw / p;
    if r >= 5.0 - 1e-9 {
        5.0 * p
    } else if r >= 2.0 - 1e-9 {
        2.0 * p
    } else {
        p
    }
}

/// The keyframe picked in the timeline: the layer, the field's key and its position.
#[derive(Clone, Debug, PartialEq)]
struct Picked {
    id: NodeId,
    field: String,
    time: f64,
}

/// A keyframe diamond being dragged along its row.
#[derive(Clone, Debug)]
struct Drag {
    picked: Picked,
    to: f64,
}

fn picked_id() -> Id {
    Id::unique("motion_timeline_picked")
}
fn drag_id() -> Id {
    Id::unique("motion_timeline_drag")
}
fn scroll_id() -> Id {
    Id::unique("motion_timeline_scroll")
}
fn clock_id() -> Id {
    Id::unique("motion_timeline_clock")
}

/// One row of the timeline: a layer's name, or one of its tracks.
enum Row {
    /// `span`: the first and last keyframe times over all its fields.
    Layer {
        id: NodeId,
        name: String,
        span: Option<(f64, f64)>,
    },
    Track {
        id: NodeId,
        field: MotionField,
        times: Vec<(f64, bool)>,
    },
}

/// Move the playhead on while playing (called every frame, whether or not the panel shows).
pub fn tick(app: &mut FigApp, ctx: &egui::Context) {
    let now = app.now_secs();
    let last = ctx.data(|d| d.get_temp::<f64>(clock_id())).unwrap_or(now);
    ctx.data_mut(|d| d.insert_temp(clock_id(), now));
    let playing = app.session.ds().is_ok_and(|ds| ds.motion.playing);
    if !playing {
        ctx.data_mut(|d| d.remove::<f64>(clock_id()));
        return;
    }
    let length = figcraft_engine::cmd::motion::current_length(&app.session);
    if let Ok(ds) = app.session.ds_mut() {
        ds.motion.advance(now - last, length);
    }
    ctx.request_repaint();
}

/// The timeline, its names column `names_w` wide (the left sidebar's width, so they line up).
pub fn show(app: &mut FigApp, ui: &mut Ui, names_w: f32) {
    let t = Tokens::get(ui.ctx());
    let Ok(ds) = app.session.ds() else { return };
    let player = ds.motion.clone();
    let frame = current_frame(&app.session);
    let doc = ds.doc.clone();
    let selection = ds.selection.clone();
    let length = frame.map_or(figcraft_doc::motion::DEFAULT_TIMELINE, |f| doc.timeline_duration(f));
    let full = ui.available_rect_before_wrap();
    ui.painter().rect_filled(full, CornerRadius::ZERO, t.panel);
    ui.painter().hline(full.x_range(), full.top() + 0.5, Stroke::new(1.0, t.border));

    let rows = rows_of(&doc, frame, &selection);

    let picked: Option<Picked> = ui.data(|d| d.get_temp(picked_id()));

    // The track area: names on the left, the ruler and rows on the right.
    let lk = look(&t);
    let body = full;
    ui.painter().rect_filled(body, CornerRadius::ZERO, lk.bg);
    ui.painter().hline(body.x_range(), body.top() + 0.5, Stroke::new(1.0, lk.separator));
    let names = Rect::from_min_max(body.min, pos2(body.left() + names_w.clamp(160.0, (body.width() - 200.0).max(160.0)), body.bottom()));
    let lane = Rect::from_min_max(pos2(names.right(), body.top()), body.max);
    ui.painter().vline(names.right() + 0.5, body.y_range(), Stroke::new(1.0, lk.separator));
    // The whole timeline fits the track area (the incumbent zooms to fit its length).
    let px_per_sec = ((lane.width() - ORIGIN - END_PAD).max(40.0) / length.max(0.01) as f32).max(1.0);
    let x_of = |s: f64| lane.left() + ORIGIN + s as f32 * px_per_sec;
    let t_of = |x: f32| (f64::from((x - lane.left() - ORIGIN) / px_per_sec)).clamp(0.0, length);
    let lane_painter = ui.painter().with_clip_rect(lane);

    let ruler_rect = Rect::from_min_size(lane.min, vec2(lane.width(), RULER_H));
    ruler(&lane_painter, &lk, ruler_rect, length, &x_of);
    scrub_ruler(app, ui, ruler_rect, &t_of);

    // The transport in the names column, level with the ruler; a line under both.
    ui.painter().hline(body.x_range(), (lane.top() + RULER_H).floor() + 0.5, Stroke::new(1.0, lk.separator));
    transport(app, ui, Rect::from_min_max(names.min, pos2(names.right(), names.top() + RULER_H)), &player, length, frame, &selection);
    close_button(app, ui, Rect::from_min_size(pos2(lane.right() - 30.0, lane.top() + MARKER_H + 3.0), vec2(26.0, 26.0)));
    let rows_top = lane.top() + RULER_H + ROWS_GAP;
    // Past the timeline's end the track area is greyed (the incumbent's working-area overlay).
    past_end(&lane_painter, &lk, lane, x_of(length));
    if frame.is_none() {
        hint(ui, &t, body, tr("Add a frame, then select layers inside it to animate them"));
    } else if rows.is_empty() {
        hint(
            ui,
            &t,
            Rect::from_min_max(pos2(body.left(), rows_top), body.max),
            tr("Select a layer inside the frame, then add a keyframe or an animation style"),
        );
    }
    let drag: Option<Drag> = ui.data(|d| d.get_temp(drag_id()));
    // Rows scroll under the ruler with the wheel; the selected layer's rows come into view when the
    // selection changes.
    let rows_rect = Rect::from_min_max(pos2(body.left(), rows_top), body.max);
    let scroll = rows_scroll(ui, &rows, rows_rect, &selection);
    let clip_painter = ui.painter().with_clip_rect(rows_rect);
    let key_painter = ui.painter().with_clip_rect(rows_rect.intersect(lane));
    let scroll = (scroll / ROW_H).round() * ROW_H;
    let mut y = rows_top - scroll;
    for (i, row) in rows.iter().enumerate() {
        // Only whole rows below the ruler (scrolling steps a row at a time).
        if y < rows_top - 0.5 {
            y += ROW_H;
            continue;
        }
        if y >= body.bottom() {
            break;
        }
        let r = Rect::from_min_size(pos2(body.left(), y), vec2(body.width(), ROW_H));
        let hovered = ui.rect_contains_pointer(r);
        match row {
            Row::Layer { id, name, span } => {
                let selected = selection.contains(id);
                let bg = if selected {
                    lk.row_selected
                } else if hovered {
                    lk.row_hover
                } else {
                    Color32::TRANSPARENT
                };
                clip_painter.rect_filled(r, CornerRadius::ZERO, bg);
                let resp =
                    ui.interact(Rect::from_min_max(r.min, pos2(names.right(), r.bottom())), Id::unique("motion_layer").with(i), Sense::click());
                if resp.clicked() {
                    app.run("select.set", json!({"ids": [id]}));
                }
                let (text, icon) = if selected { (t.text, t.icon) } else { (t.text, t.icon_secondary) };
                icons::paint(
                    &clip_painter,
                    Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), vec2(12.0, 12.0)),
                    crate::panels::layers::node_icon(&doc, *id),
                    icon,
                );
                clip_painter.text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, clip(name, 28), FontId::proportional(11.0), text);
                add_field_menu(app, ui, Rect::from_min_size(pos2(names.right() - 24.0, r.top() + 2.0), vec2(20.0, 20.0)), *id, i);
                if let Some((a, b)) = span {
                    layer_bar(&key_painter, &lk, bar_rect(x_of(*a), x_of(*b), r), selected);
                }
            }
            Row::Track { id, field, times } => {
                let row_selected = selection.contains(id);
                // Only the layer's own row is tinted when it is selected; its fields' bars say so.
                let bg = if hovered { lk.row_hover } else { Color32::TRANSPARENT };
                clip_painter.rect_filled(r, CornerRadius::ZERO, bg);
                clip_painter.text(
                    pos2(r.left() + 46.0, r.center().y),
                    Align2::LEFT_CENTER,
                    field.label(),
                    FontId::proportional(11.0),
                    if row_selected { t.text } else { t.text_secondary },
                );
                // The field's bar from its first keyframe to its last (as dragged), named for it.
                let shown: Vec<f64> = times
                    .iter()
                    .map(|(time, _)| {
                        let me = Picked { id: *id, field: field.key(), time: *time };
                        drag.as_ref().filter(|d| d.picked == me).map_or(*time, |d| d.to)
                    })
                    .collect();
                let (lo, hi) = shown.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), t| (a.min(*t), b.max(*t)));
                let bar = (lo <= hi).then(|| bar_rect(x_of(lo), x_of(hi), r));
                if let Some(bar) = bar {
                    field_bar(&key_painter, &lk, bar, &field.label(), row_selected);
                }
                for (k, (time, hold)) in times.iter().enumerate() {
                    let me = Picked { id: *id, field: field.key(), time: *time };
                    let shown_t = shown.get(k).copied().unwrap_or(*time);
                    // The first and last keyframes' ticks sit inside the bar's ends.
                    let x = match bar {
                        Some(bar) if shown_t <= lo => bar.left() + TICK_INSET,
                        Some(bar) if shown_t >= hi => bar.right() - TICK_INSET,
                        _ => x_of(shown_t),
                    };
                    let c = pos2(x.round(), r.center().y.round());
                    if c.x < lane.left() - TICK_HIT || c.x > lane.right() + TICK_HIT {
                        continue;
                    }
                    let hit = Rect::from_center_size(c, vec2(TICK_HIT, ROW_H)).intersect(lane);
                    let resp = ui.interact(hit, Id::unique("motion_key").with((i, k)), Sense::click_and_drag()).on_hover_text(format!(
                        "{} · {:.2}s",
                        field.label(),
                        time
                    ));
                    let is_picked = picked.as_ref() == Some(&me);
                    if resp.clicked() || resp.drag_started() {
                        ui.data_mut(|d| d.insert_temp(picked_id(), me.clone()));
                        app.run("select.set", json!({"ids": [id]}));
                        if resp.clicked() {
                            app.run("motion.seek", json!({"time": time}));
                        }
                    }
                    if resp.dragged()
                        && let Some(p) = resp.interact_pointer_pos()
                    {
                        let to = figcraft_doc::motion::snap_time(t_of(p.x));
                        ui.data_mut(|d| d.insert_temp(drag_id(), Drag { picked: me.clone(), to }));
                    }
                    if resp.drag_stopped() {
                        if let Some(d) = drag.as_ref().filter(|d| d.picked == me && (d.to - me.time).abs() > 1e-9) {
                            app.run("motion.keyframe.set", json!({"ids": [id], "field": me.field, "time": me.time, "newTime": d.to}));
                            ui.data_mut(|m| m.insert_temp(picked_id(), Picked { time: d.to, ..me.clone() }));
                        }
                        ui.data_mut(|m| m.remove::<Drag>(drag_id()));
                    }
                    resp.context_menu(|ui| keyframe_menu(app, ui, &me));
                    let color = if is_picked {
                        lk.key_selected
                    } else if resp.hovered() {
                        lk.key_hover
                    } else if row_selected {
                        lk.clip_text
                    } else {
                        lk.clip_text_idle
                    };
                    key_tick(&key_painter, c, color, *hold);
                    if is_picked {
                        // A picked keyframe stands out: a wider tick.
                        key_tick(&key_painter, c + vec2(-TICK_W, 0.0), color, *hold);
                        key_tick(&key_painter, c + vec2(TICK_W, 0.0), color, *hold);
                    }
                }
            }
        }
        y += ROW_H;
    }

    playhead(ui, &lk, lane, body.bottom(), x_of(player.playhead.min(length)).round() + 0.5);
}

/// The timeline's rows: the frame's animated layers, then selected layers in it that aren't
/// animated yet, each followed by a row per animated field.
fn rows_of(doc: &figcraft_doc::Document, frame: Option<NodeId>, selection: &[NodeId]) -> Vec<Row> {
    let mut layers: Vec<NodeId> = frame.map(|f| doc.animated_in(f)).unwrap_or_default();
    for s in selection {
        if frame.is_some_and(|f| *s != f && doc.top_level_frame(*s) == Some(f)) && !layers.contains(s) {
            layers.push(*s);
        }
    }
    let mut rows = Vec::new();
    for id in &layers {
        let Some(n) = doc.node(*id) else { continue };
        let span = n.motion.as_ref().and_then(|m| {
            let times = m.fields().into_iter().flat_map(|(_, track)| track.keyframes.iter().map(|k| k.time)).filter(|t| t.is_finite());
            times.fold(None, |acc: Option<(f64, f64)>, t| Some(acc.map_or((t, t), |(a, b)| (a.min(t), b.max(t)))))
        });
        rows.push(Row::Layer { id: *id, name: n.name.clone(), span });
        if let Some(m) = n.motion.as_ref() {
            for (field, track) in m.fields() {
                rows.push(Row::Track { id: *id, field, times: track.keyframes.iter().map(|k| (k.time, k.hold)).collect() });
            }
        }
    }
    rows
}

/// The playhead at `x`: a pin in the marker strip (rounded top, pointed foot) and a line down the
/// rows to `bottom`, in the brand colour.
fn playhead(ui: &Ui, lk: &Look, lane: Rect, bottom: f32, x: f32) {
    if x < lane.left() || x > lane.right() {
        return;
    }
    let ph = ui.painter().with_clip_rect(lane);
    ph.vline(x, (lane.top() + MARKER_H - 2.0)..=bottom, Stroke::new(1.0, lk.brand));
    let top = lane.top();
    let head = Rect::from_min_max(pos2(x - 8.0, top), pos2(x + 8.0, top + MARKER_H - 5.0));
    ph.rect_filled(head, CornerRadius { nw: 0, ne: 0, sw: 3, se: 3 }, lk.brand);
    ph.add(egui::Shape::convex_polygon(
        vec![pos2(head.left(), head.bottom() - 3.0), pos2(head.right(), head.bottom() - 3.0), pos2(x, head.bottom() + 5.0)],
        lk.brand,
        Stroke::NONE,
    ));
}

/// The button at the ruler's right end that closes the timeline.
fn close_button(app: &mut FigApp, ui: &mut Ui, r: Rect) {
    if icons::button(&mut ui.new_child(egui::UiBuilder::new().max_rect(r)), Icon::Close, 24.0, false, tr("Close timeline")).clicked() {
        app.run("view.motionTimeline", json!({"show": false}));
    }
}

/// A press or drag on the ruler moves the playhead there (`t_of` turns x into seconds).
fn scrub_ruler(app: &mut FigApp, ui: &Ui, ruler_rect: Rect, t_of: &dyn Fn(f32) -> f64) {
    let scrub = ui.interact(ruler_rect, Id::unique("motion_ruler"), Sense::click_and_drag());
    if (scrub.clicked() || scrub.dragged())
        && let Some(p) = scrub.interact_pointer_pos()
    {
        app.run("motion.seek", json!({"time": t_of(p.x)}));
    }
}

/// The track area after the timeline's end (`end_x`), greyed below the ruler.
fn past_end(p: &egui::Painter, lk: &Look, lane: Rect, end_x: f32) {
    if end_x >= lane.right() {
        return;
    }
    let past = Rect::from_min_max(pos2(end_x.max(lane.left()), lane.top() + RULER_H), lane.max);
    p.rect_filled(past, CornerRadius::ZERO, lk.overlay);
    p.vline(end_x + 0.5, past.y_range(), Stroke::new(1.0, lk.separator));
}

/// How far the rows are scrolled (whole rows): the wheel over them scrolls, and the selected
/// layer's rows come into view when the selection changes.
fn rows_scroll(ui: &Ui, rows: &[Row], rows_rect: Rect, selection: &[NodeId]) -> f32 {
    let room = rows_rect.height().max(0.0);
    let max_scroll = (rows.len() as f32 * ROW_H - room).max(0.0);
    let mut scroll: f32 = ui.data(|d| d.get_temp(scroll_id())).unwrap_or(0.0);
    if ui.rect_contains_pointer(rows_rect) && !ui.input(|i| i.modifiers.shift) {
        scroll -= ui.input(|i| i.smooth_scroll_delta.y);
    }
    let follow_id = scroll_id().with("selection");
    if ui.data(|d| d.get_temp::<Vec<NodeId>>(follow_id)).as_deref() != Some(selection) {
        ui.data_mut(|d| d.insert_temp(follow_id, selection.to_vec()));
        if let Some(at) = rows.iter().position(|r| matches!(r, Row::Layer { id, .. } if selection.contains(id))) {
            let top = at as f32 * ROW_H;
            if top < scroll || top + ROW_H * 2.0 > scroll + room {
                scroll = top;
            }
        }
    }
    let scroll = scroll.clamp(0.0, max_scroll);
    ui.data_mut(|d| d.insert_temp(scroll_id(), scroll));
    scroll
}

fn hint(ui: &Ui, t: &Tokens, r: Rect, text: &str) {
    ui.painter().text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), t.text_tertiary);
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() > n { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) } else { s.to_string() }
}

/// A layer's bar: the span its animation covers, solid, with a tick inside each end.
fn layer_bar(p: &egui::Painter, lk: &Look, bar: Rect, selected: bool) {
    p.rect_filled(bar, CornerRadius::same(BAR_RADIUS), if selected { lk.bar } else { lk.bar_idle });
    for x in [bar.left() + TICK_INSET, bar.right() - TICK_INSET] {
        key_tick(p, pos2(x, bar.center().y), lk.bar_tick, false);
    }
}

/// A field's bar: outlined, its name in the middle (when there is room beside the end ticks).
fn field_bar(p: &egui::Painter, lk: &Look, bar: Rect, label: &str, selected: bool) {
    let (fill, edge, text) =
        if selected { (lk.clip_fill, lk.clip_stroke, lk.clip_text) } else { (lk.clip_fill_idle, lk.clip_stroke_idle, lk.clip_text_idle) };
    p.rect_filled(bar, CornerRadius::same(BAR_RADIUS), fill);
    p.rect_stroke(bar, CornerRadius::same(BAR_RADIUS), Stroke::new(1.0, edge), egui::StrokeKind::Inside);
    let room = bar.shrink2(vec2(TICK_INSET + 6.0, 0.0));
    if room.width() > 16.0 {
        p.with_clip_rect(room.intersect(p.clip_rect())).text(bar.center(), Align2::CENTER_CENTER, label, FontId::proportional(11.0), text);
    }
}

/// A bar in row `r` from `x0` to `x1` (at least [`BAR_MIN`] wide, about its middle).
fn bar_rect(x0: f32, x1: f32, r: Rect) -> Rect {
    let (mut a, mut b) = (x0.round(), x1.round());
    if b - a < BAR_MIN {
        let mid = (a + b) / 2.0;
        (a, b) = ((mid - BAR_MIN / 2.0).round(), (mid + BAR_MIN / 2.0).round());
    }
    Rect::from_min_max(pos2(a, r.top() + BAR_INSET), pos2(b, r.bottom() - BAR_INSET))
}

/// A keyframe's tick at `c`: a short upright line (a small square for a hold keyframe).
fn key_tick(p: &egui::Painter, c: Pos2, color: Color32, hold: bool) {
    let size = if hold { vec2(4.0, 4.0) } else { vec2(TICK_W, TICK_H) };
    p.rect_filled(Rect::from_center_size(c, size), CornerRadius::same(1), color);
}

/// The ruler: the marker strip (white over the timeline, grey past it) with a dash at each label,
/// and the labels centred in the row below, every 1, 2 or 5 × 10ⁿ seconds about 200 px apart
/// (whole seconds as "2s", in between as "500ms").
fn ruler(p: &egui::Painter, lk: &Look, r: Rect, length: f64, x_of: &dyn Fn(f64) -> f32) {
    let strip = Rect::from_min_size(r.min, vec2(r.width(), MARKER_H));
    p.rect_filled(strip, CornerRadius::ZERO, lk.overlay);
    let (x0, x1) = (x_of(0.0).max(r.left()), x_of(length).min(r.right()));
    if x1 > x0 {
        p.rect_filled(Rect::from_min_max(pos2(x0, strip.top()), pos2(x1, strip.bottom())), CornerRadius::ZERO, lk.bg);
    }
    p.hline(r.x_range(), strip.bottom() - 0.5, Stroke::new(1.0, lk.ruler_stroke));
    let labels = Rect::from_min_max(pos2(r.left(), strip.bottom()), pos2(r.right(), r.bottom() - 1.0));
    p.rect_filled(labels, CornerRadius::ZERO, lk.bg);
    let per_sec = f64::from(x_of(1.0) - x_of(0.0)).max(1e-6);
    let step = nice_step(f64::from(LABEL_SPACING) / per_sec);
    let (first, last) = (f64::from(r.left() - x_of(0.0)) / per_sec, f64::from(r.right() - x_of(0.0)) / per_sec);
    let (lo, hi) = ((first / step).floor() as i64 - 1, (last / step).ceil() as i64 + 1);
    for i in lo.max(-1000)..=hi.min(lo.max(-1000) + 4000) {
        let s = i as f64 * step;
        let x = x_of(s).round();
        p.rect_filled(
            Rect::from_min_size(pos2(x - 0.5, strip.top() + ((MARKER_H - DASH_H) / 2.0).floor()), vec2(1.0, DASH_H)),
            CornerRadius::ZERO,
            lk.ruler_stroke,
        );
        let whole = (s - s.round()).abs() < 1e-9;
        // Under a second in milliseconds ("200ms"); from a second on in seconds ("1.2s").
        let text = if whole || step >= 1.0 || s >= 1.0 { format!("{}s", trim(s)) } else { format!("{}ms", (s * 1000.0).round()) };
        p.text(pos2(x, labels.center().y), Align2::CENTER_CENTER, text, FontId::proportional(11.0), lk.ruler_text);
    }
}

fn trim(s: f64) -> String {
    let v = format!("{s:.2}");
    v.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The transport, as the incumbent lays it out at the left of its timeline: play/pause, add a
/// keyframe, the playhead's time and the timeline's length in milliseconds, loop.
fn transport(
    app: &mut FigApp,
    ui: &mut Ui,
    r: Rect,
    player: &figcraft_engine::cmd::motion::MotionPlayer,
    length: f64,
    frame: Option<NodeId>,
    selection: &[NodeId],
) {
    let t = Tokens::get(ui.ctx());
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(r.shrink2(vec2(10.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = 4.0;
    let play_icon = if player.playing { Icon::Pause } else { Icon::Play };
    if icons::button(ui, play_icon, 24.0, false, if player.playing { tr("Pause") } else { tr("Play") }).clicked() {
        app.run("motion.play", json!({}));
    }
    // Add a keyframe at the playhead to every animated field of the selected layers (their
    // position when nothing is animated yet).
    let (kr, kresp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let kresp = kresp.on_hover_text(tr("Add keyframe"));
    if kresp.hovered() {
        ui.painter().rect_filled(kr, CornerRadius::same(5), t.hover);
    }
    keyframe_glyph(ui.painter(), kr.center(), t.icon, true);
    let targets: Vec<NodeId> = selection.iter().copied().filter(|s| Some(*s) != frame).collect();
    if kresp.clicked() && !targets.is_empty() {
        let fields: Vec<String> = app
            .session
            .doc()
            .ok()
            .map(|doc| {
                let mut f: Vec<String> = targets
                    .iter()
                    .filter_map(|i| doc.node(*i)?.motion.as_ref().map(|m| m.fields().into_iter().map(|(f, _)| f.key()).collect::<Vec<_>>()))
                    .flatten()
                    .collect();
                f.sort();
                f.dedup();
                f
            })
            .unwrap_or_default();
        let fields = if fields.is_empty() { vec!["TRANSLATION_X".to_string(), "TRANSLATION_Y".to_string()] } else { fields };
        for f in fields {
            app.run("motion.keyframe.add", json!({"ids": targets, "field": f}));
        }
    }
    ui.add_space(4.0);
    // The time fields: one grey box with the playhead's milliseconds (leading zeros dim), the
    // length's, the unit and the loop toggle.
    let mono = FontId::monospace(11.5);
    let now_ms = (player.playhead * 1000.0).round().clamp(0.0, 9_999_999.0) as u64;
    let digits = format!("{now_ms:04}");
    let lead = digits.len() - digits.trim_start_matches('0').len().max(1);
    let cell = |ui: &mut Ui, w: f32| ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
    let start_x = ui.cursor().min.x;
    let (c1, _) = cell(ui, 40.0);
    let mut job = egui::text::LayoutJob::default();
    job.append(&digits[..lead], 0.0, egui::TextFormat { font_id: mono.clone(), color: t.text_tertiary, ..Default::default() });
    job.append(&digits[lead..], 0.0, egui::TextFormat { font_id: mono.clone(), color: t.text, ..Default::default() });
    let g1 = ui.painter().layout_job(job);
    let mut len_ms = (length * 1000.0).round();
    let (c2, _) = cell(ui, 44.0);
    let (c3, _) = cell(ui, 24.0);
    let (c4, loop_resp) = cell(ui, 26.0);
    let boxr = Rect::from_min_max(pos2(start_x, c1.top()), c4.max);
    ui.painter().rect_filled(boxr, CornerRadius::same(5), t.field);
    for c in [c1, c2, c3] {
        ui.painter().vline(c.right() + 1.0, c.y_range(), Stroke::new(1.0, t.panel));
    }
    ui.painter().galley(pos2(c1.center().x - g1.size().x / 2.0, c1.center().y - g1.size().y / 2.0), g1, t.text);
    // The length field fills its cell, its digits centred in the playhead time's font and colour
    // so the two read as one row.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(c2).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
    {
        let style = child.style_mut();
        style.override_font_id = Some(mono.clone());
        style.visuals.override_text_color = Some(t.text);
        for w in [&mut style.visuals.widgets.inactive, &mut style.visuals.widgets.hovered, &mut style.visuals.widgets.active] {
            w.weak_bg_fill = Color32::TRANSPARENT;
            w.bg_fill = Color32::TRANSPARENT;
            w.bg_stroke = Stroke::NONE;
            w.expansion = 0.0;
        }
        style.spacing.button_padding = vec2(0.0, 0.0);
    }
    let resp = child.add_enabled(
        frame.is_some(),
        egui::DragValue::new(&mut len_ms).range(100.0..=600_000.0).speed(10.0).max_decimals(0).custom_formatter(|v, _| format!("{v:.0}")),
    );
    if resp.changed()
        && let Some(f) = frame
    {
        app.run("motion.timeline.duration", json!({"id": f, "seconds": len_ms / 1000.0}));
    }
    ui.painter().text(c3.center(), Align2::CENTER_CENTER, "ms", FontId::proportional(11.0), t.text_secondary);
    let loop_resp = loop_resp.on_hover_text(tr("Loop"));
    if loop_resp.hovered() {
        ui.painter().rect_filled(c4, CornerRadius::same(5), t.hover);
    }
    icons::paint(
        ui.painter(),
        Rect::from_center_size(c4.center(), vec2(14.0, 14.0)),
        Icon::Rotate,
        if player.looping { t.icon } else { t.icon_secondary },
    );
    if loop_resp.clicked() {
        app.run("motion.play", json!({"playing": player.playing, "loop": !player.looping}));
    }
}

/// The "add keyframe" glyph: a diamond outline with a dot in it.
pub(crate) fn keyframe_glyph(p: &egui::Painter, c: Pos2, color: Color32, dot: bool) {
    let r = 6.0;
    let pts = vec![pos2(c.x, c.y - r), pos2(c.x + r, c.y), pos2(c.x, c.y + r), pos2(c.x - r, c.y)];
    p.add(egui::Shape::closed_line(pts, Stroke::new(1.2, color)));
    if dot {
        p.circle_filled(c, 1.6, color);
    }
}

/// The keyframe picked in the timeline, as a section of the Motion panel: its value, easing and
/// Delete (nothing when none is picked).
pub(crate) fn picked_section(app: &mut FigApp, ui: &mut Ui) {
    let Some(pk) = ui.data(|d| d.get_temp::<Picked>(picked_id())) else { return };
    picked_editor(app, ui, &pk);
}

fn picked_editor(app: &mut FigApp, ui: &mut Ui, pk: &Picked) {
    let t = Tokens::get(ui.ctx());
    let Ok(doc) = app.session.doc() else { return };
    let Some(field) = MotionField::parse(&pk.field) else { return };
    let Some(k) = doc
        .node(pk.id)
        .and_then(|n| n.motion.as_ref())
        .and_then(|m| m.track(field))
        .and_then(|tr| tr.keyframes.iter().find(|k| (k.time - pk.time).abs() < 1e-6))
        .copied()
    else {
        return;
    };
    crate::widgets::separator(ui);
    crate::widgets::section_header(ui, tr("Keyframe"), &[], false);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(format!("{} · {:.0}ms", field.label(), k.time * 1000.0)).size(11.0).color(t.text_secondary));
        if let MotionValue::Float(mut v) = k.value {
            let speed = if field == MotionField::Opacity { 0.01 } else { 1.0 };
            if ui.add(egui::DragValue::new(&mut v).speed(speed).max_decimals(2)).changed() {
                app.run("motion.keyframe.set", json!({"ids": [pk.id], "field": pk.field, "time": pk.time, "value": v}));
            }
        }
        let current = easing_name(k.easing, k.hold);
        craft_ui::ComboBox::from_id_salt("motion_easing").selected_text(easing_label(k.easing, k.hold)).width(130.0).show_ui(ui, |ui| {
            for (name, label) in EASINGS {
                if ui.selectable_label(current == name, tr(label)).clicked() {
                    app.run("motion.keyframe.set", json!({"ids": [pk.id], "field": pk.field, "time": pk.time, "easing": name}));
                }
            }
        });
        if ui.button(tr("Delete")).clicked() {
            app.run("motion.keyframe.remove", json!({"ids": [pk.id], "field": pk.field, "time": pk.time}));
            ui.data_mut(|d| d.remove::<Picked>(picked_id()));
        }
    });
}

/// The easings offered, by public name.
const EASINGS: [(&str, &str); 12] = [
    ("LINEAR", crate::i18n::tr_noop("Linear")),
    ("EASE_IN", crate::i18n::tr_noop("Ease in")),
    ("EASE_OUT", crate::i18n::tr_noop("Ease out")),
    ("EASE_IN_AND_OUT", crate::i18n::tr_noop("Ease in and out")),
    ("EASE_IN_BACK", crate::i18n::tr_noop("Ease in back")),
    ("EASE_OUT_BACK", crate::i18n::tr_noop("Ease out back")),
    ("EASE_IN_AND_OUT_BACK", crate::i18n::tr_noop("Ease in and out back")),
    ("GENTLE", crate::i18n::tr_noop("Gentle")),
    ("QUICK", crate::i18n::tr_noop("Quick")),
    ("BOUNCY", crate::i18n::tr_noop("Bouncy")),
    ("SLOW", crate::i18n::tr_noop("Slow")),
    ("HOLD", crate::i18n::tr_noop("Hold")),
];

fn easing_label(e: Easing, hold: bool) -> String {
    if hold { tr("Hold").to_string() } else { tr(e.label()).to_string() }
}

/// The right-click menu of a keyframe: its easing, Delete.
fn keyframe_menu(app: &mut FigApp, ui: &mut Ui, pk: &Picked) {
    ui.menu_button(tr("Easing"), |ui| {
        for (name, label) in EASINGS {
            if ui.button(tr(label)).clicked() {
                app.run("motion.keyframe.set", json!({"ids": [pk.id], "field": pk.field, "time": pk.time, "easing": name}));
                ui.close();
            }
        }
    });
    if ui.button(tr("Delete keyframe")).clicked() {
        app.run("motion.keyframe.remove", json!({"ids": [pk.id], "field": pk.field, "time": pk.time}));
        ui.close();
    }
}

/// "+" on a layer's row: add a keyframe for a property at the playhead.
fn add_field_menu(app: &mut FigApp, ui: &mut Ui, r: Rect, id: NodeId, row: usize) {
    let mut fields: Vec<MotionField> = MotionField::BASIC.to_vec();
    if let Some(n) = app.session.doc().ok().and_then(|d| d.node(id)) {
        fields.extend((0..n.fills.len().min(4)).map(MotionField::Fill).filter(|f| f.rest_value(n).is_some()));
        fields.extend((0..n.strokes.len().min(4)).map(MotionField::Stroke).filter(|f| f.rest_value(n).is_some()));
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt(("motion_add", row)));
    egui::containers::menu::MenuButton::new(egui::RichText::new("+").size(14.0)).ui(&mut child, |ui| {
        ui.label(egui::RichText::new(tr("Add keyframe at the playhead")).size(11.0).weak());
        for f in fields {
            if ui.button(f.label()).clicked() {
                app.run("motion.keyframe.add", json!({"ids": [id], "field": f.key()}));
                ui.close();
            }
        }
    });
}
