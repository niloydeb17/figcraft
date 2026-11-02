//! The Design panel's layout section: auto layout direction and wrap, W/H with their sizing (Fixed /
//! Hug / Fill) and min/max limits, alignment (including text baselines), fixed or auto gaps,
//! padding, strokes in layout and canvas stacking, grid tracks, a grid child's cell, spans and
//! alignment in its cell, and clip content. Everything goes through `node.set` props or `layout.*`
//! commands.

use std::sync::Arc;

use egui::{Align2, CornerRadius, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use figcraft_doc::{AutoLayout, CellAlign, ContentAlign, CrossAlign, Document, LayoutMode, MainAlign, Node, NodeId, NodeKind, Sizing};
use serde_json::json;

use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::{tr, tr_noop};
use crate::icons::{self, Icon};
use crate::right::{block, common, menu_check, menu_item, menu_popup, row};
use crate::theme::{Tokens, regular, semibold};
use crate::toolbar::DESIGN_BLUE;
use crate::widgets::{self, FIELD_H, Lead};

/// Most tracks listed in the panel (the rest are still laid out; edit them through commands).
const MAX_TRACK_ROWS: usize = 24;

fn parent_of(doc: &Document, id: NodeId) -> Option<&Node> {
    doc.node(id).and_then(|n| n.parent).and_then(|p| doc.node(p))
}

/// Show selection sizing and layout controls; offer Add only when every selected container has
/// no auto layout and the selection is editable outside an instance.
pub fn layout_section(app: &mut FigApp, ui: &mut Ui, doc: &Arc<Document>, sel: &[NodeId]) {
    let ids = sel.to_vec();
    let frames = crate::selection_cache::every(doc, sel, |n| n.kind.is_frame_like());
    let layout_containers = crate::selection_cache::every(doc, sel, |n| n.kind.is_frame_like() || matches!(n.kind, NodeKind::Group));
    let no_al = !sel.is_empty() && crate::selection_cache::every(doc, sel, |n| n.layout.is_none());
    let al = common(doc, sel, |n| n.layout.clone());
    let has_al = matches!(al, Some(Some(_)));
    let title = if has_al { tr("Auto layout") } else { tr("Layout") };
    // An instance's auto layout is its main's (only padding, gap and alignment are its own); the
    // layers inside it override their size and resizing, not their place in the layout.
    let facts = crate::right::in_instance(ui.ctx(), doc, sel);
    let (in_instance, nested) = (facts.any, facts.nested);
    let btns: Vec<(Icon, &str)> = if layout_containers && no_al && !in_instance {
        vec![(Icon::Plus, crate::shortcuts::tip(tr("Add auto layout"), "layout.add"))]
    } else {
        vec![]
    };
    if has_al {
        if auto_layout_heading(ui) && !in_instance {
            app.run("layout.remove", json!({"ids": ids}));
        }
    } else if widgets::section_header(ui, title, &btns, false).is_some() {
        app.run("layout.add", json!({"ids": ids}));
    }
    let from_main = tr("In an instance, this comes from the main component");
    if let Some(Some(l)) = &al {
        ui.add_enabled_ui(!in_instance, |ui| direction(app, ui, &ids, l)).response.on_disabled_hover_text(from_main);
    }
    dimensions(app, ui, doc, sel, has_al, nested, from_main);
    if let Some(Some(l)) = &al {
        alignment(app, ui, &ids, l);
        spacing(app, ui, &ids, l);
        ui.add_enabled_ui(!in_instance, |ui| {
            if l.mode == LayoutMode::Grid && sel.len() == 1 {
                tracks(app, ui, doc, &ids);
            }
        })
        .response
        .on_disabled_hover_text(from_main);
    }
    crate::selection_spacing::panel(app, ui, sel);
    let grid_parent = sel.iter().all(|i| parent_of(doc, *i).and_then(|p| p.layout.as_ref()).is_some_and(|l| l.mode == LayoutMode::Grid));
    if grid_parent && common(doc, sel, |n| n.child_layout.absolute) == Some(false) {
        ui.add_enabled_ui(!nested, |ui| grid_cell(app, ui, doc, sel)).response.on_disabled_hover_text(from_main);
    }
    if frames {
        let clip = common(doc, sel, |n| n.clip);
        row(ui, 32.0, |ui| {
            if widgets::checkbox(ui, clip.unwrap_or(false), tr("Clip content")) {
                app.set_props(&ids, json!({"clip": !clip.unwrap_or(false)}));
            }
        });
    }
}

/// The "Auto layout" heading: the title and, at the margin, the kit's button for the section being on
/// (a tinted square); clicking it removes auto layout. True when clicked.
fn auto_layout_heading(ui: &mut Ui) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().text(pos2(r.min.x + widgets::PAD, r.center().y), Align2::LEFT_CENTER, tr("Auto layout"), semibold(11.0), t.text);
    let b = Rect::from_min_size(pos2(r.max.x - widgets::PAD_RIGHT - 24.0, r.center().y - 12.0), vec2(24.0, 24.0));
    let resp = ui.interact(b, ui.scope_id().with("al_toggle"), Sense::click()).tip(crate::shortcuts::tip(tr("Remove auto layout"), "layout.remove"));
    let tint = DESIGN_BLUE.gamma_multiply(if resp.hovered() { 0.32 } else { 0.22 });
    ui.painter().rect_filled(b, CornerRadius::same(5), tint);
    icons::paint(ui.painter(), Rect::from_center_size(b.center(), vec2(16.0, 16.0)), Icon::AutoLayout, DESIGN_BLUE);
    resp.clicked()
}

