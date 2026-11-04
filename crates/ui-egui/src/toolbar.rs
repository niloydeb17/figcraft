//! The floating bottom toolbar (UI3): tool groups with flyouts, comments, actions.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_engine::ToolId;
use figcraft_engine::vectoredit::VTool;
use serde_json::json;

use crate::FigApp;
use crate::a11y::{self, Nav, Region, Tip};
use crate::i18n::{tr, tr_noop, trf};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular};

/// Every mark in the toolbar row is painted in a box this size, so they share one optical size
/// and one stroke width (#737, #738).
const GLYPH: f32 = 16.0;
/// The flyout carets: smaller than the marks, but on a box whose stroke rounds to the same width
/// as theirs at 1×, 1.5× and 2× (a 10-point box drew them one physical pixel thinner).
const CARET: f32 = 16.0;
/// A flyout caret's button: wide enough to hit, a little clear of its tool's (#988).
const FLYOUT_W: f32 = 16.0;
const FLYOUT_GAP: f32 = 1.0;

// The pill's grid: every state of it (the tools, the vector and shape builder ones) is laid out on
// one 32 pt cell, PAD in from its ends and GAP between its top-level items, the divider included.
/// The UI3 toolbar's colours: the chosen tool's fill (the kit's blue; its hand-off green in Dev
/// Mode), white on it, and the chosen mode's mark.
pub(crate) const DESIGN_BLUE: Color32 = Color32::from_rgb(0x0D, 0x99, 0xFF);
const HANDOFF_GREEN: Color32 = Color32::from_rgb(0x14, 0xAE, 0x5C);
const MODE_BLUE: Color32 = Color32::from_rgb(0x00, 0x7B, 0xE5);
/// The chosen mode's mark on the dark theme.
const MODE_BLUE_DARK: Color32 = Color32::from_rgb(0x00, 0x9C, 0xFE);

/// The fill of the toolbar's chosen tool and of the Share button for the panel's `tab`.
pub(crate) fn mode_fill(tab: crate::RightTab) -> Color32 {
    if tab == crate::RightTab::Inspect { HANDOFF_GREEN } else { DESIGN_BLUE }
}

fn active_id() -> egui::Id {
    egui::Id::unique("toolbar_active_fill")
}

/// The fill of the chosen tool's button: set by [`show`] for the file's mode.
fn active_fill(ui: &Ui) -> Color32 {
    ui.data(|d| d.get_temp::<Color32>(active_id())).unwrap_or(DESIGN_BLUE)
}

/// The pill's height and corner radius.
const PILL_H: f32 = 48.0;
const PILL_RADIUS: u8 = 13;
/// A toolbar button's size: the cell everything in the pill is placed on.
const TOOL_CELL: f32 = 32.0;
/// The pill's left and right padding.
const PAD: f32 = 8.0;
/// Between the pill's top-level items — the divider included.
const GAP: f32 = 8.0;
/// The divider's width.
const DIVIDER: f32 = 1.0;
/// The mode switcher's marks: smaller than the tools' (13–14 pt).
const MODE_GLYPH: f32 = 16.0;
/// The mode switcher's segments: a little smaller than the tool cells, `MODE_INSET` in from the
/// field's edge and `MODE_INSET` apart.
const MODE_CELL: f32 = 28.0;
const MODE_INSET: f32 = 2.0;
/// The mode switcher's width: four segments, three gaps and an inset at each end.
const MODE_W: f32 = 4.0 * MODE_CELL + 3.0 * MODE_INSET + 2.0 * MODE_INSET;

/// Draw mode's illustrated tools: each stands in a cell this wide, its top this far below the
/// pill's (the rest hidden below the pill), and rises this far on hover and further when chosen
/// (UI3's Draw toolbar).
const DRAW_CELL: f32 = 48.0;
const DRAW_REST: f32 = 8.0;
const DRAW_HOVER_RISE: f32 = 8.0;
const DRAW_CHOSEN_RISE: f32 = 14.0;

/// `GLYPH`'s box centred in `r`.
fn glyph_rect(r: Rect) -> Rect {
    Rect::from_center_size(r.center(), vec2(GLYPH, GLYPH))
}

struct Group {
    key: &'static str,
    /// The flyout button's name for screen readers.
    name: &'static str,
    tools: &'static [(ToolId, Icon)],
    /// What a one-tool group's flyout offers instead of tools (boards: colours, shapes, stamps).
    options: Options,
    /// Rows after the tools that run a command instead of choosing a tool.
    extras: &'static [Extra],
}

/// A flyout row that runs a command: the shape tools' Image/video…
struct Extra {
    label: &'static str,
    icon: Icon,
    cmd: &'static str,
}

/// The choices a board tool's flyout shows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Options {
    None,
    Colors,
    Shapes,
    Stamps,
}

impl Group {
    const fn tools(key: &'static str, name: &'static str, tools: &'static [(ToolId, Icon)]) -> Group {
        Group { key, name, tools, options: Options::None, extras: &[] }
    }
    const fn with(self, extras: &'static [Extra]) -> Group {
        Group { extras, ..self }
    }
    fn has_flyout(&self) -> bool {
        self.tools.len() > 1 || self.options != Options::None
    }
}

const GROUPS: &[Group] = &[
    Group::tools("move", tr_noop("Move tools"), &[(ToolId::Move, Icon::Move), (ToolId::Hand, Icon::Hand), (ToolId::Scale, Icon::Scale)]),
    Group::tools("frame", tr_noop("Frame tools"), &[(ToolId::Frame, Icon::Frame), (ToolId::Section, Icon::Section), (ToolId::Slice, Icon::Slice)]),
    Group::tools(
        "shape",
        tr_noop("Shape tools"),
        &[
            (ToolId::Rectangle, Icon::Rectangle),
            (ToolId::Line, Icon::Line),
            (ToolId::Arrow, Icon::Arrow),
            (ToolId::Ellipse, Icon::Ellipse),
            (ToolId::Polygon, Icon::Polygon),
            (ToolId::Star, Icon::Star),
        ],
    )
    .with(&[Extra { label: tr_noop("Image/video…"), icon: Icon::Image, cmd: "file.placeImage" }]),
    Group::tools(
        "pen",
        tr_noop("Drawing tools"),
        &[(ToolId::Pen, Icon::Pen), (ToolId::Pencil, Icon::Pencil), (ToolId::ShapeBuilder, Icon::ShapeBuilder)],
    ),
    Group::tools("text", tr_noop("Text"), &[(ToolId::Text, Icon::Text), (ToolId::TextOnPath, Icon::TextOnPath)]),
    Group::tools("comment", tr_noop("Comment"), &[(ToolId::Comment, Icon::Comment)]),
];

/// Draw mode's tool groups around its illustrated tools: Move, then Frame, the shapes (the
/// ellipse first), Text and Comment.
const DRAW_GROUPS: &[Group] = &[
    Group::tools("move", tr_noop("Move tools"), &[(ToolId::Move, Icon::Move), (ToolId::Hand, Icon::Hand), (ToolId::Scale, Icon::Scale)]),
    Group::tools("frame", tr_noop("Frame tools"), &[(ToolId::Frame, Icon::Frame), (ToolId::Section, Icon::Section), (ToolId::Slice, Icon::Slice)]),
    Group::tools(
        "dshape",
        tr_noop("Shape tools"),
        &[
            (ToolId::Ellipse, Icon::Ellipse),
            (ToolId::Rectangle, Icon::Rectangle),
            (ToolId::Line, Icon::Line),
            (ToolId::Arrow, Icon::Arrow),
            (ToolId::Polygon, Icon::Polygon),
            (ToolId::Star, Icon::Star),
        ],
    )
    .with(&[Extra { label: tr_noop("Image/video…"), icon: Icon::Image, cmd: "file.placeImage" }]),
    Group::tools("text", tr_noop("Text"), &[(ToolId::Text, Icon::Text), (ToolId::TextOnPath, Icon::TextOnPath)]),
    Group::tools("comment", tr_noop("Comment"), &[(ToolId::Comment, Icon::Comment)]),
];

/// Draw mode's illustrated tools.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DrawTool {
    Pen,
    /// The Pencil with a bristle brush.
    Brush,
    /// The Pencil with a plain stroke.
    Pencil,
}

/// Each illustrated tool: what it is, its name, its picture (the UI3 kit's, 4× its size, private
/// use) and its size in points.
const DRAW_TOOLS: [(DrawTool, &str, &[u8], [f32; 2]); 3] = [
    (DrawTool::Pen, tr_noop("Pen"), include_bytes!("../../../assets/figma-ui3/draw-pen.png"), [34.0, 68.0]),
    (DrawTool::Brush, tr_noop("Brush"), include_bytes!("../../../assets/figma-ui3/draw-brush.png"), [36.0, 67.0]),
    (DrawTool::Pencil, tr_noop("Pencil"), include_bytes!("../../../assets/figma-ui3/draw-pencil.png"), [34.0, 67.0]),
];

