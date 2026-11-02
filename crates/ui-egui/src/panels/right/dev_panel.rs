//! The Inspect tab's pieces in the kit's Dev Mode layout: collapsible sections, the layer's header
//! (name, what it is, actions), the MCP block, the box model, and code blocks with line numbers.

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, StrokeKind};
use figcraft_doc::DevStatus;
use figcraft_engine::codegen::CodeLang;
use serde_json::json;

use super::menu::{menu_item, menu_popup};
use super::{
    Align2, Color32, CornerRadius, Document, FigApp, Icon, Id, NodeId, PAD, Rect, Sense, Stroke, Tip, Tokens, Ui, a11y, icons, pos2, regular,
    semibold, tr, trf, vec2, widgets,
};

/// Keywords and numbers in a code block's values, and `var(…)`.
const PINK: Color32 = Color32::from_rgb(0xFC, 0x9C, 0xE0);
const ORANGE: Color32 = Color32::from_rgb(0xE8, 0xA8, 0x5C);
/// A link in the prompt box.
const LINK: Color32 = Color32::from_rgb(0x5C, 0xA8, 0xE8);
/// The box model's padding area.
const PADDING_FILL: Color32 = Color32::from_rgb(0x39, 0x43, 0x60);
/// The padding box's outline, and the border box's.
const PADDING_RING: Color32 = Color32::from_rgb(0xB4, 0xB6, 0xBE);
const BORDER_RING: Color32 = Color32::from_rgb(0x3E, 0x3E, 0x3E);

/// Where the values of a label/value row start, from the panel's left edge.
const VALUE_X: f32 = 141.0;

/// A section of the panel: a header with a disclosure chevron, its `title` (and a dimmer `suffix`)
/// and icon buttons at the end. Whether it is open and which button was clicked.
pub(super) fn section(ui: &mut Ui, key: &str, title: &str, suffix: Option<&str>, trailing: &[(Icon, &str)]) -> (bool, Option<usize>) {
    let t = Tokens::get(ui.ctx());
    let id = Id::unique(("dev_section", key));
    let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    a11y::toggle(&resp, title, open);
    let chevron = if open { Icon::Chevron } else { Icon::ChevronRight };
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 9.0, r.center().y), vec2(12.0, 12.0)), chevron, t.icon_secondary);
    let g = ui.painter().layout_no_wrap(title.to_string(), semibold(11.0), t.text);
    let end = r.min.x + PAD + g.size().x;
    ui.painter().galley(pos2(r.min.x + PAD, r.center().y - g.size().y / 2.0), g, t.text);
    if let Some(s) = suffix {
        ui.painter().text(pos2(end + 6.0, r.center().y), Align2::LEFT_CENTER, s, regular(11.0), t.text_secondary);
    }
    let mut clicked = None;
    let mut x = r.max.x - 8.0;
    for (i, (icon, tip)) in trailing.iter().enumerate().rev() {
        let br = Rect::from_min_size(pos2(x - 24.0, r.center().y - 12.0), vec2(24.0, 24.0));
        let bresp = ui.interact(br, ui.scope_id().with(("dev_sec_btn", key, i)), Sense::click()).tip(*tip);
        if bresp.hovered() {
            ui.painter().rect_filled(br, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), br.shrink(6.0), *icon, t.icon);
        if bresp.clicked() {
            clicked = Some(i);
        }
        x -= 26.0;
    }
    if resp.clicked() && clicked.is_none() && !ui.rect_contains_pointer(Rect::from_min_max(pos2(x, r.min.y), pos2(r.max.x, r.max.y))) {
        ui.data_mut(|d| d.insert_temp(id, !open));
    }
    (open, clicked)
}