/// A 24 pt icon button that, switched on, takes the kit's blue tint (the glyph in blue on it).
fn kit_toggle(ui: &mut Ui, icon: Icon, on: bool, tip: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
    let bg = if on {
        DESIGN_BLUE.gamma_multiply(if resp.hovered() { 0.32 } else { 0.22 })
    } else if resp.hovered() {
        t.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(r, CornerRadius::same(5), bg);
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, if on { DESIGN_BLUE } else { t.icon });
    let resp = resp.tip(tip);
    if on {
        crate::a11y::toggle(&resp, tip, true);
    }
    resp
}

/// The Flow row: free-form (no auto layout), vertical, horizontal and grid in one switch, and the
/// wrap button beside it.
fn direction(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], l: &AutoLayout) {
    widgets::sublabels(ui, tr("Flow"), None);
    row(ui, 32.0, |ui| {
        let items =
            [(Icon::LayoutFree, tr("Free-form")), (Icon::FlowV, tr("Vertical")), (Icon::FlowH, tr("Horizontal")), (Icon::FlowGrid, tr("Grid"))];
        let active = match l.mode {
            LayoutMode::Vertical => 1,
            LayoutMode::Horizontal => 2,
            LayoutMode::Grid => 3,
        };
        if let Some(i) = widgets::segmented_kit(ui, &items, Some(active), 46.0) {
            if i == 0 {
                app.run("layout.remove", json!({"ids": ids}));
            } else {
                // Wrap carries over between the flows: rows of a horizontal one, columns of a vertical one.
                let mode = ["", "vertical", "horizontal", "grid"].get(i).copied().unwrap_or("grid");
                app.set_props(ids, json!({"layout": mode}));
            }
        }
        if l.mode != LayoutMode::Grid {
            let resp = kit_toggle(ui, Icon::FlowWrap, l.wrap, tr("Wrap"));
            crate::a11y::toggle(&resp, tr("Wrap"), l.wrap);
            if resp.clicked() {
                app.set_props(ids, json!({"wrap": !l.wrap}));
            }
        }
    });
}

/// One axis' sizing, min/max keys and labels (the add-min and add-max menu items, the remove
/// button, the min and max field leads).
struct Axis {
    sizing: &'static str,
    sizing_name: &'static str,
    min: &'static str,
    max: &'static str,
    add_min: &'static str,
    add_max: &'static str,
    remove: &'static str,
    min_lead: &'static str,
    max_lead: &'static str,
}

const AXES: [Axis; 2] = [
    Axis {
        sizing: "sizingH",
        sizing_name: tr_noop("Horizontal resizing"),
        min: "minWidth",
        max: "maxWidth",
        add_min: tr_noop("Add min width"),
        add_max: tr_noop("Add max width"),
        remove: tr_noop("Remove min and max width"),
        min_lead: tr_noop("Min W"),
        max_lead: tr_noop("Max W"),
    },
    Axis {
        sizing: "sizingV",
        sizing_name: tr_noop("Vertical resizing"),
        min: "minHeight",
        max: "maxHeight",
        add_min: tr_noop("Add min height"),
        add_max: tr_noop("Add max height"),
        remove: tr_noop("Remove min and max height"),
        min_lead: tr_noop("Min H"),
        max_lead: tr_noop("Max H"),
    },
];

/// A node's min (`is_min`) or max limit on one axis.
fn limit(n: &Node, horizontal: bool, is_min: bool) -> Option<f64> {
    let cl = &n.child_layout;
    match (horizontal, is_min) {
        (true, true) => cl.min_w,
        (true, false) => cl.max_w,
        (false, true) => cl.min_h,
        (false, false) => cl.max_h,
    }
}