/// The illustrated tool chosen now, if one is.
fn drawn(app: &FigApp) -> Option<DrawTool> {
    match app.session.tool.tool {
        ToolId::Pen => Some(DrawTool::Pen),
        ToolId::Pencil if app.session.tool.pencil_brush.is_some() => Some(DrawTool::Brush),
        ToolId::Pencil => Some(DrawTool::Pencil),
        _ => None,
    }
}

/// Choose illustrated tool `tool`: the Pen, or the Pencil with a bristle brush or a plain stroke.
fn pick_draw_tool(app: &mut FigApp, tool: DrawTool) {
    match tool {
        DrawTool::Pen => choose(app, ToolId::Pen),
        DrawTool::Brush => {
            app.run("tool.pencilBrush", json!({"preset": "bristle"}));
            choose(app, ToolId::Pencil);
        }
        DrawTool::Pencil => {
            app.run("tool.pencilBrush", json!({"preset": null}));
            choose(app, ToolId::Pencil);
        }
    }
}

/// Illustrated tool `i`'s picture as a texture, decoded once.
fn draw_texture(ctx: &egui::Context, i: usize) -> Option<egui::TextureHandle> {
    let id = egui::Id::unique(("draw_tool_texture", i));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return Some(t);
    }
    let (_, _, bytes, _) = DRAW_TOOLS.get(i)?;
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let image = egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw());
    // Drawn at a quarter of its size or less: mipmaps keep it smooth.
    let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
    let texture = ctx.load_texture(format!("figcraft-draw-tool-{i}"), image, options);
    ctx.data_mut(|d| d.insert_temp(id, texture.clone()));
    Some(texture)
}

/// A board's toolbar: moving around, ink, stickies, shapes, connectors, text, sections, stamps.
const BOARD_GROUPS: &[Group] = &[
    Group::tools("bmove", tr_noop("Move tools"), &[(ToolId::Move, Icon::Move), (ToolId::Hand, Icon::Hand)]),
    Group::tools(
        "ink",
        tr_noop("Drawing tools"),
        &[(ToolId::Marker, Icon::Marker), (ToolId::Highlighter, Icon::Highlighter), (ToolId::Eraser, Icon::Eraser)],
    ),
    Group { key: "sticky", name: tr_noop("Sticky note colors"), tools: &[(ToolId::Sticky, Icon::Board)], options: Options::Colors, extras: &[] },
    Group { key: "bshape", name: tr_noop("Shapes"), tools: &[(ToolId::BoardShape, Icon::Shapes)], options: Options::Shapes, extras: &[] },
    Group::tools("connector", tr_noop("Connector"), &[(ToolId::Connector, Icon::Connector)]),
    Group::tools("text", tr_noop("Text"), &[(ToolId::Text, Icon::Text)]),
    Group::tools("section", tr_noop("Section"), &[(ToolId::Section, Icon::Section)]),
    Group { key: "stamp", name: tr_noop("Stamps"), tools: &[(ToolId::Stamp, Icon::Stamp)], options: Options::Stamps, extras: &[] },
    Group::tools("comment", tr_noop("Comment"), &[(ToolId::Comment, Icon::Comment)]),
];

/// Return the last tool index remembered for a toolbar group, falling back to `default`.
fn remembered(ui: &Ui, key: &str, default: ToolId) -> ToolId {
    ui.data(|d| d.get_temp::<u8>(egui::Id::unique(("tb_last", key)))).and_then(|i| ToolId::ALL.get(i as usize).copied()).unwrap_or(default)
}

pub fn show(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let canvas = app.canvas.rect;
    app.ui.actions_rect = None;
    ui.data_mut(|d| d.insert_temp(active_id(), mode_fill(app.ui.right_tab)));
    // The page's automatic save: the toolbar stays, a quiet status says what's going on.
    let autosave = autosaving(app);
    save_status(app, ui, &t, autosave);
    // A file you can't edit now (view only, or on its way to a server): moving around only.
    if let Some(why) = app.session.active_read_only().filter(|_| !autosave) {
        view_only_toolbar(app, ui, why);
        return;
    }
    // Vector edit mode swaps the tools for the edit tools.
    if let Some(tool) = app.session.ds().ok().and_then(|d| d.vector_edit.as_ref()).map(|v| v.tool) {
        edit_toolbar(app, ui, tool);
        return;
    }
    // So does the shape builder, for its merge/remove modes.
    if app.session.tool.tool == ToolId::ShapeBuilder {
        builder_toolbar(app, ui);
        return;
    }
    let board = app.session.doc().is_ok_and(figcraft_engine::board::is_board);
    main_toolbar(app, ui, canvas, &t, board);
}

/// The main toolbar pill's width for the active file: the Actions palette matches it so the two
/// read as one bar.
pub(crate) fn bar_width(app: &FigApp) -> f32 {
    let board = app.session.doc().is_ok_and(figcraft_engine::board::is_board);
    if !board && app.ui.right_tab == crate::RightTab::Inspect {
        return dev_width();
    }
    if !board && draw_mode(app) {
        return draw_width();
    }
    main_width(if board { BOARD_GROUPS } else { GROUPS }, board)
}

/// Draw mode shows its toolbar: chosen, on the Design panel.
fn draw_mode(app: &FigApp) -> bool {
    app.ui.draw_mode && app.ui.right_tab == crate::RightTab::Design
}

/// The Draw pill's width: Move, the illustrated tools between dividers, Frame, the shapes, Text,
/// Comment, Actions, the divider and the mode switcher (625 pt, as in the kit).
fn draw_width() -> f32 {
    let group = TOOL_CELL + FLYOUT_GAP + FLYOUT_W + GAP;
    PAD + group + 2.0 * (DIVIDER + GAP) + 3.0 * DRAW_CELL + GAP + 3.0 * group + (TOOL_CELL + GAP) + TOOL_CELL + GAP + DIVIDER + GAP + MODE_W + PAD
}

/// The Dev Mode pill's width: its five tools, the divider and the mode switcher.
fn dev_width() -> f32 {
    PAD + DEV_TOOLS as f32 * (TOOL_CELL + GAP) + DIVIDER + GAP + MODE_W + PAD
}

/// The buttons of the Dev Mode row: Move, Copy colors, Measurement, Annotation, Comment.
const DEV_TOOLS: usize = 5;

fn anim_id() -> egui::Id {
    egui::Id::unique("toolbar_anim")
}

/// How far the switch to Dev Mode has got, as (the rows' slide, the pill's growth): 0 for the
/// design pill, 1 for the Dev Mode one. As in the kit the rows slide first and the pill follows a
/// little behind, each easing out.
fn mode_anim(app: &FigApp, ctx: &egui::Context, dev: bool) -> (f32, f32) {
    let time = if app.reduce_motion() { 0.0 } else { 0.66 };
    mode_progress(ctx.animate_bool_with_time(egui::Id::unique("toolbar_dev"), dev, time))
}

/// A switch into or out of Draw mode: which way, when, and where it started from (the rows'
/// slide and the pill's growth, each 0 for the design toolbar and 1 for the Draw one).
#[derive(Clone, Copy, Debug)]
struct DrawSwitch {
    on: bool,
    at: f64,
    from: (f32, f32),
}

/// As UI3's toolbar: the rows slide over 0.3 s, the pill's width follows from 0.2 s on over
/// 0.5 s, each with CSS's `ease`; the same order both ways.
const DRAW_SLIDE_SECS: f32 = 0.3;
const DRAW_GROW_DELAY: f32 = 0.2;
const DRAW_GROW_SECS: f32 = 0.5;

/// CSS's `ease` (cubic-bezier(0.25, 0.1, 0.25, 1)) at `x` (0..1).
fn css_ease(x: f32) -> f32 {
    let x = if x.is_nan() { 1.0 } else { x.clamp(0.0, 1.0) };
    if x == 0.0 || x == 1.0 {
        return x;
    }
    let bezier = |t: f32, a: f32, b: f32| 3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        if bezier(mid, 0.25, 0.25) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bezier(0.5 * (lo + hi), 0.1, 1.0)
}

/// Switch `s`'s (slide, growth) `t` seconds after it began.
fn draw_progress(s: DrawSwitch, t: f64) -> (f32, f32) {
    let to = if s.on { 1.0 } else { 0.0 };
    let t = t as f32;
    let slide = s.from.0 + (to - s.from.0) * css_ease(t / DRAW_SLIDE_SECS);
    let grow = s.from.1 + (to - s.from.1) * css_ease((t - DRAW_GROW_DELAY) / DRAW_GROW_SECS);
    (slide, grow)
}