/// What a layer is, in a few words: "Frame with auto layout", "Text", "Instance"…
pub(super) fn describe(doc: &Document, id: NodeId) -> (Icon, String) {
    let Some(n) = doc.node(id) else { return (Icon::Frame, String::new()) };
    let label = n.kind.label();
    match label {
        "Frame" if n.layout.is_some() => (Icon::AutoLayout, tr("Frame with auto layout").to_string()),
        "Frame" => (Icon::Frame, tr("Frame").to_string()),
        "Page" => (Icon::Page, tr(label).to_string()),
        "Group" => (Icon::Group, tr(label).to_string()),
        "Section" => (Icon::Section, tr(label).to_string()),
        "Rectangle" => (Icon::Rectangle, tr(label).to_string()),
        "Ellipse" => (Icon::Ellipse, tr(label).to_string()),
        "Polygon" => (Icon::Polygon, tr(label).to_string()),
        "Star" => (Icon::Star, tr(label).to_string()),
        "Line" => (Icon::Line, tr(label).to_string()),
        "Text" => (Icon::Text, tr(label).to_string()),
        "Component" | "Component set" => (Icon::Component, label.to_string()),
        "Instance" => (Icon::Instance, tr(label).to_string()),
        _ => (Icon::Vector, label.to_string()),
    }
}