/// `nested`: layers inside an instance, whose proportion lock is their main's (`from_main` says so).
fn dimensions(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], has_al: bool, nested: bool, from_main: &str) {
    let ids = sel.to_vec();
    let w_of = |n: &Node| (n.width * 100.0).round() / 100.0;
    let h_of = |n: &Node| (n.height * 100.0).round() / 100.0;
    let (w, h) = (common(doc, sel, w_of), common(doc, sel, h_of));
    let col = widgets::col_width(ui.available_width());
    let parent_al = sel.iter().all(|i| parent_of(doc, *i).is_some_and(|p| p.layout.is_some()));
    let is_text = sel.iter().all(|i| matches!(doc.node(*i).map(|n| &n.kind), Some(NodeKind::Text(_))));
    // Sizing (Fixed / Hug / Fill) and the min/max limits apply to auto layout frames, their
    // children and text; absolute children neither fill nor limit.
    let can_size = has_al || parent_al || is_text;
    let can_fill = parent_al && crate::selection_cache::every(doc, sel, |n| !n.child_layout.absolute);
    let can_limit = has_al || can_fill;
    widgets::sublabels(ui, tr("Dimensions"), None);
    row(ui, 32.0, |ui| {
        for (ax, axis) in AXES.iter().enumerate() {
            let horizontal = ax == 0;
            let (lead, value, id) = if horizontal { ("W", w, "lay_w") } else { ("H", h, "lay_h") };
            let (field_key, shown): (&str, fn(&Node) -> f64) =
                if horizontal { ("width", |n| (n.width * 100.0).round() / 100.0) } else { ("height", |n| (n.height * 100.0).round() / 100.0) };
            // Where the field lands: the sizing menu's chevron sits over its right end.
            let field =
                Rect::from_min_size(pos2(ui.available_rect_before_wrap().min.x, ui.max_rect().center().y - FIELD_H / 2.0), vec2(col, FIELD_H));
            let e = crate::variable_ui::number_field_mixed(app, ui, &ids, field_key, Id::unique(id), Lead::Text(lead), value, "", col);
            app.field_of(&ids, field_key, e, shown, |v| json!(v.max(0.0)));
            if can_size {
                sizing_menu(app, ui, doc, sel, field, axis, horizontal, (has_al || is_text, can_fill, can_limit));
            }
        }
        let locked = common(doc, sel, |n| n.constrain_proportions).unwrap_or(false);
        let link = ui
            .add_enabled_ui(!nested, |ui| kit_toggle(ui, Icon::LockAspect, locked, tr("Constrain proportions")))
            .inner
            .on_disabled_hover_text(from_main);
        if link.clicked() {
            app.set_props(&ids, json!({"constrainProportions": !locked}));
        }
    });
    if can_limit {
        for (ax, axis) in AXES.iter().enumerate() {
            limit_row(app, ui, doc, sel, ax == 0, axis, col);
        }
    }
}

/// One axis' sizing: its name (Hug, Fill) before the chevron at the field's end, and the menu the
/// chevron opens (Fixed, Hug contents, Fill container, min and max limits). `can`: hug, fill, limit.
fn sizing_menu(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], field: Rect, axis: &Axis, horizontal: bool, can: (bool, bool, bool)) {
    let t = Tokens::get(ui.ctx());
    let (can_hug, can_fill, can_limit) = can;
    let cur = common(doc, sel, |n| if horizontal { n.child_layout.sizing_h } else { n.child_layout.sizing_v });
    let any_limit = sel.iter().filter_map(|i| doc.node(*i)).any(|n| limit(n, horizontal, true).is_some() || limit(n, horizontal, false).is_some());
    // "Fill" here is the sizing (fill the container), not the paint.
    let name = match cur {
        Some(Sizing::Hug) => tr("Hug"),
        Some(Sizing::Fill) => tr("Fill|sizing"),
        Some(Sizing::Fixed) => "",
        None => tr("Mixed"),
    };
    let chevron = Rect::from_min_size(pos2(field.max.x - 24.0, field.min.y), vec2(24.0, FIELD_H));
    let resp = ui.interact(chevron, ui.scope_id().with(("sizing", horizontal)), Sense::click());
    crate::a11y::combo(&resp, tr(axis.sizing_name), name, crate::a11y::menu_open(&resp));
    if resp.hovered() {
        ui.painter().rect_filled(chevron.shrink(1.0), CornerRadius::same(4), t.hover);
    }
    if !name.is_empty() {
        ui.painter().text(pos2(chevron.min.x, field.center().y), Align2::RIGHT_CENTER, name, regular(11.0), t.text_secondary);
    }
    icons::paint(ui.painter(), Rect::from_center_size(chevron.center(), vec2(16.0, 16.0)), Icon::Chevron, t.icon);
    let first = sel.first().and_then(|i| doc.node(*i));
    let current = first.map(|n| if horizontal { n.width } else { n.height }).unwrap_or(0.0);
    let ids = sel.to_vec();
    menu_popup(&resp).width(200.0).show(|ui| {
        let mut opts = vec![(tr("Fixed"), "fixed")];
        if can_hug {
            opts.push((tr("Hug contents"), "hug"));
        }
        if can_fill {
            opts.push((tr("Fill container"), "fill"));
        }
        for (l, v) in opts {
            if menu_item(ui, l, "") {
                app.set_props(&ids, json!({axis.sizing: v}));
                ui.close();
            }
        }
        if can_limit {
            ui.separator();
            let (min, max) = (first.and_then(|n| limit(n, horizontal, true)), first.and_then(|n| limit(n, horizontal, false)));
            if min.is_none() && menu_item(ui, tr(axis.add_min), "") {
                app.set_props(&ids, json!({axis.min: current}));
                ui.close();
            }
            if max.is_none() && menu_item(ui, tr(axis.add_max), "") {
                app.set_props(&ids, json!({axis.max: current}));
                ui.close();
            }
            if any_limit && menu_item(ui, tr("Remove min and max"), "") {
                app.set_props(&ids, json!({axis.min: null, axis.max: null}));
                ui.close();
            }
        }
    });
}