/// How far the switch to Draw mode has got, as (the rows' slide, the pill's growth): the Draw
/// row rises into the pill as the design row leaves through its top, the pill following.
fn draw_anim(app: &FigApp, ctx: &egui::Context, draw: bool) -> (f32, f32) {
    let id = egui::Id::unique("toolbar_draw");
    let now = ctx.input(|i| i.time);
    let end = if draw { 1.0 } else { 0.0 };
    let s = match ctx.data(|d| d.get_temp::<DrawSwitch>(id)) {
        // Turned around (or turned again mid-way): from wherever it is now.
        Some(s) if s.on != draw => DrawSwitch { on: draw, at: now, from: draw_progress(s, now - s.at) },
        Some(s) => s,
        None => DrawSwitch { on: draw, at: f64::NEG_INFINITY, from: (end, end) },
    };
    ctx.data_mut(|d| d.insert_temp(id, s));
    if app.reduce_motion() {
        return (end, end);
    }
    let p = draw_progress(s, now - s.at);
    if p != (end, end) {
        ctx.request_repaint();
    }
    p
}

/// The slide and the growth at `u` (0..1, the switch's linear progress): the rows slide over the
/// first half, the pill follows from 40% on, each easing out.
fn mode_progress(u: f32) -> (f32, f32) {
    let ease = |x: f32| egui::emath::easing::cubic_out(x.clamp(0.0, 1.0));
    (ease(u / 0.5), ease((u - 0.4) / 0.6))
}

/// The main toolbar pill's width: the tool groups with their flyout carets, Actions, the divider
/// and the mode switcher (boards: the templates and the Design panel's buttons instead).
fn main_width(groups: &[Group], board: bool) -> f32 {
    let tools: f32 = groups.iter().map(|g| TOOL_CELL + if g.has_flyout() { FLYOUT_GAP + FLYOUT_W } else { 0.0 }).sum();
    // The end, and the gaps that aren't a group's own: after Actions and after the divider (and
    // between a board's two buttons, which the end counts as one item).
    let (end, gaps) = if board { (2.0 * TOOL_CELL, 3.0) } else { (MODE_W, 2.0) };
    PAD * 2.0 + tools + (groups.len() as f32 + gaps) * GAP + TOOL_CELL + DIVIDER + end
}

/// The main toolbar pill: the tool groups with their flyouts, Actions, then the
/// Draw · Design · Prototype · Inspect mode switcher (on a board, its two buttons). In Dev Mode the
/// tools give way to the Dev Mode row (the rows slide, the pill shrinks round the switcher).
fn main_toolbar(app: &mut FigApp, ui: &mut Ui, canvas: Rect, t: &Tokens, board: bool) {
    let groups = if board { BOARD_GROUPS } else { GROUPS };
    // Boards have no Dev Mode.
    let dev = !board && app.ui.right_tab == crate::RightTab::Inspect;
    if dev {
        app.ui.tool_flyout = None;
    } else {
        app.ui.dev.measure_on = false;
    }
    let (slide, grow) = if board { (0.0, 0.0) } else { mode_anim(app, ui.ctx(), dev) };
    // Draw mode (on the Design panel only) swaps the design row for its own the same way.
    let draw = !board && draw_mode(app);
    let (dslide, dgrow) = if board { (0.0, 0.0) } else { draw_anim(app, ui.ctx(), draw) };
    let design_w = main_width(groups, board);
    let tools_w = design_w + (draw_width() - design_w) * dgrow;
    let width = tools_w + (dev_width() - tools_w) * grow;
    ui.data_mut(|d| d.insert_temp(anim_id(), (slide, grow, width)));
    let rect = pill(ui, &mut app.ui.nav, canvas, width, t);
    app.ui.tour_anchors.toolbar = Some(rect);
    let cy = rect.center().y;
    let scroll = pill_scroll(ui, rect, width);
    // A pill wider than the canvas (a phone) scrolls, its divider and switcher after the buttons.
    let scrolling = width > rect.width() + 0.5;
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(rect.intersect(outer_clip));
    let mut x = rect.min.x + PAD - scroll;
    // The design row slides down out of the pill as the Dev Mode row comes in from above.
    if slide < 1.0 {
        let row_y = cy + slide * PILL_H;
        let start = x;
        // The row on its way out first, so the one coming in has the last word (the Actions
        // button's place); each where its slide puts it.
        let design_row = |app: &mut FigApp, ui: &mut Ui| {
            let y = row_y - dslide * PILL_H;
            let mut x = start;
            for g in groups {
                x = group_cell(app, ui, g, pos2(x, y), rect.min.y, t);
            }
            actions_button(app, ui, x, y, t)
        };
        // Its own ids: both rows are on screen while they swap.
        let draw_tools = |app: &mut FigApp, ui: &mut Ui| {
            let y = row_y + (1.0 - dslide) * PILL_H;
            ui.push_id("draw_row", |ui| {
                let x = draw_row(app, ui, start, y, rect, outer_clip, t);
                actions_button(app, ui, x, y, t)
            })
            .inner
        };
        let (shown_design, shown_draw) = (dslide < 1.0, dslide > 0.0);
        if draw {
            if shown_design {
                design_row(app, ui);
            }
            x = draw_tools(app, ui);
        } else {
            if shown_draw {
                draw_tools(app, ui);
            }
            x = design_row(app, ui);
        }
    }
    if slide > 0.0 {
        let end = dev_row(app, ui, rect.min.x + PAD, cy + (slide - 1.0) * PILL_H, t);
        if dev {
            x = end;
        }
    }
    if board {
        divider(ui, t, x, cy);
        x += DIVIDER + GAP;
        // Boards have no prototypes or Dev Mode; the Design panel is hidden until asked for.
        x = crate::board_picker::button(app, ui, pos2(x, cy), rect.min.y, t) + GAP;
        let r = Rect::from_center_size(pos2(x + TOOL_CELL / 2.0, cy), vec2(TOOL_CELL, TOOL_CELL));
        let tip = crate::shortcuts::tip(tr("Design panel"), "view.design");
        if tool_cell(ui, t, r, "board_panel", Icon::Panels, app.ui.board_panel, tip).clicked() {
            app.run("view.design", json!({}));
        }
    } else {
        // The switcher stays at the pill's right end while the tools before it change.
        let switcher = if scrolling { x + DIVIDER + GAP } else { rect.max.x - PAD - MODE_W };
        divider(ui, t, if scrolling { x } else { switcher - GAP - DIVIDER }, cy);
        mode_switcher(app, ui, switcher, cy, t);
    }
    ui.set_clip_rect(outer_clip);
    help_button(app, ui, canvas, rect, t);
}