/// The selected layer's header: its name with a link button, for a frame or section a button that
/// toggles its ready-for-dev status, and a menu; below it what it is, or its status once it has one.
pub(super) fn layer_header(app: &mut FigApp, ui: &mut Ui, doc: &Document, id: NodeId, code: &str) {
    let t = Tokens::get(ui.ctx());
    let Some(n) = doc.node(id) else { return };
    // Only frames and sections have a handoff status.
    let eligible = crate::section_panel::has_status(doc, id);
    let ready = n.dev_status == DevStatus::ReadyForDev;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let cell = |i: f32| Rect::from_min_size(pos2(r.max.x - 8.0 - 24.0 * (i + 1.0) - 2.0 * i, r.center().y - 12.0), vec2(24.0, 24.0));
    // From the right: the menu, the ready toggle (frames and sections), the link.
    let more = cell(0.0);
    let mresp = ui.interact(more, ui.scope_id().with("dev_more"), Sense::click()).tip(tr("More"));
    a11y::menu_button(&mresp, tr("More"), a11y::menu_open(&mresp));
    if mresp.hovered() {
        ui.painter().rect_filled(more, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), more.shrink(6.0), Icon::Dots, t.icon);
    menu_popup(&mresp).show(|ui| {
        if menu_item(ui, tr("Copy link to selection"), crate::shortcuts::shortcut("edit.copyLink").unwrap_or("")) {
            app.run("edit.copyLink", json!({}));
            ui.close();
        }
        if menu_item(ui, tr("Copy code"), "") {
            copy_code(app, code);
            ui.close();
        }
        // The Dev resources section below opens its field for a link.
        if menu_item(ui, tr("Add a dev resource link"), "") {
            ui.data_mut(|d| d.insert_temp(Id::unique(("devres_adding", id)), true));
            ui.close();
        }
        if eligible && menu_item(ui, if ready { tr("Remove ready for dev status") } else { tr("Mark as ready for dev") }, "") {
            toggle_ready(app, id, ready);
            ui.close();
        }
        if ready && menu_item(ui, tr("Mark as completed"), "") {
            app.set_props(&[id], json!({"devStatus": DevStatus::Completed.id()}));
            ui.close();
        }
    });
    let mut link = cell(1.0);
    if eligible {
        let tr_cell = cell(1.0);
        let tip = tr("Toggle ready for dev status");
        let resp = ui.interact(tr_cell, ui.scope_id().with("dev_ready_toggle"), Sense::click()).tip(tip);
        a11y::toggle(&resp, tip, ready);
        let green = crate::toolbar::mode_fill(crate::RightTab::Inspect);
        if ready {
            ui.painter().rect_filled(tr_cell, CornerRadius::same(5), green.gamma_multiply(0.35));
        } else if resp.hovered() {
            ui.painter().rect_filled(tr_cell, CornerRadius::same(5), t.hover);
        }
        icons::paint(ui.painter(), tr_cell.shrink(6.0), Icon::Code, if ready { green } else { t.icon });
        if resp.clicked() {
            toggle_ready(app, id, ready);
        }
        link = cell(2.0);
    }
    if widgets::icon_button_at(ui, Id::unique("dev_copy_link"), link, Icon::Link, tr("Copy link to selection")) {
        app.run("edit.copyLink", json!({}));
    }
    // The name on one line; a long one ends with an ellipsis and shows in full on hover (#1650).
    let name = n.name.replace(['\r', '\n'], " ");
    let shown = widgets::elide_font(ui, &name, link.min.x - r.min.x - PAD - 6.0, semibold(12.0));
    let galley = ui.painter().layout_no_wrap(shown.clone(), semibold(12.0), t.text);
    let at = pos2(r.min.x + PAD, r.center().y - galley.size().y / 2.0);
    let name_rect = Rect::from_min_size(at, galley.size());
    ui.painter().galley(at, galley, t.text);
    if shown != name {
        ui.interact(name_rect, ui.scope_id().with("dev_name"), Sense::hover()).on_hover_text(name);
    }
    // What it is; a frame with a handoff status says that instead.
    let (icon, what) = match n.dev_status {
        DevStatus::ReadyForDev => (Icon::Code, tr(DevStatus::ReadyForDev.label()).to_string()),
        DevStatus::Completed => (Icon::Check, tr(DevStatus::Completed.label()).to_string()),
        DevStatus::None => describe(doc, id),
    };
    let (tr_, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    icons::paint(ui.painter(), Rect::from_center_size(pos2(tr_.min.x + PAD + 8.0, tr_.center().y), vec2(16.0, 16.0)), icon, t.icon_secondary);
    ui.painter().text(pos2(tr_.min.x + PAD + 26.0, tr_.center().y), Align2::LEFT_CENTER, what, regular(11.0), t.text_secondary);
    ui.add_space(6.0);
}

/// Mark layer `id` ready for dev, or take that status off again.
fn toggle_ready(app: &mut FigApp, id: NodeId, ready: bool) {
    let status = if ready { DevStatus::None } else { DevStatus::ReadyForDev };
    app.set_props(&[id], json!({"devStatus": status.id()}));
}

fn copy_code(app: &mut FigApp, code: &str) {
    app.clipboard_set_text(code);
    app.toast(tr("Copied code"));
}

/// An estimate of the tokens `text` takes an AI model (a token is about four characters).
pub(crate) fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// `n` as "812", "1.2k" or "167k".
pub(crate) fn format_tokens(n: usize) -> String {
    match n {
        0..=999 => n.to_string(),
        1000..=9999 => format!("{:.1}k", n as f64 / 1000.0),
        _ => format!("{}k", n / 1000),
    }
}

/// The example prompt for handing the selected layer to an AI model through the MCP server.
pub(super) fn example_prompt(doc: &Document, id: NodeId) -> (String, String) {
    let name = doc.node(id).map(|n| n.name.replace(['\r', '\n'], " ")).unwrap_or_default();
    ("Implement this design from FigCraft.".to_string(), format!("@\"{name}\" (layer {})", id.0))
}

/// The MCP block: what the selection costs an AI model and a prompt to copy.
pub(super) fn mcp(app: &mut FigApp, ui: &mut Ui, doc: &Document, id: NodeId, code: &str) {
    let t = Tokens::get(ui.ctx());
    let (open, _) = section(ui, "mcp", tr("MCP"), None, &[]);
    if !open {
        return;
    }
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, tr("Selection details"), semibold(11.0), t.text);
    let tokens = estimate_tokens(code);
    for (label, value, warn) in
        [(tr("Session activity"), tr("Not sent").to_string(), false), (tr("Estimated tokens"), format_tokens(tokens), tokens > 100_000)]
    {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, label, regular(11.0), t.text_secondary);
        let g = ui.painter().layout_no_wrap(value, regular(11.0), t.text);
        let w = g.size().x;
        ui.painter().galley(pos2(r.min.x + VALUE_X, r.center().y - g.size().y / 2.0), g, t.text);
        if warn {
            ui.painter().text(pos2(r.min.x + VALUE_X + w + 6.0, r.center().y), Align2::LEFT_CENTER, "⚠", regular(11.0), ORANGE);
        }
    }
    ui.add_space(6.0);
    let (line1, line2) = example_prompt(doc, id);
    let box_w = ui.available_width() - PAD * 2.0;
    let mono = FontId::monospace(11.0);
    let g1 = ui.painter().layout(line1, mono.clone(), t.text, box_w - 20.0);
    let g2 = ui.painter().layout(line2, mono, LINK, box_w - 20.0);
    let box_h = g1.size().y + g2.size().y + 20.0;
    let (br, _) = ui.allocate_exact_size(vec2(ui.available_width(), box_h), Sense::hover());
    let boxr = Rect::from_min_size(pos2(br.min.x + PAD, br.min.y), vec2(box_w, box_h));
    ui.painter().rect_stroke(boxr, CornerRadius::same(6), Stroke::new(1.0, t.border), StrokeKind::Inside);
    let y2 = boxr.min.y + 10.0 + g1.size().y;
    ui.painter().galley(pos2(boxr.min.x + 10.0, boxr.min.y + 10.0), g1, t.text);
    ui.painter().galley(pos2(boxr.min.x + 10.0, y2), g2, LINK);
    let (cr, cresp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    let cresp = cresp.tip(tr("Copy example prompt"));
    if cresp.hovered() {
        ui.painter().rect_filled(cr.shrink2(vec2(PAD, 4.0)), CornerRadius::same(5), t.hover);
    }
    ui.painter().text(cr.center(), Align2::CENTER_CENTER, tr("Copy example prompt"), regular(11.0), t.text);
    if cresp.clicked() {
        let (a, b) = example_prompt(doc, id);
        app.clipboard_set_text(&format!("{a} {b} Use the figcraft MCP server."));
        app.toast(tr("Copied example prompt"));
    }
    ui.add_space(4.0);
}