/// The min/max fields of one axis (shown once either is set): min in the first column, max in the
/// second, an empty slot offers to add the missing one; the button removes both.
fn limit_row(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId], horizontal: bool, axis: &Axis, col: f32) {
    let nodes: Vec<&Node> = sel.iter().filter_map(|i| doc.node(*i)).collect();
    let has = |is_min: bool| nodes.iter().any(|n| limit(n, horizontal, is_min).is_some());
    if !has(true) && !has(false) {
        return;
    }
    let ids = sel.to_vec();
    let current = nodes.first().map(|n| if horizontal { n.width } else { n.height }).unwrap_or(0.0);
    row(ui, 32.0, |ui| {
        for (is_min, key) in [(true, axis.min), (false, axis.max)] {
            let (lead, add) = if is_min { (tr(axis.min_lead), tr(axis.add_min)) } else { (tr(axis.max_lead), tr(axis.add_max)) };
            if has(is_min) {
                let v = common(doc, sel, |n| limit(n, horizontal, is_min).map(|v| (v * 100.0).round() / 100.0)).flatten();
                let e = crate::variable_ui::number_field(app, ui, &ids, key, Id::unique(("lay_limit", key)), Lead::Text(lead), v, "", col);
                app.field(&ids, key, e, |v| json!(v.max(0.0)));
            } else if add_slot(ui, lead, add, col).clicked() {
                app.set_props(&ids, json!({key: current}));
            }
        }
        if icons::button(ui, Icon::Minus, 24.0, false, tr(axis.remove)).clicked() {
            app.set_props(&ids, json!({axis.min: null, axis.max: null}));
        }
    });
}

/// A quiet placeholder button the size of a field ("+ Max W") with the tooltip `tip`.
fn add_slot(ui: &mut Ui, label: &str, tip: &str, width: f32) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(width, FIELD_H), Sense::click());
    let resp = resp.tip(tip);
    let border = if resp.hovered() { t.field_hover_border } else { t.border };
    ui.painter().rect_stroke(r, CornerRadius::same(5), Stroke::new(1.0, border), StrokeKind::Inside);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 12.0, r.center().y), vec2(10.0, 10.0)), Icon::Plus, t.icon_secondary);
    ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.text_secondary);
    resp
}

/// One selected cell of the alignment grid: the kit's three bars (their heights 7, 10 and 5 of a
/// 16 pt box), upright in a horizontal flow and lying in a vertical one.
fn alignment_mark(ui: &Ui, c: egui::Pos2, upright: bool) {
    for (x, len, off) in [(3.0, 7.0, 4.5), (7.0, 10.0, 3.0), (11.0, 5.0, 5.5)] {
        let r = if upright {
            Rect::from_min_size(pos2(c.x - 8.0 + x, c.y - 8.0 + off), vec2(2.0, len))
        } else {
            Rect::from_min_size(pos2(c.x - 8.0 + off, c.y - 8.0 + x), vec2(len, 2.0))
        };
        ui.painter().rect_filled(r, CornerRadius::same(1), DESIGN_BLUE);
    }
}