/// The Draw row from `x` (its left edge; `cy`, the row's middle) in `pill`: Move, the
/// illustrated pen, brush and pencil between dividers, then Frame, the shapes, Text and Comment.
/// The tools stand up out of the pill's bottom edge and may rise above its top (inside
/// `outer_clip`). Where Actions starts.
fn draw_row(app: &mut FigApp, ui: &mut Ui, x: f32, cy: f32, pill: Rect, outer_clip: Rect, t: &Tokens) -> f32 {
    let (first, rest) = DRAW_GROUPS.split_first().map_or((None, DRAW_GROUPS), |(f, r)| (Some(f), r));
    let mut x = x;
    if let Some(g) = first {
        x = group_cell(app, ui, g, pos2(x, cy), pill.min.y, t);
    }
    divider(ui, t, x, cy);
    x += DIVIDER + GAP;
    let shift = cy - pill.center().y;
    // The pill's bottom always cuts the tools off; they may stand above its top only while the
    // row is in place (sliding in or out, the pill clips them).
    let top = pill.min.y - DRAW_CHOSEN_RISE + shift.abs().min(DRAW_CHOSEN_RISE);
    let clip = Rect::from_min_max(pos2(x, top), pos2(x + 3.0 * DRAW_CELL, pill.max.y));
    let painter = ui.painter().with_clip_rect(clip.intersect(outer_clip));
    let chosen = drawn(app);
    let time = if app.reduce_motion() { 0.0 } else { 0.15 };
    for (i, (tool, name, _, [w, h])) in DRAW_TOOLS.iter().enumerate() {
        let cell = Rect::from_min_max(pos2(x, pill.min.y), pos2(x + DRAW_CELL, pill.max.y)).translate(vec2(0.0, shift));
        let tip = match tool {
            DrawTool::Pen => format!("{}  {}", tr(name), pretty_shortcut(ToolId::Pen.shortcut())),
            DrawTool::Pencil => format!("{}  {}", tr(name), pretty_shortcut(ToolId::Pencil.shortcut())),
            DrawTool::Brush => tr(name).to_string(),
        };
        let resp = ui.interact(cell, ui.scope_id().with(("draw_tool", i)), Sense::click()).on_hover_text(&tip);
        let on = chosen == Some(*tool);
        a11y::toggle(&resp, &tip, on);
        if on {
            a11y::prefer(&mut app.ui.nav, Region::Toolbar, resp.id);
            // The chosen tool stands on a light cell open at the bottom.
            let under = Rect::from_min_max(pos2(cell.min.x, cell.min.y + 6.0), pos2(cell.max.x, cell.max.y + 8.0));
            painter.rect_filled(under, CornerRadius::same(5), t.hover);
        }
        let target = if on {
            DRAW_CHOSEN_RISE
        } else if resp.hovered() {
            DRAW_HOVER_RISE
        } else {
            0.0
        };
        let rise = ui.ctx().animate_value_with_time(resp.id.with("rise"), target, time);
        if let Some(texture) = draw_texture(ui.ctx(), i) {
            let r = Rect::from_min_size(pos2(cell.center().x - w / 2.0, cell.min.y + DRAW_REST - rise), vec2(*w, *h));
            painter.image(texture.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if resp.clicked() {
            pick_draw_tool(app, *tool);
        }
        x += DRAW_CELL;
    }
    x += GAP;
    divider(ui, t, x, cy);
    x += DIVIDER + GAP;
    for g in rest {
        x = group_cell(app, ui, g, pos2(x, cy), pill.min.y, t);
    }
    x
}

/// The Dev Mode row from `x` (its left edge, the row's middle): Move, Copy colors, Measurement,
/// Annotation and Comment. Where the next part starts.
fn dev_row(app: &mut FigApp, ui: &mut Ui, x: f32, cy: f32, t: &Tokens) -> f32 {
    let tool = app.session.tool.tool;
    let (measure, copy) = (app.ui.dev.measure_on, app.ui.dev.copy_colors);
    let cell = |x: f32| Rect::from_center_size(pos2(x + TOOL_CELL / 2.0, cy), vec2(TOOL_CELL, TOOL_CELL));
    let step = TOOL_CELL + GAP;
    let mut x = x;

    let tip = format!("{}  {}", tr(ToolId::Move.label()), pretty_shortcut(ToolId::Move.shortcut()));
    let active = tool == ToolId::Move && !measure && !copy;
    let resp = tool_cell(ui, t, cell(x), ("dev", "move"), Icon::Move, active, &tip);
    if active {
        a11y::prefer(&mut app.ui.nav, Region::Toolbar, resp.id);
    }
    if resp.clicked() {
        dev_reset(app);
        choose(app, ToolId::Move);
    }
    x += step;

    // Copy colors samples a colour from the canvas onto the clipboard (the picker's eyedropper,
    // which only samples).
    if tool_cell(ui, t, cell(x), ("dev", "copy"), Icon::DevCopyColors, copy, tr("Copy colors")).clicked() {
        dev_reset(app);
        if !copy {
            app.ui.dev.copy_colors = true;
            app.run("edit.pickColor", json!({"apply": false}));
        }
    }
    x += step;

    // Measurement measures from the selection to the layer under the pointer, as holding Alt does.
    if tool_cell(ui, t, cell(x), ("dev", "measure"), Icon::DevMeasure, measure, tr("Measurement")).clicked() {
        dev_reset(app);
        app.ui.dev.measure_on = !measure;
    }
    x += step;

    // Annotation starts a note on the selected layer: its field in the Inspect tab takes the keys.
    if tool_cell(ui, t, cell(x), ("dev", "annotate"), Icon::DevAnnotation, false, tr("Annotation")).clicked() {
        match app.session.selection().first().copied() {
            Some(id) => ui.ctx().memory_mut(|m| m.request_focus(egui::Id::unique(("annot_new", id)).with("te"))),
            None => app.session.message = Some(tr("Select a layer to annotate").to_string()),
        }
    }
    x += step;

    let tip = format!("{}  {}", tr(ToolId::Comment.label()), pretty_shortcut(ToolId::Comment.shortcut()));
    if tool_cell(ui, t, cell(x), ("dev", "comment"), Icon::Comment, tool == ToolId::Comment, &tip).clicked() {
        dev_reset(app);
        choose(app, ToolId::Comment);
    }
    x + step
}

/// Leave Dev Mode's Measurement and Copy colors (a sampling tool in progress included).
fn dev_reset(app: &mut FigApp) {
    app.ui.dev.measure_on = false;
    if std::mem::take(&mut app.ui.dev.copy_colors) && app.session.tool.tool == ToolId::Eyedropper {
        app.run("tool.move", json!({}));
    }
}

/// How far the buttons of a pill `width` wide drawn at `rect` are scrolled: wider than the pill (a
/// phone), they slide sideways under a drag or the wheel.
fn pill_scroll(ui: &Ui, rect: Rect, width: f32) -> f32 {
    let scroll_id = ui.scope_id().with("tb_scroll");
    let max_scroll = (width - rect.width()).max(0.0);
    let mut scroll = 0.0;
    if max_scroll > 0.0 {
        let drag = ui.ctx().read_response(ui.scope_id().with("pill")).map_or(0.0, |r| r.drag_delta().x);
        let wheel = if ui.rect_contains_pointer(rect) { ui.input(|i| i.smooth_scroll_delta.x + i.smooth_scroll_delta.y) } else { 0.0 };
        scroll = (ui.data(|d| d.get_temp::<f32>(scroll_id)).unwrap_or(0.0) - drag - wheel).clamp(0.0, max_scroll);
    }
    ui.data_mut(|d| d.insert_temp(scroll_id, scroll));
    scroll
}

/// Tool group `g` at `at` (its left edge, the pill's middle): the shown tool's button, its flyout
/// caret and, while open, its flyout above the pill (whose top is `top`). Where the next starts.
fn group_cell(app: &mut FigApp, ui: &mut Ui, g: &Group, at: egui::Pos2, top: f32, t: &Tokens) -> f32 {
    let current = app.session.tool.tool;
    let in_group = g.tools.iter().any(|(tid, _)| *tid == current);
    let shown = if in_group { current } else { remembered(ui, g.key, g.tools.first().map(|x| x.0).unwrap_or(ToolId::Move)) };
    let icon = g.tools.iter().find(|(tid, _)| *tid == shown).map(|(_, i)| *i).unwrap_or(Icon::Move);
    let r = Rect::from_center_size(pos2(at.x + TOOL_CELL / 2.0, at.y), vec2(TOOL_CELL, TOOL_CELL));
    let resp = tool_cell(ui, t, r, ("tool", g.key), icon, in_group, &format!("{}  {}", tr(shown.label()), pretty_shortcut(shown.shortcut())));
    if in_group {
        a11y::prefer(&mut app.ui.nav, Region::Toolbar, resp.id);
    }
    if resp.clicked() {
        // Enter (a keyboard click) on the shape or frame tool already chosen places one; the
        // canvas takes the keyboard.
        // In Dev Mode the tool goes through `choose`, which returns to the design first.
        if in_group && shown.draws_shape() && app.ui.right_tab != crate::RightTab::Inspect && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            app.run("tool.place", json!({}));
            app.ui.nav.go(Region::Canvas);
        } else {
            choose(app, shown);
        }
    }
    let mut x = at.x + TOOL_CELL;
    if g.has_flyout() {
        flyout_caret(app, ui, g, pos2(x, at.y), t);
        x += FLYOUT_GAP + FLYOUT_W;
    }
    if app.ui.tool_flyout == Some(g.key) {
        if g.options == Options::None {
            flyout(app, ui, g, pos2(r.min.x, top - 8.0));
        } else {
            options_flyout(app, ui, g, pos2(r.min.x, top - 8.0));
        }
    }
    x + GAP
}

/// The caret after group `g`'s button (at `at`, its left edge and middle): opens or closes its
/// flyout.
fn flyout_caret(app: &mut FigApp, ui: &mut Ui, g: &Group, at: egui::Pos2, t: &Tokens) {
    let cr = Rect::from_min_size(pos2(at.x + FLYOUT_GAP, at.y - TOOL_CELL / 2.0), vec2(FLYOUT_W, TOOL_CELL));
    let cresp = ui.interact(cr, ui.scope_id().with(("flyout", g.key)), Sense::click());
    let open = app.ui.tool_flyout == Some(g.key);
    a11y::menu_button(&cresp, tr(g.name), open);
    if open {
        a11y::popup(&mut app.ui.nav, cresp.id, flyout_id(g.key));
    }
    if cresp.hovered() {
        ui.painter().rect_filled(cr, CornerRadius::same(4), t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(cr.center(), vec2(CARET, CARET)), Icon::Chevron, t.icon);
    if cresp.clicked() {
        app.ui.tool_flyout = if open { None } else { Some(g.key) };
    }
}

/// The Actions palette's button, from `x`. Where the next part starts.
fn actions_button(app: &mut FigApp, ui: &mut Ui, x: f32, cy: f32, t: &Tokens) -> f32 {
    let r = Rect::from_center_size(pos2(x + TOOL_CELL / 2.0, cy), vec2(TOOL_CELL, TOOL_CELL));
    let resp = ui.interact(r, ui.scope_id().with("actions"), Sense::click()).tip(crate::shortcuts::tip(tr("Actions"), "app.actions"));
    app.ui.actions_rect = Some(r);
    // The Actions button reads on while its panel is open; clicking it again closes it.
    let open = app.ui.palette.is_some();
    if open {
        ui.painter().rect_filled(r, CornerRadius::same(5), active_fill(ui));
    } else if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), glyph_rect(r), Icon::Actions, if open { Color32::WHITE } else { t.icon });
    if resp.clicked() {
        crate::menus::toggle_palette(&mut app.ui);
    }
    x + TOOL_CELL + GAP
}