/// The box model of the layer: border, padding and content size, like the browser's inspector,
/// on the kit's grid: a 184 pt box, the border box 32 in from its sides, the padding box 32 in from
/// that horizontally and 24 vertically, the content 32 and 24 further.
pub(super) fn box_model(ui: &mut Ui, doc: &Document, id: NodeId) {
    let t = Tokens::get(ui.ctx());
    let Some(n) = doc.node(id) else { return };
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 192.0), Sense::hover());
    let outer = Rect::from_min_size(pos2(r.min.x + PAD, r.min.y + 4.0), vec2(r.width() - PAD * 2.0, 184.0));
    let p = ui.painter();
    p.rect_filled(outer, CornerRadius::same(5), t.field);
    let border = outer.shrink(32.0);
    p.rect_filled(border, CornerRadius::same(2), t.panel);
    p.rect_stroke(border, CornerRadius::same(2), Stroke::new(1.0, BORDER_RING), StrokeKind::Inside);
    let padding = Rect::from_min_max(pos2(border.min.x + 32.0, border.min.y + 24.0), pos2(border.max.x - 32.0, border.max.y - 24.0));
    p.rect_filled(padding, CornerRadius::same(2), PADDING_FILL);
    p.rect_stroke(padding, CornerRadius::same(2), Stroke::new(1.0, PADDING_RING), StrokeKind::Inside);
    let content = Rect::from_min_max(pos2(padding.min.x + 32.0, padding.min.y + 24.0), pos2(padding.max.x - 32.0, padding.min.y + 50.0));
    p.rect_filled(content, CornerRadius::ZERO, t.panel);
    dashed_rect(p, content, Stroke::new(1.0, t.text));
    // The strokes' weight per side and the auto layout's padding, "-" for none.
    let weight = if n.strokes.is_empty() { 0.0 } else { n.stroke.weight };
    let sides = n.stroke.sides.filter(|_| weight > 0.0).unwrap_or([weight; 4]);
    let pad = n.layout.as_ref().map_or([0.0; 4], |l| l.padding);
    let num = |v: f64| if v > 0.0 { format!("{}", (v * 100.0).round() / 100.0) } else { "-".to_string() };
    let font = regular(11.0);
    let dim = Color32::from_white_alpha(178);
    let at = |pos: egui::Pos2, text: String, color: Color32| p.text(pos, Align2::CENTER_CENTER, text, font.clone(), color);
    // The border ring is a grid of three by three: the corners "-", the four sides' weights between.
    let (top, mid, bottom) = (border.min.y + 12.0, border.center().y, border.max.y - 12.0);
    let (left, centre, right) = (border.min.x + 14.5, border.center().x, border.max.x - 15.0);
    for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        at(pos2(x, y), "-".into(), dim);
    }
    at(pos2(centre, top), num(sides[0]), dim);
    at(pos2(right, mid), num(sides[1]), dim);
    at(pos2(centre, bottom), num(sides[2]), dim);
    at(pos2(left, mid), num(sides[3]), dim);
    p.text(pos2(border.min.x + 40.0, top), Align2::LEFT_CENTER, tr("Border"), font.clone(), dim);
    // The padding ring: its label and top value on the first row, left and right beside the content,
    // the bottom value under it.
    p.text(pos2(padding.min.x + 8.0, padding.min.y + 12.0), Align2::LEFT_CENTER, tr("Padding"), font.clone(), dim);
    let value = |v: f64| if v > 0.0 { (num(v), t.text) } else { ("-".to_string(), dim) };
    let (text, color) = value(pad[0]);
    at(pos2(padding.center().x, padding.min.y + 12.0), text, color);
    let (text, color) = value(pad[1]);
    at(pos2(padding.max.x - 16.0, content.center().y), text, color);
    let (text, color) = value(pad[2]);
    at(pos2(padding.center().x, padding.max.y - 12.0), text, color);
    let (text, color) = value(pad[3]);
    at(pos2(padding.min.x + 16.0, content.center().y), text, color);
    let size = format!("{} × {}", (n.width * 100.0).round() / 100.0, (n.height * 100.0).round() / 100.0);
    at(content.center(), size, t.text);
    p.text(pos2(outer.max.x - 8.0, outer.max.y - 14.0), Align2::RIGHT_CENTER, "border-box", font, Color32::from_white_alpha(102));
}