/// The Alignment and Gap row: the 3×3 main/cross alignment grid, the gap beside it (with its auto
/// modes behind the chevron; the column and row gaps of a grid), and the settings button.
fn alignment(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], l: &AutoLayout) {
    let t = Tokens::get(ui.ctx());
    let grid_mode = l.mode == LayoutMode::Grid;
    widgets::sublabels(ui, tr("Alignment"), Some(tr("Gap")));
    block(ui, 64.0, 0.0, |ui| {
        let inner = ui.max_rect();
        let col = widgets::col_width(inner.width() + widgets::PAD + widgets::PAD_RIGHT);
        let grid = Rect::from_min_size(pos2(inner.min.x, inner.min.y + 4.0), vec2(col, 56.0));
        ui.allocate_rect(grid, Sense::hover());
        ui.painter().rect_filled(grid, CornerRadius::same(5), t.field);
        let horizontal = l.mode != LayoutMode::Vertical;
        let cur_main = match l.main_align {
            MainAlign::Min => 0,
            MainAlign::Center => 1,
            MainAlign::Max => 2,
            MainAlign::SpaceBetween | MainAlign::SpaceAround | MainAlign::SpaceEvenly => 9,
        };
        // Wrapped lines spread across the flow light the whole cross axis, like an auto gap does
        // the main one.
        let spread = !grid_mode && l.wrap && l.content_align == ContentAlign::SpaceBetween;
        let cur_cross = match l.cross_align {
            _ if spread => 9,
            CrossAlign::Min | CrossAlign::Baseline => 0,
            CrossAlign::Center => 1,
            CrossAlign::Max => 2,
        };
        let cells = Rect::from_min_size(grid.min + vec2(1.0, 5.0), vec2(col - 2.0, 46.0));
        let (cw, ch) = (cells.width() / 3.0, cells.height() / 3.0);
        for yi in 0..3 {
            for xi in 0..3 {
                let r = Rect::from_min_size(cells.min + vec2(xi as f32 * cw, yi as f32 * ch), vec2(cw, ch));
                let resp = ui.interact(r, Id::unique(("al_grid", xi, yi)), Sense::click());
                let (main_i, cross_i) = if horizontal { (xi, yi) } else { (yi, xi) };
                let on = (cur_main == main_i || cur_main == 9) && (cur_cross == cross_i || cur_cross == 9);
                if on {
                    alignment_mark(ui, r.center(), horizontal);
                } else {
                    ui.painter().circle_filled(
                        r.center(),
                        if resp.hovered() { 2.0 } else { 1.0 },
                        if resp.hovered() { DESIGN_BLUE } else { t.text_tertiary },
                    );
                }
                if resp.clicked() {
                    let m = ["min", "center", "max"][main_i];
                    let cr = ["min", "center", "max"][cross_i];
                    // An auto gap keeps its mode: the cells pick the cross alignment.
                    let main = if l.main_align.is_auto_gap() && !grid_mode { json!(l.main_align) } else { json!(m) };
                    app.set_props(ids, json!({"mainAlign": main, "crossAlign": cr}));
                }
            }
        }
        // The gap: beside the grid, level with its top.
        let right = Rect::from_min_size(pos2(grid.max.x + widgets::GUTTER, grid.min.y), vec2(col, 56.0));
        let mut gap_ui = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(egui::Layout::top_down(egui::Align::Min)));
        gap_ui.spacing_mut().item_spacing = vec2(0.0, 8.0);
        let ui2 = &mut gap_ui;
        if grid_mode {
            // Grid: gap between columns and between rows.
            let e = crate::variable_ui::number_field(
                app,
                ui2,
                ids,
                "columnGap",
                Id::unique("al_gap"),
                Lead::Icon(Icon::Gap, tr("Column gap")),
                Some(l.gap),
                "",
                col,
            );
            app.field(ids, "columnGap", e, |v| json!(v.max(0.0)));
            let e = crate::variable_ui::number_field(
                app,
                ui2,
                ids,
                "rowGap",
                Id::unique("al_rowgap"),
                Lead::Icon(Icon::GapV, tr("Row gap")),
                Some(l.cross_gap),
                "",
                col,
            );
            app.field(ids, "rowGap", e, |v| json!(v.max(0.0)));
        } else {
            let auto = l.main_align.is_auto_gap();
            let gap_icon = if l.mode == LayoutMode::Vertical { Icon::GapV } else { Icon::Gap };
            let (shown, lead) = ((!auto).then_some(l.gap), Lead::Icon(gap_icon, tr("Gap between items")));
            let field = Rect::from_min_size(ui2.max_rect().min, vec2(col, FIELD_H));
            let e = crate::variable_ui::number_field_or(app, ui2, ids, "gap", Id::unique("al_gap"), shown, col, tr("Auto"), lead, "");
            app.field(ids, "gap", e, |v| json!(v));
            // The auto modes behind the chevron at the field's end, set off by a hairline.
            let chevron = Rect::from_min_size(pos2(field.max.x - 25.0, field.min.y), vec2(25.0, FIELD_H));
            ui2.painter()
                .line_segment([chevron.left_top() + vec2(0.0, 1.0), chevron.left_bottom() - vec2(0.0, 1.0)], egui::Stroke::new(1.0, t.border));
            let resp = ui2.interact(chevron, ui2.scope_id().with("gap_mode"), Sense::click());
            let label = match l.main_align {
                MainAlign::SpaceBetween => tr("Space between"),
                MainAlign::SpaceAround => tr("Space around"),
                MainAlign::SpaceEvenly => tr("Space evenly"),
                _ => tr("Fixed gap"),
            };
            crate::a11y::combo(&resp, tr("Auto gap"), label, crate::a11y::menu_open(&resp));
            if resp.hovered() {
                ui2.painter().rect_filled(chevron.shrink2(vec2(1.0, 1.0)), CornerRadius::same(4), t.hover);
            }
            icons::paint(ui2.painter(), Rect::from_center_size(chevron.center(), vec2(16.0, 16.0)), Icon::Chevron, t.icon);
            menu_popup(&resp).show(|ui| {
                let modes = [
                    (tr("Fixed"), "min"),
                    (tr("Auto (space between)"), "spaceBetween"),
                    (tr("Auto (space evenly)"), "spaceEvenly"),
                    (tr("Auto (space around)"), "spaceAround"),
                ];
                for (l, v) in modes {
                    if menu_item(ui, l, "") {
                        app.set_props(ids, json!({"mainAlign": v}));
                        ui.close();
                    }
                }
            });
        }
        // Auto layout settings: strokes, canvas stacking and text baselines.
        let button = Rect::from_min_size(pos2(inner.max.x - 24.0, inner.min.y + 4.0), vec2(24.0, 24.0));
        let resp = ui.interact(button, ui.scope_id().with("al_settings"), Sense::click()).tip(tr("Auto layout settings"));
        if resp.hovered() {
            ui.painter().rect_filled(button, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(button.center(), vec2(16.0, 16.0)), Icon::LayoutSettings, t.icon);
        menu_popup(&resp).width(200.0).show(|ui| settings_menu(app, ui, ids, l));
    });
}