/// The mode switcher from `x`: Draw · Design · Prototype · Inspect (code).
fn mode_switcher(app: &mut FigApp, ui: &mut Ui, x: f32, cy: f32, t: &Tokens) {
    let modes = [(Icon::ModeDraw, tr("Draw")), (Icon::ModeDesign, tr("Design")), (Icon::ModePrototype, tr("Prototype")), (Icon::Code, tr("Inspect"))];
    let active = match app.ui.right_tab {
        crate::RightTab::Design if app.ui.draw_mode => 0,
        crate::RightTab::Design => 1,
        crate::RightTab::Prototype => 2,
        crate::RightTab::Inspect => 3,
    };
    let seg = Rect::from_min_size(pos2(x, cy - TOOL_CELL / 2.0), vec2(MODE_W, TOOL_CELL));
    ui.painter().rect_filled(seg, CornerRadius::same(5), t.field);
    let cell =
        |i: f32| Rect::from_min_size(pos2(seg.min.x + MODE_INSET + i * (MODE_CELL + MODE_INSET), seg.min.y + MODE_INSET), vec2(MODE_CELL, MODE_CELL));
    // The current mode lifts off the field: a white cell with a soft drop, gliding to a new mode.
    let glide = if app.reduce_motion() { 0.0 } else { 0.08 };
    let at = ui.ctx().animate_value_with_time(egui::Id::unique("toolbar_mode_highlight"), active as f32, glide);
    let lifted = cell(at);
    let radius = CornerRadius::same(3);
    ui.painter().add(egui::Shadow { offset: [0, 1], blur: 3, spread: 0, color: Color32::from_black_alpha(38) }.as_shape(lifted, radius));
    ui.painter().rect_filled(lifted, radius, t.panel);
    ui.painter().rect_stroke(lifted, radius, Stroke::new(0.5, Color32::from_black_alpha(30)), StrokeKind::Outside);
    for (i, (icon, tip)) in modes.iter().enumerate() {
        let r = cell(i as f32);
        let resp = ui.interact(r, ui.scope_id().with(("mode", i)), Sense::click()).on_hover_text(*tip);
        a11y::toggle(&resp, tip, i == active);
        if i != active && resp.hovered() {
            ui.painter().rect_filled(r, CornerRadius::same(3), t.hover);
        }
        icons::paint(
            ui.painter(),
            Rect::from_center_size(r.center(), vec2(MODE_GLYPH, MODE_GLYPH)),
            *icon,
            if i != active {
                t.icon_secondary
            } else if i == 3 {
                HANDOFF_GREEN
            } else if t.dark {
                MODE_BLUE_DARK
            } else {
                MODE_BLUE
            },
        );
        if resp.clicked() {
            pick_mode(app, i);
        }
    }
}

/// Mode `i` of the switcher chosen. The Prototype mode also brings up the motion timeline under
/// the canvas; the other modes put it away.
fn pick_mode(app: &mut FigApp, i: usize) {
    match i {
        0 => {
            app.ui.right_tab = crate::RightTab::Design;
            app.ui.draw_mode = true;
        }
        1 => {
            app.ui.right_tab = crate::RightTab::Design;
            app.ui.draw_mode = false;
        }
        2 => {
            app.ui.right_tab = crate::RightTab::Prototype;
            app.ui.draw_mode = false;
        }
        _ => {
            app.ui.right_tab = crate::RightTab::Inspect;
            app.ui.draw_mode = false;
            // Dev Mode only inspects: an editing tool gives way to Move.
            if !matches!(app.session.tool.tool, ToolId::Move | ToolId::Hand | ToolId::Comment) {
                app.run("tool.set", json!({"tool": "move"}));
            }
        }
    }
    let prototype = i == 2;
    if app.session.ds().is_ok_and(|d| d.motion.panel != prototype) {
        app.run("view.motionTimeline", json!({"show": prototype}));
    }
}

/// The floating help button at the bottom right of the canvas.
fn help_button(app: &mut FigApp, ui: &mut Ui, canvas: Rect, pill: Rect, t: &Tokens) {
    let mut hr = Rect::from_center_size(pos2(canvas.max.x - 32.0, canvas.max.y - 32.0), vec2(32.0, 32.0));
    // Above the pill when they'd overlap (a narrow window).
    if hr.intersects(pill) {
        hr = hr.translate(vec2(0.0, -(pill.height() + 12.0)));
    }
    let hresp = ui.interact(hr, ui.scope_id().with("help"), Sense::click()).tip(tr("Help and resources"));
    ui.painter().circle_filled(hr.center() + vec2(0.0, 1.0), 17.0, Color32::from_black_alpha(20));
    ui.painter().circle_filled(hr.center(), 16.0, t.panel);
    ui.painter().text(hr.center(), Align2::CENTER_CENTER, "?", crate::theme::regular(15.0), t.text);
    crate::right::menu_popup(&hresp).show(|ui| {
        for (l, c) in [
            (tr("Getting started"), "help.gettingStarted"),
            (tr("Keyboard shortcuts"), "help.shortcuts"),
            (tr("Help page"), "help.page"),
            (tr("Check for updates…"), "help.checkForUpdates"),
            (tr("About FigCraft"), "help.about"),
        ] {
            if crate::right::menu_item(ui, l, crate::shortcuts::shortcut(c).unwrap_or("")) {
                crate::menus::invoke(app, c);
                ui.close();
            }
        }
    });
}

/// The active file is on its way to the server because the page is keeping it there
/// (`collab.share {quiet: true}`): edits wait for the upload, but nothing in the toolbar changes.
fn autosaving(app: &FigApp) -> bool {
    app.session.active_read_only() == Some(figcraft_engine::live::PUBLISHING) && app.session.live().quiet
}

/// How long "Saved" stays up, in seconds.
const SAVED_FOR: f64 = 2.5;

/// "Saving…" while the automatic save uploads, then "Saved" for a moment: a quiet chip above the
/// toolbar.
fn save_status(app: &FigApp, ui: &Ui, t: &Tokens, saving: bool) {
    let ctx = ui.ctx();
    let now = ctx.input(|i| i.time);
    let id = egui::Id::unique("autosave_status");
    // (it was saving last frame, when "Saved" went up)
    let (was_saving, saved_at) = ctx.data(|d| d.get_temp::<(bool, Option<f64>)>(id)).unwrap_or((false, None));
    let saved_at = match (saving, was_saving) {
        (true, _) => None,
        (false, true) => Some(now),
        (false, false) => saved_at,
    };
    ctx.data_mut(|d| d.insert_temp(id, (saving, saved_at)));
    let label = if saving {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        tr("Saving…")
    } else if let Some(at) = saved_at.filter(|at| now - at < SAVED_FOR) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((SAVED_FOR - (now - at)).max(0.05)));
        tr("Saved")
    } else {
        return;
    };
    let text = ui.painter().layout_no_wrap(label.to_string(), crate::theme::regular(11.5), t.text_secondary);
    let size = vec2(text.size().x + 20.0, 22.0);
    let bar = pill_rect(app.canvas.rect, 0.0);
    let chip = Rect::from_center_size(pos2(bar.center().x, bar.min.y - 8.0 - size.y / 2.0), size);
    let resp = ui.interact(chip, ui.scope_id().with("autosave"), Sense::hover());
    a11y::status(&resp, label, label);
    ui.painter().rect_filled(chip, CornerRadius::same(11), t.panel);
    ui.painter().rect_stroke(chip, CornerRadius::same(11), Stroke::new(0.5, Color32::from_black_alpha(46)), StrokeKind::Outside);
    ui.painter().galley(pos2(chip.min.x + 10.0, chip.center().y - text.size().y / 2.0), text, t.text_secondary);
}