/// A dashed rectangle outline (one pixel dashes, three on and three off).
fn dashed_rect(p: &egui::Painter, r: Rect, stroke: Stroke) {
    let (on, off) = (3.0, 3.0);
    let edges =
        [(r.left_top(), r.right_top()), (r.right_top(), r.right_bottom()), (r.right_bottom(), r.left_bottom()), (r.left_bottom(), r.left_top())];
    for (a, b) in edges {
        let len = (b - a).length();
        if len < 1.0 {
            continue;
        }
        let dir = (b - a) / len;
        let mut d = 0.0;
        while d < len {
            p.line_segment([a + dir * d, a + dir * (d + on).min(len)], stroke);
            d += on + off;
        }
    }
}

/// The CSS properties of the layout group: where a layer is and how it is sized and laid out.
const LAYOUT_PROPS: &[&str] = &[
    "display",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "z-index",
    "width",
    "height",
    "min-width",
    "min-height",
    "max-width",
    "max-height",
    "flex",
    "flex-direction",
    "flex-wrap",
    "flex-grow",
    "flex-shrink",
    "flex-basis",
    "gap",
    "row-gap",
    "column-gap",
    "padding",
    "margin",
    "align-items",
    "align-self",
    "align-content",
    "justify-content",
    "justify-items",
    "justify-self",
    "grid-template-columns",
    "grid-template-rows",
    "overflow",
    "box-sizing",
    "order",
];

/// The type properties of the typography group.
fn is_type_prop(p: &str) -> bool {
    p.starts_with("font") || p.starts_with("text-") || p.starts_with("line-") || p.starts_with("letter-") || p == "color" || p == "white-space"
}