/// The Auto layout settings menu: whether strokes take up space, which child paints on top where
/// children overlap (canvas stacking; the layer order stays), and, in a horizontal stack, text
/// baselines.
fn settings_menu(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], l: &AutoLayout) {
    let t = Tokens::get(ui.ctx());
    let heading = |ui: &mut Ui, text: &str| {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        ui.painter().text(pos2(r.min.x + 12.0, r.center().y), Align2::LEFT_CENTER, text, regular(10.0), t.menu_dim);
    };
    heading(ui, tr("Strokes"));
    for (v, label) in [(true, tr("Included in layout")), (false, tr("Excluded from layout"))] {
        if menu_check(ui, l.strokes_included == v, label, "") {
            app.set_props(ids, json!({"strokesIncluded": v}));
            ui.close();
        }
    }
    heading(ui, tr("Canvas stacking"));
    for (v, label) in [(false, tr("Last on top")), (true, tr("First on top"))] {
        if menu_check(ui, l.reverse_z == v, label, "") {
            app.set_props(ids, json!({"reverseZ": v}));
            ui.close();
        }
    }
    // Text baselines: horizontal stacks only.
    if l.mode == LayoutMode::Horizontal {
        ui.separator();
        let on = l.cross_align == CrossAlign::Baseline;
        if menu_check(ui, on, tr("Align text baselines"), "") {
            app.set_props(ids, json!({"crossAlign": if on { "min" } else { "baseline" }}));
            ui.close();
        }
    }
}