/// The pill shown for a file you can't edit now: Move and Hand (and Comment with a comment link),
/// and a badge saying why ("View only", "Can comment", or "Publishing…" until the file is on the
/// server).
fn view_only_toolbar(app: &mut FigApp, ui: &mut Ui, why: &'static str) {
    let t = Tokens::get(ui.ctx());
    // The editing tools are off: the tool falls back to moving around.
    if !figcraft_engine::live::tool_left(why, app.session.tool.tool) {
        app.run("tool.set", json!({"tool": "move"}));
    }
    let comment = why == figcraft_engine::live::COMMENT_ONLY;
    let moving = [(ToolId::Move, Icon::Move), (ToolId::Hand, Icon::Hand), (ToolId::Comment, Icon::Comment)];
    let tools = if comment { &moving[..] } else { &moving[..2] };
    let (label, badge_icon) = match why {
        figcraft_engine::live::PUBLISHING => (tr("Publishing…"), Icon::Eye),
        _ if comment => (tr("Can comment"), Icon::Comment),
        _ => (tr("View only"), Icon::Eye),
    };
    let text = ui.painter().layout_no_wrap(label.to_string(), crate::theme::semibold(11.5), t.text);
    let badge_w = 12.0 + 14.0 + 6.0 + text.size().x + 12.0;
    let width = PAD * 2.0 + tools.len() as f32 * (TOOL_CELL + GAP) + DIVIDER + GAP + badge_w;
    let rect = pill(ui, &mut app.ui.nav, app.canvas.rect, width, &t);
    let cy = rect.center().y;
    let mut x = rect.min.x + PAD;
    for &(tool, icon) in tools {
        let r = Rect::from_center_size(pos2(x + TOOL_CELL / 2.0, cy), vec2(TOOL_CELL, TOOL_CELL));
        let active = app.session.tool.tool == tool;
        let resp = tool_cell(ui, &t, r, ("tool", tool.id()), icon, active, &format!("{}  {}", tr(tool.label()), pretty_shortcut(tool.shortcut())));
        if active {
            a11y::prefer(&mut app.ui.nav, Region::Toolbar, resp.id);
        }
        if resp.clicked() {
            app.run("tool.set", json!({"tool": tool.id()}));
        }
        x += TOOL_CELL + GAP;
    }
    divider(ui, &t, x, cy);
    let badge = Rect::from_min_size(pos2(x + DIVIDER + GAP, cy - 13.0), vec2(badge_w, 26.0));
    let resp = ui.interact(badge, ui.scope_id().with("view_only"), Sense::hover()).on_hover_text(why);
    a11y::status(&resp, label, why);
    ui.painter().rect_filled(badge, CornerRadius::same(13), t.field);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(badge.min.x + 12.0 + 7.0, cy), vec2(14.0, 14.0)), badge_icon, t.icon_secondary);
    ui.painter().galley(pos2(badge.min.x + 12.0 + 14.0 + 6.0, cy - text.size().y / 2.0), text, t.text);
}

/// The pill shown in vector edit mode: the edit tools and Done.
fn edit_toolbar(app: &mut FigApp, ui: &mut Ui, current: VTool) {
    let t = Tokens::get(ui.ctx());
    let canvas = app.canvas.rect;
    let width = VTool::ALL.len() as f32 * (TOOL_CELL + GAP) + PAD * 2.0 + DIVIDER + GAP + DONE_W;
    let rect = pill(ui, &mut app.ui.nav, canvas, width, &t);
    let cy = rect.center().y;
    let mut x = rect.min.x + PAD;
    // Holding Cmd with the Move tool bends: show it.
    let held = current == VTool::Move && ui.input(|i| i.modifiers.command);
    for tool in VTool::ALL {
        let r = Rect::from_center_size(pos2(x + TOOL_CELL / 2.0, cy), vec2(TOOL_CELL, TOOL_CELL));
        let tip = if tool == VTool::Bend {
            trf("{tool}  hold {key}", &[("tool", &tr(tool.label())), ("key", &pretty_shortcut(tool.shortcut()))])
        } else {
            format!("{}  {}", tr(tool.label()), pretty_shortcut(tool.shortcut()))
        };
        let active = tool == current && !held || held && tool == VTool::Bend;
        let icon = match tool {
            VTool::Move => Icon::Move,
            VTool::Pen => Icon::Pen,
            VTool::Bend => Icon::Bend,
            VTool::Paint => Icon::PaintBucket,
            VTool::Lasso => Icon::Lasso,
            VTool::Cut => Icon::Cut,
            VTool::Eraser => Icon::Eraser,
            VTool::Width => Icon::VariableWidth,
        };
        let resp = tool_cell(ui, &t, r, ("vtool", tool.id()), icon, active, &tip);
        if active {
            a11y::prefer(&mut app.ui.nav, Region::Toolbar, resp.id);
        }
        if resp.clicked() {
            app.run("vector.edit.tool", json!({"tool": tool.id()}));
        }
        x += TOOL_CELL + GAP;
    }
    if done_button(ui, &t, x, cy, &format!("{}  Esc", tr("Done editing"))) {
        app.run("vector.edit.end", json!({}));
    }
}

/// The pill shown while the shape builder is active: Merge and Remove modes, a hint and Done.
fn builder_toolbar(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let remove = app.session.tool.builder_remove;
    let has_regions = figcraft_engine::shapebuilder::current(&app.session).is_some_and(|a| !a.regions.is_empty());
    let alt = pretty_shortcut("Alt");
    let hint = if !has_regions {
        tr("Select overlapping shapes").to_string()
    } else if remove {
        trf("Click or drag to remove · {alt} merges", &[("alt", &alt)])
    } else {
        trf("Click or drag to merge · {alt} removes", &[("alt", &alt)])
    };
    let galley = ui.painter().layout_no_wrap(hint, regular(12.0), t.text_secondary);
    let width = PAD * 2.0 + 2.0 * (TOOL_CELL + GAP) + 2.0 * (DIVIDER + GAP) + galley.size().x + GAP + DONE_W;
    let rect = pill(ui, &mut app.ui.nav, app.canvas.rect, width, &t);
    let cy = rect.center().y;
    let mut x = rect.min.x + PAD;
    for (mode, icon, tip) in [("merge", Icon::ShapeBuilder, tr("Merge regions")), ("remove", Icon::ShapeBuilderRemove, tr("Remove regions"))] {
        let r = Rect::from_center_size(pos2(x + TOOL_CELL / 2.0, cy), vec2(TOOL_CELL, TOOL_CELL));
        let active = (mode == "remove") == remove;
        let resp = tool_cell(ui, &t, r, ("sbmode", mode), icon, active, tip);
        if active {
            a11y::prefer(&mut app.ui.nav, Region::Toolbar, resp.id);
        }
        if resp.clicked() {
            app.run("shapeBuilder.mode", json!({"mode": mode}));
        }
        x += TOOL_CELL + GAP;
    }
    divider(ui, &t, x, cy);
    x += DIVIDER + GAP;
    let w = galley.size().x;
    ui.painter().galley(pos2(x, cy - galley.size().y / 2.0), galley, t.text_secondary);
    x += w + GAP;
    if done_button(ui, &t, x, cy, &format!("{}  {}", tr("Done"), pretty_shortcut("Enter"))) {
        app.run("tool.set", json!({"tool": "move"}));
    }
}

/// Where a toolbar pill `width` wide sits: centred 12 px above the canvas's bottom edge.
/// (Never wider than the canvas less a margin: on a phone the main toolbar scrolls inside it.)
pub(crate) fn pill_rect(canvas: Rect, width: f32) -> Rect {
    let width = width.min((canvas.width() - 16.0).max(0.0));
    Rect::from_center_size(pos2(canvas.center().x, canvas.max.y - 12.0 - PILL_H / 2.0), vec2(width, PILL_H))
}

/// The floating pill (shadow, panel, border) `width` wide at the bottom centre of the canvas: the
/// toolbar region keyboard navigation moves through.
fn pill(ui: &Ui, nav: &mut Nav, canvas: Rect, width: f32, t: &Tokens) -> Rect {
    let rect = pill_rect(canvas, width);
    // The pill takes the press and the drag over it: its buttons sense clicks only, and egui gives
    // a drag the canvas under them would sense to the canvas, so the pen placed an anchor there.
    // Not focusable: the keyboard walks the buttons, not the pill.
    ui.interact(rect, ui.scope_id().with("pill"), Sense::CLICK | Sense::DRAG);
    a11y::area(nav, Region::Toolbar, rect);
    a11y::container_bounds(ui, rect);
    let painter = ui.painter();
    let radius = CornerRadius::same(PILL_RADIUS);
    // Two soft drops, then the pill, then a hairline round it: no border.
    for (dy, blur) in [(3, 4), (1, 2)] {
        painter.add(egui::Shadow { offset: [0, dy], blur, spread: 0, color: Color32::from_black_alpha(26) }.as_shape(rect, radius));
    }
    painter.rect_filled(rect, radius, t.panel);
    painter.rect_stroke(rect, radius, Stroke::new(0.5, Color32::from_black_alpha(46)), StrokeKind::Outside);
    rect
}

/// A square tool button: accent when active, tinted on hover; `tip` is its tooltip and name.
pub(crate) fn tool_cell(ui: &mut Ui, t: &Tokens, r: Rect, id: impl egui::AsIdSalt, icon: Icon, active: bool, tip: &str) -> egui::Response {
    let resp = ui.interact(r, ui.scope_id().with(id), Sense::click()).on_hover_ui(|ui| {
        ui.label(tip);
    });
    a11y::toggle(&resp, tip, active);
    let bg = if active {
        active_fill(ui)
    } else if resp.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, CornerRadius::same(5), bg);
    icons::paint(ui.painter(), glyph_rect(r), icon, if active { Color32::WHITE } else { t.icon });
    resp
}

/// A vertical separator just after `x`: DIVIDER wide, the pill's full height.
fn divider(ui: &Ui, t: &Tokens, x: f32, cy: f32) {
    let half = PILL_H / 2.0;
    ui.painter().line_segment([pos2(x + DIVIDER / 2.0, cy - half), pos2(x + DIVIDER / 2.0, cy + half)], Stroke::new(DIVIDER, t.border));
}

const DONE_W: f32 = 56.0;

