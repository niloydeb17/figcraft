//! The Design tab with nothing selected: the page's background, its local styles, its export
//! setting and the MCP row, laid out as the UI3 kit's page panel (headings and the MCP row start
//! at an 8 pt margin; the colour row and the style groups at 16).

use egui::{Align2, Color32, CornerRadius, Id, Rect, Sense, Ui, pos2, vec2};
use figcraft_color::Paint;
use figcraft_doc::{Document, NodeId, NodeKind};
use serde_json::json;

use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular, semibold};
use crate::widgets::{self, PAD};

/// The kit's margin for headings, buttons and the MCP row.
const M: f32 = 8.0;
/// A heading's, a button's and a colour row's heights.
const HEADING_H: f32 = 40.0;
const BUTTON: f32 = 24.0;
const ROW_H: f32 = 32.0;
/// The colour field's height; its width is what the row leaves beside the eye button.
const FIELD_H: f32 = 24.0;
/// The command the MCP row's button copies, to start FigCraft's MCP server from an agent's settings.
const MCP_COMMAND: &str = "figcraft-cli mcp";

/// A 24 pt icon button in `r`: the glyph is drawn at the kit's size (24 pt on a 24 pt button).
fn icon_button(ui: &mut Ui, r: Rect, key: &str, icon: Icon, color: Color32, tip: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(r, ui.scope_id().with(("page_btn", key)), Sense::click()).tip(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(5), t.hover);
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, color);
    resp.clicked()
}

/// A 40 pt heading line: `title` at `x` from the left, and the rect its 24 pt button takes at the
/// right margin.
fn heading(ui: &mut Ui, title: &str, x: f32, color: Color32) -> (Rect, Rect) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), HEADING_H), Sense::hover());
    ui.painter().text(pos2(r.min.x + x, r.center().y), Align2::LEFT_CENTER, title, semibold(11.0), color);
    (r, Rect::from_min_size(pos2(r.max.x - M - BUTTON, r.center().y - BUTTON / 2.0), vec2(BUTTON, BUTTON)))
}

/// The page section: background, local styles, export and MCP. Colour profile and the variables
/// list have their own commands (`dialog.colorProfile`, the Variables button of the left rail).
pub(super) fn page_section(app: &mut FigApp, ui: &mut Ui, doc: &Document) {
    let t = Tokens::get(ui.ctx());
    let Ok(page) = app.session.page() else { return };
    let bg = match doc.node(page).map(|n| &n.kind) {
        Some(NodeKind::Page { background, .. }) => *background,
        _ => figcraft_color::Color::DEFAULT_PAGE,
    };
    // Page.
    let (_, modes) = heading(ui, tr("Page"), M, t.text);
    if icon_button(ui, modes, "modes", Icon::PageModes, t.icon, tr("Apply variable mode")) {
        app.ui.preferences = false;
        app.ui.palette = None;
        app.ui.variables.get_or_insert_with(Default::default);
    }
    background_row(app, ui, bg);
    ui.add_space(12.0);
    widgets::separator(ui);
    // Styles.
    heading(ui, tr("Styles"), PAD, t.text);
    crate::style_picker::local_styles(app, ui, doc);
    ui.add_space(16.0);
    widgets::separator(ui);
    // Export.
    export_heading(app, ui, doc, page);
    widgets::separator(ui);
    // MCP.
    mcp_row(app, ui);
    widgets::separator(ui);
}

/// The page colour row: a swatch and its hex in one 24 pt field, the opacity beside them, and the
/// eye at the right margin.
fn background_row(app: &mut FigApp, ui: &mut Ui, bg: figcraft_color::Color) {
    let t = Tokens::get(ui.ctx());
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let eye = Rect::from_min_size(pos2(row.max.x - M - BUTTON, row.min.y + 4.0), vec2(BUTTON, BUTTON));
    let field = Rect::from_min_max(pos2(row.min.x + PAD, row.min.y + 4.0), pos2(eye.min.x - 8.0, row.min.y + 4.0 + FIELD_H));
    ui.painter().rect_filled(field, CornerRadius::same(5), t.field);
    // Swatch: 14 pt, 5 pt in from the field's edge.
    let sw = Rect::from_center_size(pos2(field.min.x + 12.0, field.center().y), vec2(14.0, 14.0));
    let hit = ui.interact(sw.expand2(vec2(4.0, 5.0)), ui.scope_id().with("page_bg_swatch"), Sense::click());
    crate::a11y::color(&hit, tr("Color"), &bg.to_hex());
    widgets::paint_swatch(ui.painter(), sw, &Paint::solid(bg));
    app.picker_control(crate::colorpicker::PickerTarget::PageBackground, &hit);
    // Opacity: "100" and "%", the last 54 pt of the field.
    ui.painter().text(pos2(field.max.x - 17.0, field.center().y), Align2::RIGHT_CENTER, "%", regular(11.0), t.text_secondary);
    ui.painter().text(pos2(field.max.x - 47.0, field.center().y), Align2::LEFT_CENTER, "100", regular(11.0), t.text);
    // The hex, editable in place.
    let hex = Rect::from_min_max(pos2(field.min.x + 24.0, field.min.y), pos2(field.max.x - 56.0, field.max.y));
    if let Some(h) = widgets::text_field_inline(ui, Id::unique("pagebg"), &bg.to_hex(), hex, regular(11.0))
        && let Some(c) = figcraft_color::Color::from_hex_or_name(&h)
    {
        app.run("page.setBackground", json!({"color": c.to_css_hex()}));
    }
    // The eye is the kit's: the page's colour is always shown.
    icons::paint(ui.painter(), Rect::from_center_size(eye.center(), vec2(16.0, 16.0)), Icon::Eye, t.icon);
}

/// The Export heading: dim, with its `+` at the right margin; the button exports the page as PNG.
fn export_heading(app: &mut FigApp, ui: &mut Ui, doc: &Document, page: NodeId) {
    let t = Tokens::get(ui.ctx());
    let (_, plus) = heading(ui, tr("Export"), M, t.text_secondary);
    if icon_button(ui, plus, "export", Icon::Plus, t.icon_secondary, tr("Export page as PNG")) {
        crate::export_panel::export_page_png(app, doc, page);
    }
}

/// The MCP row: its label, how many agents are connected, and the button that copies the command
/// an agent's settings use to start FigCraft's MCP server.
fn mcp_row(app: &mut FigApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), M * 2.0 + BUTTON), Sense::hover());
    let mid = r.center().y;
    let label = ui.painter().text(pos2(r.min.x + M, mid), Align2::LEFT_CENTER, tr("MCP"), semibold(11.0), t.text);
    ui.painter().text(pos2(label.max.x + 4.0 + 4.0, mid), Align2::LEFT_CENTER, tr("Not connected"), regular(11.0), t.text_secondary);
    let button = Rect::from_min_size(pos2(r.max.x - M - BUTTON, r.min.y + M), vec2(BUTTON, BUTTON));
    if icon_button(ui, button, "mcp", Icon::McpSettings, t.icon, tr("Copy the MCP server command")) {
        app.clipboard_set_text(MCP_COMMAND);
        app.toast(tr("Copied the MCP server command"));
    }
}