/// Split CSS declarations into the kit's groups, `Layout`, `Typography` and `Style`, each with its
/// own lines. Comments and blank lines are left out.
pub(crate) fn css_groups(code: &str) -> Vec<(&'static str, Vec<String>)> {
    let mut groups: [(&str, Vec<String>); 3] = [("Layout", Vec::new()), ("Typography", Vec::new()), ("Style", Vec::new())];
    for line in code.lines() {
        let l = line.trim();
        let Some((prop, _)) = l.split_once(':').filter(|_| l.ends_with(';')) else { continue };
        let prop = prop.trim();
        let g = if LAYOUT_PROPS.contains(&prop) || prop.starts_with("padding") || prop.starts_with("margin") {
            0
        } else if is_type_prop(prop) {
            1
        } else {
            2
        };
        groups[g].1.push(l.to_string());
    }
    groups.into_iter().filter(|(_, v)| !v.is_empty()).collect()
}

/// A line of CSS as coloured runs: the property, then its value, keywords and numbers pink,
/// `var(…)` orange.
fn css_job(line: &str, t: &Tokens) -> LayoutJob {
    let mono = FontId::monospace(11.0);
    let fmt = |c: Color32| TextFormat { font_id: mono.clone(), color: c, ..Default::default() };
    let mut job = LayoutJob::default();
    let Some((prop, value)) = line.split_once(':') else {
        job.append(line, 0.0, fmt(t.text));
        return job;
    };
    job.append(prop, 0.0, fmt(t.text));
    job.append(":", 0.0, fmt(t.text_secondary));
    let value = value.strip_suffix(';').unwrap_or(value).trim_start();
    for (i, word) in value.split(' ').enumerate() {
        job.append(" ", 0.0, fmt(t.text));
        let _ = i;
        if let Some(rest) = word.strip_prefix("var(") {
            job.append("var", 0.0, fmt(ORANGE));
            job.append(&format!("({rest}"), 0.0, fmt(t.text));
        } else if word.contains('(') || word.starts_with('"') {
            job.append(word, 0.0, fmt(t.text));
        } else if word.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '#')) {
            job.append(word, 0.0, fmt(PINK));
        } else {
            job.append(word, 0.0, fmt(t.text));
        }
    }
    job.append(";", 0.0, fmt(t.text));
    job
}

/// One block of code lines with a gutter of line numbers; `highlight` colours CSS.
pub(super) fn code_block(ui: &mut Ui, lines: &[String], highlight: bool) {
    let t = Tokens::get(ui.ctx());
    let line_h = 18.0;
    let h = lines.len() as f32 * line_h + 16.0;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let boxr = Rect::from_min_size(pos2(r.min.x + PAD, r.min.y), vec2(r.width() - PAD * 2.0, h));
    let ring = Color32::from_white_alpha(26);
    ui.painter().rect_stroke(boxr, CornerRadius::same(5), Stroke::new(1.0, ring), StrokeKind::Inside);
    // The gutter of line numbers, 21 wide, and the code 24 after it.
    ui.painter().line_segment([pos2(boxr.min.x + 21.0, boxr.min.y + 1.0), pos2(boxr.min.x + 21.0, boxr.max.y - 1.0)], Stroke::new(1.0, ring));
    let clip = ui.painter().with_clip_rect(boxr.shrink(1.0));
    for (i, line) in lines.iter().enumerate() {
        let y = boxr.min.y + 8.0 + i as f32 * line_h + line_h / 2.0;
        clip.text(pos2(boxr.min.x + 17.0, y), Align2::RIGHT_CENTER, (i + 1).to_string(), regular(11.0), Color32::from_white_alpha(102));
        let job = if highlight {
            css_job(line, &t)
        } else {
            let mut j = LayoutJob::default();
            j.append(line, 0.0, TextFormat { font_id: FontId::monospace(11.0), color: t.text, ..Default::default() });
            j
        };
        let g = clip.layout_job(job);
        clip.galley(pos2(boxr.min.x + 45.0, y - g.size().y / 2.0), g, t.text);
    }
}