/// A divider at `x` and the accent Done button after it; true when clicked.
fn done_button(ui: &mut Ui, t: &Tokens, x: f32, cy: f32, tip: &str) -> bool {
    divider(ui, t, x, cy);
    let done = Rect::from_min_size(pos2(x + DIVIDER + GAP, cy - 14.0), vec2(DONE_W, 28.0));
    let resp = ui.interact(done, ui.scope_id().with("tool_done"), Sense::click()).tip(tip);
    let fill = active_fill(ui);
    ui.painter().rect_filled(done, CornerRadius::same(6), if resp.hovered() { fill.gamma_multiply(0.9) } else { fill });
    ui.painter().text(done.center(), Align2::CENTER_CENTER, tr("Done"), crate::theme::semibold(11.5), Color32::WHITE);
    resp.clicked()
}

/// A board tool's flyout: the colours, shapes or stamps it places, in rows of up to eight. A pick
/// chooses the tool with it.
fn options_flyout(app: &mut FigApp, ui: &mut Ui, g: &Group, bottom_left: egui::Pos2) {
    let t = Tokens::get(ui.ctx());
    let Some(&(tool, _)) = g.tools.first() else { return };
    let items = crate::board_bar::choices(g.options, &app.session.tool.board);
    let (cell, pad) = (32.0, 8.0);
    let cols = items.len().clamp(1, 8);
    let rows = items.len().div_ceil(cols);
    let size = vec2(cols as f32 * (cell + 4.0) - 4.0 + 2.0 * pad, rows as f32 * (cell + 4.0) - 4.0 + 2.0 * pad);
    let mut chosen = None;
    egui::Area::new(flyout_id(g.key)).fixed_pos(pos2(bottom_left.x, bottom_left.y - size.y)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(13), t.menu);
        for (i, c) in items.iter().enumerate() {
            let at = vec2((i % cols) as f32, (i / cols) as f32) * (cell + 4.0);
            let r = Rect::from_min_size(rect.min + vec2(pad, pad) + at, vec2(cell, cell));
            let resp = ui.interact(r, ui.scope_id().with(("opt", g.key, i)), Sense::click()).on_hover_text(tr(c.label));
            a11y::menu_item(&resp, tr(c.label), "", Some(c.on));
            a11y::own_focus(&resp);
            if c.on || resp.hovered() || resp.has_focus() {
                ui.painter().rect_filled(r, CornerRadius::same(5), t.menu_hover);
            }
            crate::board_bar::paint_choice(ui.painter(), r.shrink(5.0), &c.mark, t.menu_text);
            if resp.clicked() {
                chosen = Some(c.params.clone());
            }
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.ui.tool_flyout = None;
    }
    if let Some(params) = chosen {
        app.run(&format!("tool.{}", tool.id()), params);
        app.ui.tool_flyout = None;
    }
}

/// Draw a tool group's flyout above its button, with keyboard/hover highlighting, remembered
/// selection when a tool is clicked, and Escape to close.
fn flyout(app: &mut FigApp, ui: &mut Ui, g: &Group, bottom_left: egui::Pos2) {
    let t = Tokens::get(ui.ctx());
    let item_h = 32.0;
    let w = 220.0;
    let h = item_h * (g.tools.len() + g.extras.len()) as f32 + 16.0;
    let mut chosen = None;
    let mut ran = None;
    egui::Area::new(flyout_id(g.key)).fixed_pos(pos2(bottom_left.x, bottom_left.y - h)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(13), t.menu);
        let mut y = rect.min.y + 8.0;
        for (tid, icon) in g.tools {
            let r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(w - 16.0, item_h));
            let resp = ui.interact(r, ui.scope_id().with(("fly", tid.id())), Sense::click());
            a11y::menu_item(&resp, tr(tid.label()), tid.shortcut(), Some(*tid == app.session.tool.tool));
            // The keyboard's item is highlighted like the hovered one.
            a11y::own_focus(&resp);
            if resp.hovered() || resp.has_focus() {
                ui.painter().rect_filled(r, CornerRadius::same(5), t.menu_hover);
            }
            if *tid == app.session.tool.tool {
                icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 10.0, r.center().y), vec2(12.0, 12.0)), Icon::Check, t.menu_text);
            }
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 32.0, r.center().y), vec2(16.0, 16.0)), *icon, t.menu_text);
            ui.painter().text(pos2(r.min.x + 48.0, r.center().y), Align2::LEFT_CENTER, tr(tid.label()), regular(12.0), t.menu_text);
            ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, pretty_shortcut(tid.shortcut()), regular(12.0), t.menu_dim);
            if resp.clicked() {
                chosen = Some(*tid);
            }
            y += item_h;
        }
        // Rows that run a command (Image/video… opens the file dialog) follow the tools.
        for e in g.extras {
            let r = Rect::from_min_size(pos2(rect.min.x + 8.0, y), vec2(w - 16.0, item_h));
            let resp = ui.interact(r, ui.scope_id().with(("fly", e.cmd)), Sense::click());
            let keys = pretty_shortcut(crate::shortcuts::shortcut(e.cmd).unwrap_or(""));
            a11y::menu_item(&resp, tr(e.label), &keys, None);
            a11y::own_focus(&resp);
            if resp.hovered() || resp.has_focus() {
                ui.painter().rect_filled(r, CornerRadius::same(5), t.menu_hover);
            }
            icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 32.0, r.center().y), vec2(16.0, 16.0)), e.icon, t.menu_text);
            ui.painter().text(pos2(r.min.x + 48.0, r.center().y), Align2::LEFT_CENTER, tr(e.label), regular(12.0), t.menu_text);
            ui.painter().text(pos2(r.max.x - 8.0, r.center().y), Align2::RIGHT_CENTER, keys, regular(12.0), t.menu_dim);
            if resp.clicked() {
                ran = Some(e.cmd);
            }
            y += item_h;
        }
    });
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.ui.tool_flyout = None;
    }
    if let Some(cmd) = ran {
        crate::menus::invoke(app, cmd);
        app.ui.tool_flyout = None;
    }
    if let Some(tid) = chosen {
        if let Some(i) = ToolId::ALL.iter().position(|x| *x == tid) {
            ui.data_mut(|d| d.insert_temp(egui::Id::unique(("tb_last", g.key)), i as u8));
        }
        choose(app, tid);
        app.ui.tool_flyout = None;
    }
}

/// Choose `tool` from the toolbar. Dev Mode only inspects: a tool that edits leaves it for the
/// design.
fn choose(app: &mut FigApp, tool: ToolId) {
    if app.ui.right_tab == crate::RightTab::Inspect && !matches!(tool, ToolId::Move | ToolId::Hand | ToolId::Comment) {
        app.ui.right_tab = crate::RightTab::Design;
    }
    app.run("tool.set", json!({"tool": tool.id()}));
}

/// A tool group's flyout (an egui area).
fn flyout_id(key: &str) -> egui::Id {
    egui::Id::unique(("flyout_area", key))
}

/// "Shift+Cmd+K" → "⇧⌘K" on macOS, "Ctrl+Shift+K" elsewhere.
pub fn pretty_shortcut(s: &str) -> String {
    figcraft_engine::keymap::display(s, crate::shortcuts::platform())
}

#[cfg(test)]
pub(crate) fn mode_progress_for_tests(u: f32) -> (f32, f32) {
    mode_progress(u)
}

/// (slide, growth, width) of the main pill as last drawn.
#[cfg(test)]
pub(crate) fn anim_state(ctx: &egui::Context) -> Option<(f32, f32, f32)> {
    ctx.data(|d| d.get_temp(anim_id()))
}

/// The middle of the Dev Mode row's `i`th button in a pill at `pill`.
#[cfg(test)]
pub(crate) fn dev_cell_centre(pill: Rect, i: usize) -> egui::Pos2 {
    pos2(pill.min.x + PAD + i as f32 * (TOOL_CELL + GAP) + TOOL_CELL / 2.0, pill.center().y)
}