/// The Spacing row: the H/V padding pair, or, with Individual padding on, two rows of one field
/// per side (like Individual corners).
fn spacing(app: &mut FigApp, ui: &mut Ui, ids: &[NodeId], l: &AutoLayout) {
    let col = widgets::col_width(ui.available_width());
    let [pt, pr, pb, pl] = l.padding;
    let open = ui.data(|d| d.get_temp::<bool>(Id::unique("indep_padding"))).unwrap_or(false);
    let toggle = |ui: &mut Ui| {
        if icons::button(ui, Icon::IndependentPadding, 24.0, open, tr("Individual padding")).clicked() {
            ui.data_mut(|d| d.insert_temp(Id::unique("indep_padding"), !open));
        }
    };
    widgets::sublabels(ui, tr("Padding"), None);
    if open {
        // `l` is the selection's common layout, so its sides are common too. `k` indexes
        // [top, right, bottom, left].
        let sides = [
            ("paddingLeft", tr("Left padding"), Icon::PaddingLeft, pl, 3),
            ("paddingTop", tr("Top padding"), Icon::PaddingTop, pt, 0),
            ("paddingRight", tr("Right padding"), Icon::PaddingRight, pr, 1),
            ("paddingBottom", tr("Bottom padding"), Icon::PaddingBottom, pb, 2),
        ];
        for (r, pair) in sides.chunks(2).enumerate() {
            row(ui, 32.0, |ui| {
                for (key, name, icon, v, k) in pair {
                    let e =
                        crate::variable_ui::number_field(app, ui, ids, key, Id::unique(("padding", *k)), Lead::Icon(*icon, name), Some(*v), "", col);
                    app.field(ids, key, e, |v| json!(v.max(0.0)));
                }
                if r == 0 {
                    toggle(ui);
                }
            });
        }
    } else {
        row(ui, 32.0, |ui| {
            let e = crate::variable_ui::number_field(
                app,
                ui,
                ids,
                "paddingHorizontal",
                Id::unique("al_padh"),
                Lead::Icon(Icon::PaddingH, tr("Horizontal padding")),
                (pl == pr).then_some(pl),
                "",
                col,
            );
            app.field(ids, "paddingHorizontal", e, |v| json!(v.max(0.0)));
            let e = crate::variable_ui::number_field(
                app,
                ui,
                ids,
                "paddingVertical",
                Id::unique("al_padv"),
                Lead::Icon(Icon::PaddingV, tr("Vertical padding")),
                (pt == pb).then_some(pt),
                "",
                col,
            );
            app.field(ids, "paddingVertical", e, |v| json!(v.max(0.0)));
            toggle(ui);
        });
    }
    if l.mode != LayoutMode::Grid && l.wrap {
        // Wrapped rows (horizontal) or columns (vertical): the gap between them.
        let vertical = l.mode == LayoutMode::Vertical;
        let name = if vertical { tr("Column gap") } else { tr("Row gap") };
        widgets::sublabels(ui, name, None);
        row(ui, 32.0, |ui| {
            let (icon, tip) = if vertical { (Icon::Gap, tr("Gap between columns")) } else { (Icon::DistributeV, tr("Gap between rows")) };
            let spread = l.content_align == ContentAlign::SpaceBetween;
            let shown = (!spread).then_some(l.cross_gap);
            let e = crate::variable_ui::number_field_or(
                app,
                ui,
                ids,
                "crossGap",
                Id::unique("al_crossgap"),
                shown,
                col,
                tr("Auto"),
                Lead::Icon(icon, tip),
                "",
            );
            // A typed gap is a fixed gap.
            app.field_run(e, "node.set", |v| json!({"ids": ids, "props": {"crossGap": v.max(0.0), "contentAlign": "auto"}}));
            let label = if spread { tr("Space between") } else { tr("Fixed gap") };
            let resp = widgets::dropdown(ui, label, col, None);
            crate::a11y::combo(&resp, name, label, crate::a11y::menu_open(&resp));
            menu_popup(&resp).show(|ui| {
                for (l, v) in [(tr("Fixed"), "auto"), (tr("Auto (space between)"), "spaceBetween")] {
                    if menu_item(ui, l, "") {
                        app.set_props(ids, json!({"contentAlign": v}));
                        ui.close();
                    }
                }
            });
        });
    }
    if l.mode == LayoutMode::Grid {
        widgets::sublabels(ui, tr("Columns"), Some(tr("Rows")));
        row(ui, 32.0, |ui| {
            let e = widgets::number_field(ui, Id::unique("al_cols"), Lead::None, Some(f64::from(l.grid_columns)), "", col, 1.0);
            app.field(ids, "gridColumns", e, |v| json!(v.max(1.0)));
            let e = widgets::number_field(ui, Id::unique("al_rows"), Lead::None, Some(f64::from(l.grid_rows)), "", col, 1.0);
            app.field(ids, "gridRows", e, |v| json!(v.max(1.0)));
        });
    }
}

/// Track sizes of a single grid frame: one row per column / row with its size and sizing.
fn tracks(app: &mut FigApp, ui: &mut Ui, doc: &Document, ids: &[NodeId]) {
    let Some(id) = ids.first().copied() else { return };
    let Some(l) = doc.node(id).and_then(|n| n.layout.clone()) else { return };
    let Some(g) = figcraft_layout::grid_geometry(doc, id) else { return };
    let col = widgets::col_width(ui.available_width());
    for (axis, specs, ranges) in [("columns", &l.column_tracks, &g.columns), ("rows", &l.row_tracks, &g.rows)] {
        widgets::sublabels(ui, if axis == "columns" { tr("Column sizes") } else { tr("Row sizes") }, None);
        for (i, (a, b)) in ranges.iter().enumerate().take(MAX_TRACK_ROWS) {
            let track = specs.get(i).copied().unwrap_or_default();
            let size = ((b - a) * 100.0).round() / 100.0;
            let base = json!({"ids": [id], "axis": axis, "index": i});
            let with = |sizing: &str, value: Option<f64>| {
                let mut p = base.clone();
                p["sizing"] = json!(sizing);
                if let Some(v) = value {
                    p["value"] = json!(v);
                }
                p
            };
            row(ui, 32.0, |ui| {
                let label = (i + 1).to_string();
                // Fill tracks edit their fr; fixed and hug tracks show (and set) a px size.
                let (value, suffix) = if track.sizing == Sizing::Fill { (track.value, "fr") } else { (size, "") };
                let e = widgets::number_field(
                    ui,
                    Id::unique(("al_track", axis, i)),
                    Lead::Text(&label),
                    Some(value),
                    suffix,
                    col,
                    if suffix.is_empty() { 1.0 } else { 0.1 },
                );
                let fill = track.sizing == Sizing::Fill;
                app.field_run(e, "layout.setTrack", |v| with(if fill { "fill" } else { "fixed" }, Some(v.max(0.0))));
                let (label, icon) = match track.sizing {
                    Sizing::Fixed => (tr("Fixed"), Icon::Fixed),
                    Sizing::Hug => (tr("Hug"), Icon::Hug),
                    Sizing::Fill => (tr("Fill|sizing"), Icon::FillContainer),
                };
                let resp = widgets::dropdown(ui, label, col, Some(icon));
                menu_popup(&resp).show(|ui| {
                    for (l, v) in [(tr("Fixed"), "fixed"), (tr("Hug contents"), "hug"), (tr("Fill (fr)"), "fill")] {
                        if menu_item(ui, l, "") {
                            app.run("layout.setTrack", with(v, None));
                            ui.close();
                        }
                    }
                });
            });
        }
    }
}