/// A code group: its label with a copy button, then the block.
pub(super) fn code_group(app: &mut FigApp, ui: &mut Ui, label: &str, lines: &[String], highlight: bool) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, tr(label), regular(11.0), t.text);
    let copy = Rect::from_min_size(pos2(r.max.x - PAD - 24.0, r.center().y - 12.0), vec2(24.0, 24.0));
    if widgets::icon_button_at(ui, Id::unique(("dev_group_copy", label)), copy, Icon::Clipboard, &trf("Copy {value}", &[("value", &tr(label))])) {
        app.clipboard_set_text(&lines.join("\n"));
        app.toast(trf("Copied {value}", &[("value", &tr(label))]));
    }
    code_block(ui, lines, highlight);
    ui.add_space(8.0);
}

/// A small dropdown at `pos` (its left edge, or its right edge when `from_right`) and `cy`: the
/// label, 8 in from the left, then the chevron's 24 pt box, in a 24 pt tall chip with a one point
/// outline. Its response opens the menu.
pub(super) fn chip(ui: &mut Ui, id: Id, label: &str, x: f32, cy: f32, from_right: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let g = ui.painter().layout_no_wrap(label.to_string(), regular(11.0), t.text);
    let w = 8.0 + g.size().x + 24.0 + 2.0;
    let rect = Rect::from_min_size(pos2(if from_right { x - w } else { x }, cy - 12.0), vec2(w, 24.0));
    let resp = ui.interact(rect, id, Sense::click());
    a11y::combo(&resp, "", label, a11y::menu_open(&resp));
    ui.painter().rect_filled(rect, CornerRadius::same(5), t.panel);
    let ring = if resp.hovered() { t.field_hover_border } else { t.border };
    ui.painter().rect_stroke(rect, CornerRadius::same(5), Stroke::new(1.0, ring), StrokeKind::Inside);
    ui.painter().galley(pos2(rect.min.x + 9.0, cy - g.size().y / 2.0), g, t.text);
    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.max.x - 13.0, cy), vec2(16.0, 16.0)), Icon::Chevron, t.icon);
    resp
}