/// The middle of the shape group's flyout caret in a design pill at `pill`, and of the flyout's
/// last row once it is open.
#[cfg(test)]
pub(crate) fn shape_flyout_points(pill: Rect) -> (egui::Pos2, egui::Pos2) {
    let group = TOOL_CELL + FLYOUT_GAP + FLYOUT_W;
    let cell_left = pill.min.x + PAD + 2.0 * (group + GAP);
    let caret = pos2(cell_left + TOOL_CELL + FLYOUT_GAP + FLYOUT_W / 2.0, pill.center().y);
    let shape = &GROUPS[2];
    let rows = (shape.tools.len() + shape.extras.len()) as f32;
    let top = pill.min.y - 8.0 - (32.0 * rows + 16.0);
    (caret, pos2(cell_left + 60.0, top + 8.0 + 32.0 * (rows - 0.5)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Shape;

    #[test]
    fn pill_never_outgrows_a_phone_canvas() {
        let full = main_width(GROUPS, false);
        let phone = Rect::from_min_size(pos2(0.0, 0.0), vec2(375.0, 700.0));
        let pill = pill_rect(phone, full);
        assert!(pill.width() <= phone.width() - 16.0 && pill.min.x >= 8.0 && pill.max.x <= phone.max.x - 8.0, "{pill:?}");
        // Narrower than its items, so it scrolls under a drag or the wheel (#1209).
        assert!(full > pill.width(), "{full} in {}", pill.width());
        // A desktop canvas keeps the full width, and a degenerate one doesn't go negative.
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 800.0));
        assert_eq!(pill_rect(wide, full).width(), full);
        assert_eq!(pill_rect(Rect::from_min_size(pos2(0.0, 0.0), vec2(4.0, 4.0)), full).width(), 0.0);
    }

    /// The width the pill asks for is the width its items take — the groups, Actions, the divider
    /// and the end (the mode switcher, or a board's two buttons), PAD in from either end and one
    /// GAP apart — so its last item ends exactly PAD from its right edge.
    #[test]
    fn the_pill_is_as_wide_as_its_items() {
        let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 800.0));
        for (groups, board) in [(GROUPS, false), (BOARD_GROUPS, true)] {
            let pill = pill_rect(canvas, main_width(groups, board));
            let mut x = pill.min.x + PAD;
            for g in groups {
                x += TOOL_CELL + if g.has_flyout() { FLYOUT_GAP + FLYOUT_W } else { 0.0 } + GAP;
            }
            x += TOOL_CELL + GAP; // Actions
            x += DIVIDER + GAP; // the divider
            let end = if board { 2.0 * TOOL_CELL + GAP } else { MODE_W };
            assert!((x + end - (pill.max.x - PAD)).abs() < 0.01, "board {board}: {} vs {}", x + end, pill.max.x - PAD);
        }
    }

    /// The pill's cells follow the UI3 grid: 49 pt tool groups (button, 1 pt, caret) and a 122 pt
    /// mode switcher of four 28 pt segments. That makes the pill 529 pt wide, less the 17 pt
    /// caret of the Comment group, which has no other tools to offer.
    #[test]
    fn the_design_pill_is_on_the_ui3_grid() {
        assert_eq!(MODE_W, 122.0);
        assert_eq!(TOOL_CELL + FLYOUT_GAP + FLYOUT_W, 49.0);
        assert_eq!(main_width(GROUPS, false), 529.0 - (FLYOUT_GAP + FLYOUT_W));
    }

    /// The Draw pill is the kit's 625 pt: Move, the three illustrated tools between dividers,
    /// Frame, the shapes, Text, Comment, Actions and the mode switcher.
    #[test]
    fn the_draw_pill_is_on_the_ui3_grid() {
        assert_eq!(draw_width(), 625.0);
        assert_eq!(DRAW_TOOLS.len(), 3);
    }

    /// The switch to and from Draw mode keeps UI3's timing: CSS's `ease`, the rows over 0.3 s,
    /// the pill from 0.2 s over 0.5 s, the rows first both ways; turned around mid-way, it goes
    /// back from where it is.
    #[test]
    fn the_draw_switch_keeps_ui3s_timing() {
        let close = |a: f32, b: f32| (a - b).abs() < 2e-3;
        // cubic-bezier(0.25, 0.1, 0.25, 1)'s known values.
        assert!(close(css_ease(0.0), 0.0) && close(css_ease(1.0), 1.0));
        assert!(close(css_ease(0.5), 0.8024), "{}", css_ease(0.5));
        assert!(close(css_ease(0.25), 0.4094), "{}", css_ease(0.25));
        assert_eq!(css_ease(f32::NAN), 1.0);
        let on = DrawSwitch { on: true, at: 0.0, from: (0.0, 0.0) };
        assert_eq!(draw_progress(on, 0.0), (0.0, 0.0));
        let (slide, grow) = draw_progress(on, 0.15);
        assert!(close(slide, 0.8024) && grow == 0.0, "the rows first: {slide} {grow}");
        let (slide, grow) = draw_progress(on, 0.3);
        assert!(slide == 1.0 && grow > 0.0 && grow < 0.5, "the pill under way: {grow}");
        assert_eq!(draw_progress(on, 0.7), (1.0, 1.0));
        let off = DrawSwitch { on: false, at: 0.0, from: (1.0, 1.0) };
        let (slide, grow) = draw_progress(off, 0.15);
        assert!(slide < 0.25 && grow == 1.0, "back: the rows first again: {slide} {grow}");
        assert_eq!(draw_progress(off, 0.7), (0.0, 0.0));
        // Turned around half-way: from there, not from the end.
        let back = DrawSwitch { on: false, at: 0.0, from: draw_progress(on, 0.15) };
        assert_eq!(draw_progress(back, 0.0), draw_progress(on, 0.15));
        assert_eq!(draw_progress(back, f64::INFINITY), (0.0, 0.0));
    }

    /// The switcher's first segment opens Draw mode (its toolbar and header) and the others leave
    /// it; the brush and the pencil are the Pencil with a bristle brush or a plain stroke.
    #[test]
    fn draw_mode_and_its_tools() {
        let mut app = FigApp::new(figcraft_engine::Session::new(), Box::new(crate::NoServices));
        pick_mode(&mut app, 0);
        assert!(app.ui.draw_mode && draw_mode(&app));
        assert_eq!(bar_width(&app), draw_width());
        pick_draw_tool(&mut app, DrawTool::Brush);
        assert_eq!((app.session.tool.tool, drawn(&app)), (ToolId::Pencil, Some(DrawTool::Brush)));
        pick_draw_tool(&mut app, DrawTool::Pencil);
        assert_eq!((app.session.tool.tool, drawn(&app)), (ToolId::Pencil, Some(DrawTool::Pencil)));
        pick_draw_tool(&mut app, DrawTool::Pen);
        assert_eq!(drawn(&app), Some(DrawTool::Pen));
        for i in 1..4 {
            pick_mode(&mut app, 0);
            pick_mode(&mut app, i);
            assert!(!app.ui.draw_mode, "segment {i} leaves Draw mode");
        }
        // Every picture decodes.
        for (_, name, bytes, [w, h]) in DRAW_TOOLS {
            let img = image::load_from_memory(bytes).unwrap();
            assert_eq!((img.width() as f32, img.height() as f32), (w * 4.0, h * 4.0), "{name}");
        }
    }

    /// The Prototype mode opens the motion timeline under the canvas, and the other modes close it.
    #[test]
    fn prototype_mode_shows_the_motion_timeline() {
        let mut app = FigApp::new(figcraft_engine::Session::new(), Box::new(crate::NoServices));
        let shown = |app: &FigApp| app.session.ds().is_ok_and(|d| d.motion.panel);
        assert!(!shown(&app));
        pick_mode(&mut app, 2);
        assert!(shown(&app) && app.ui.right_tab == crate::RightTab::Prototype);
        pick_mode(&mut app, 2);
        assert!(shown(&app), "picking it again keeps it open");
        for i in [0, 1, 3] {
            pick_mode(&mut app, 2);
            pick_mode(&mut app, i);
            assert!(!shown(&app), "mode {i} puts the timeline away");
        }
    }

    /// The stroke widths `icon` paints with in a `size` box at `ppp` pixels per point.
    fn widths(ppp: f32, icon: Icon, size: f32) -> Vec<f32> {
        let ctx = egui::Context::default();
        let input = || {
            let mut i = egui::RawInput::default();
            i.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
            i
        };
        ctx.run_ui(input(), |_| {}).textures_delta.clear();
        let mut out =
            ctx.run_ui(input(), |ui| icons::paint(ui.painter(), Rect::from_center_size(pos2(50.0, 50.0), vec2(size, size)), icon, Color32::WHITE));
        out.textures_delta.clear();
        assert_eq!(ctx.pixels_per_point(), ppp);
        let mut w: Vec<f32> = out
            .shapes
            .iter()
            .filter_map(|c| match &c.shape {
                Shape::LineSegment { stroke, .. } => Some(stroke.width),
                Shape::Path(p) => Some(p.stroke.width),
                Shape::Rect(r) => Some(r.stroke.width),
                _ => None,
            })
            .filter(|w| *w > 0.0)
            .collect();
        w.dedup();
        w
    }

    /// The flyout carets and the marks beside them draw with one stroke width (#737), and the
    /// mode switcher's marks are the tool marks' size (#738).
    #[test]
    fn toolbar_marks_share_one_stroke_width() {
        for ppp in [1.0, 1.5, 2.0] {
            let mark = widths(ppp, Icon::Rectangle, GLYPH);
            assert_eq!(mark.len(), 1, "one width at {ppp}×: {mark:?}");
            assert_eq!(widths(ppp, Icon::Chevron, CARET), mark, "caret at {ppp}×");
            assert_eq!(widths(ppp, Icon::Code, GLYPH), mark, "mode mark at {ppp}×");
            for icon in [Icon::Frame, Icon::Text] {
                assert_eq!(widths(ppp, icon, GLYPH), mark, "{icon:?} at {ppp}×");
            }
        }
    }
}