/// A grid child's cell (single selection: column and row, 1-based; auto placement resets them)
/// and its spans.
fn grid_cell(app: &mut FigApp, ui: &mut Ui, doc: &Document, sel: &[NodeId]) {
    let ids = sel.to_vec();
    let col = widgets::col_width(ui.available_width());
    if let [id] = sel
        && let Some(cell) =
            parent_of(doc, *id).and_then(|p| figcraft_layout::grid_geometry(doc, p.id)).and_then(|g| g.cells.into_iter().find(|c| c.id == *id))
    {
        let anchored = doc.node(*id).is_some_and(|n| n.child_layout.grid_column.is_some() || n.child_layout.grid_row.is_some());
        widgets::sublabels(ui, tr("Column"), Some(tr("Row")));
        row(ui, 32.0, |ui| {
            // Moving on one axis pins the other where it is, so the child doesn't jump.
            let at = |c: f64, r: f64| json!({"ids": [id], "props": {"gridColumn": (c - 1.0).max(0.0), "gridRow": (r - 1.0).max(0.0)}});
            let (c0, r0) = ((cell.column + 1) as f64, (cell.row + 1) as f64);
            let e = widgets::number_field(ui, Id::unique("al_cell_col"), Lead::None, Some(c0), "", col, 1.0);
            app.field_run(e, "node.set", |v| at(v, r0));
            let e = widgets::number_field(ui, Id::unique("al_cell_row"), Lead::None, Some(r0), "", col, 1.0);
            app.field_run(e, "node.set", |v| at(c0, v));
            if anchored && icons::button(ui, Icon::Reset, 24.0, false, tr("Place automatically")).clicked() {
                app.set_props(&ids, json!({"gridColumn": null, "gridRow": null}));
            }
        });
    }
    let cs = common(doc, sel, |n| f64::from(n.child_layout.column_span()));
    let rs = common(doc, sel, |n| f64::from(n.child_layout.row_span()));
    widgets::sublabels(ui, tr("Column span"), Some(tr("Row span")));
    row(ui, 32.0, |ui| {
        let e = widgets::number_field(ui, Id::unique("al_span_c"), Lead::None, cs, "", col, 1.0);
        app.field(&ids, "gridColumnSpan", e, |v| json!(v.max(1.0)));
        let e = widgets::number_field(ui, Id::unique("al_span_r"), Lead::None, rs, "", col, 1.0);
        app.field(&ids, "gridRowSpan", e, |v| json!(v.max(1.0)));
    });
    // Alignment in the cell; Auto follows the frame's alignment.
    widgets::sublabels(ui, tr("Horizontal align"), Some(tr("Vertical align")));
    row(ui, 32.0, |ui| {
        let axes = [
            ("gridAlignH", tr("Horizontal align"), common(doc, sel, |n| n.child_layout.grid_align_h), [tr("Left"), tr("Center"), tr("Right")]),
            ("gridAlignV", tr("Vertical align"), common(doc, sel, |n| n.child_layout.grid_align_v), [tr("Top"), tr("Center"), tr("Bottom")]),
        ];
        for (key, name, cur, [min, center, max]) in axes {
            let options = [(None, tr("Auto")), (Some(CellAlign::Min), min), (Some(CellAlign::Center), center), (Some(CellAlign::Max), max)];
            let label = cur.map_or(tr("Mixed"), |c| options.iter().find(|(v, _)| *v == c).map_or("", |(_, l)| *l));
            let resp = widgets::dropdown(ui, label, col, None);
            crate::a11y::combo(&resp, name, label, crate::a11y::menu_open(&resp));
            menu_popup(&resp).show(|ui| {
                for (v, l) in options {
                    if menu_item(ui, l, "") {
                        app.set_props(&ids, json!({key: v}));
                        ui.close();
                    }
                }
            });
        }
    });
}