/// The List | Code switch on the left, the language chip on the right of the same row when
/// `lang` names one. Whether Code is chosen (remembered across frames), and the chip's response.
pub(super) fn view_switch(ui: &mut Ui, lang: Option<&str>) -> (bool, Option<egui::Response>) {
    let t = Tokens::get(ui.ctx());
    let id = Id::unique("dev_props_code");
    let code = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(true);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let mut now = code;
    let segs: Vec<(&str, bool, f32)> = [(tr("List"), false), (tr("Code"), true)]
        .into_iter()
        .map(|(label, is_code)| (label, is_code, ui.painter().layout_no_wrap(label.to_string(), regular(11.0), t.text).size().x + 16.0))
        .collect();
    // A track, with the chosen segment raised on it: the panel's colour inside a one point ring.
    let total: f32 = segs.iter().map(|s| s.2).sum();
    let track = Rect::from_min_size(pos2(r.min.x + PAD, r.center().y - 12.0), vec2(total, 24.0));
    ui.painter().rect_filled(track, CornerRadius::same(5), t.field);
    let mut x = track.min.x;
    for (label, is_code, w) in segs {
        let seg = Rect::from_min_size(pos2(x, track.min.y), vec2(w, 24.0));
        let resp = ui.interact(seg, ui.scope_id().with(("dev_switch", label)), Sense::click());
        let on = is_code == code;
        a11y::toggle(&resp, label, on);
        if on {
            ui.painter().rect_filled(seg, CornerRadius::same(5), t.panel);
            ui.painter().rect_stroke(seg, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
        }
        ui.painter().text(seg.center(), Align2::CENTER_CENTER, label, regular(11.0), if on { t.text } else { Color32::from_white_alpha(178) });
        if resp.clicked() {
            now = is_code;
        }
        x += w;
    }
    if now != code {
        ui.data_mut(|d| d.insert_temp(id, now));
    }
    let chip_resp = lang.filter(|_| now).map(|l| chip(ui, Id::unique("dev_lang_chip"), l, r.max.x - PAD, r.center().y, true));
    (now, chip_resp)
}

/// Where the code language and the unit are remembered (they are settings of the file view, shared
/// by the page's Code settings and the layer's language chip).
pub(super) fn lang_id() -> Id {
    Id::unique("inspect_lang")
}

pub(super) fn rem_id() -> Id {
    Id::unique("inspect_rem")
}

/// The code language chosen.
pub(super) fn current_lang(ui: &Ui) -> CodeLang {
    ui.data(|d| d.get_temp::<usize>(lang_id())).and_then(|i| CodeLang::ALL.get(i).copied()).unwrap_or_default()
}

/// The language menu: every language, the chosen one first checked.
pub(super) fn language_menu(resp: &egui::Response) {
    menu_popup(resp).show(|ui| {
        for (i, l) in CodeLang::ALL.iter().enumerate() {
            if menu_item(ui, l.label(), "") {
                ui.data_mut(|d| d.insert_temp(lang_id(), i));
                ui.close();
            }
        }
    });
}

/// The page's header when no layer is selected: its name and "Page" under it.
pub(super) fn page_header(ui: &mut Ui, doc: &Document, page: NodeId) {
    let t = Tokens::get(ui.ctx());
    let Some(n) = doc.node(page) else { return };
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let name = widgets::elide_font(ui, &n.name.replace(['\r', '\n'], " "), r.width() - PAD * 2.0, semibold(12.0));
    ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, name, semibold(12.0), t.text);
    let (icon, what) = describe(doc, page);
    let (tr_, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    icons::paint(ui.painter(), Rect::from_center_size(pos2(tr_.min.x + PAD + 8.0, tr_.center().y), vec2(16.0, 16.0)), icon, t.icon_secondary);
    ui.painter().text(pos2(tr_.min.x + PAD + 26.0, tr_.center().y), Align2::LEFT_CENTER, what, regular(11.0), t.text_secondary);
    ui.add_space(6.0);
}

/// The Code settings of the page: the language and the unit the code is written in.
pub(super) fn code_settings(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (open, _) = section(ui, "code_settings", tr("Code settings"), None, &[]);
    if !open {
        return;
    }
    let lang = current_lang(ui);
    let rem = ui.data(|d| d.get_temp::<bool>(rem_id())).unwrap_or(false);
    let rows = [(tr("Language"), lang.label().to_string()), (tr("Unit"), (if rem { "rem" } else { "px" }).to_string())];
    for (i, (label, value)) in rows.iter().enumerate() {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
        ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, *label, regular(11.0), t.text_secondary);
        // The chip's text starts where the other rows' values do.
        let resp = chip(ui, Id::unique(("dev_setting", i)), value, r.min.x + VALUE_X - 9.0, r.center().y, false);
        if i == 0 {
            language_menu(&resp);
        } else {
            menu_popup(&resp).show(|ui| {
                for (on, label) in [(false, "px"), (true, "rem")] {
                    if menu_item(ui, label, "") {
                        ui.data_mut(|d| d.insert_temp(rem_id(), on));
                        ui.close();
                    }
                }
            });
        }
    }
    ui.add_space(8.0);
}

/// Code Connect for a layer that isn't a component: where it stands and what it needs.
pub(super) fn code_connect_intro(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().text(pos2(r.min.x + PAD, r.center().y), Align2::LEFT_CENTER, tr("Code Connect"), semibold(11.0), t.text);
    let g = ui.painter().layout_no_wrap(tr("Not started").to_string(), regular(11.0), t.text_secondary);
    let chip = Rect::from_min_size(pos2(r.max.x - PAD - g.size().x - 16.0, r.center().y - 12.0), vec2(g.size().x + 16.0, 24.0));
    ui.painter().rect_stroke(chip, CornerRadius::same(5), Stroke::new(1.0, t.border), StrokeKind::Inside);
    ui.painter().galley(pos2(chip.min.x + 8.0, r.center().y - g.size().y / 2.0), g, t.text_secondary);
    let text = tr("Publish components to a library, then you can connect them to your codebase with Code Connect.");
    let g = ui.painter().layout(text.to_string(), regular(11.0), t.text_secondary, ui.available_width() - PAD * 2.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.size().y + 12.0), Sense::hover());
    ui.painter().galley(pos2(r.min.x + PAD, r.min.y), g, t.text_secondary);
}
