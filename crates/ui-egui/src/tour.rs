//! The editor tour: five dark coach marks that point at the toolbar, the Design panel, the Fill
//! section, the layers and the Assets tab, with Back, Next and Done (as UI3's first-file tour).
//! Shown after onboarding makes the first design file, and by Help ▸ Take the tour (`help.tour`).

use egui::{Align2, Color32, Context, CornerRadius, Id, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, pos2, vec2};

use crate::FigApp;
use crate::i18n::{tr, tr_noop};
use crate::icons::{self, Icon};
use crate::theme::{Tokens, regular, semibold};

/// Where the tour's steps point, as drawn this frame (none while hidden).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Anchors {
    pub toolbar: Option<Rect>,
    pub fill: Option<Rect>,
    pub layers: Option<Rect>,
    pub assets: Option<Rect>,
}

#[derive(Clone, Copy)]
enum Target {
    Toolbar,
    Design,
    Fill,
    Layers,
    Assets,
}

const STEPS: [(&str, &str, Target); 5] = [
    (
        tr_noop("Pick a tool"),
        tr_noop("Draw frames, shapes, text and paths from the toolbar. Press F for a frame, R for a rectangle and T for text."),
        Target::Toolbar,
    ),
    (
        tr_noop("Adjust properties"),
        tr_noop("Change the size, position, layout and more of whatever is selected, here in the Design panel."),
        Target::Design,
    ),
    (tr_noop("Change fills"), tr_noop("Set a layer's colour, gradient or image here. Click the swatch to open the colour picker."), Target::Fill),
    (tr_noop("Find your layers"), tr_noop("Everything on the page is listed here. Drag to reorder, double-click to rename."), Target::Layers),
    (
        tr_noop("Assets are always here for you"),
        tr_noop("Quickly find components, styles and your team's libraries whenever you need them by opening the Assets tab."),
        Target::Assets,
    ),
];

const CARD_W: f32 = 300.0;

/// Draw the current step, if the tour is on and a file is shown.
pub fn show(app: &mut FigApp, ctx: &Context) {
    let Some(i) = app.ui.tour else { return };
    let Some((title, body, target)) = STEPS.get(i).copied() else {
        app.ui.tour = None;
        return;
    };
    if app.ui.home || app.session.ds().is_err() {
        return;
    }
    let screen = ctx.content_rect();
    // The last step shows the Assets tab it is about; the others the layers.
    let tab = if matches!(target, Target::Assets) { crate::LeftTab::Assets } else { crate::LeftTab::File };
    if app.ui.left_tab != tab {
        app.ui.left_tab = tab;
        ctx.request_repaint();
    }
    let a = app.ui.tour_anchors;
    // The rail item and the panel beside it, so the card stands clear of what it is about.
    let panel = |r: Rect| {
        let w = if app.ui.left_collapsed { 0.0 } else { app.ui.left_width };
        Rect::from_min_size(r.min, vec2(r.width() + w, 260.0))
    };
    let anchor = match target {
        Target::Toolbar => a.toolbar,
        Target::Design => app.ui.right_panel_rect.map(|r| Rect::from_min_size(r.min, vec2(r.width(), r.height().min(220.0)))),
        Target::Fill => a.fill.or(app.ui.right_panel_rect.map(|r| Rect::from_min_size(r.min, vec2(r.width(), 120.0)))),
        Target::Layers => a.layers.map(panel),
        Target::Assets => a.assets.map(panel),
    };
    let t = Tokens::get(ctx);
    let mut next = None;
    egui::Area::new(Id::unique("tour")).order(Order::Foreground).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        let p = ui.painter().clone();
        let body_g = p.layout(tr(body).to_string(), regular(11.0), t.text, CARD_W - 32.0);
        let h = 40.0 + 12.0 + body_g.size().y + 16.0 + 24.0 + 12.0;
        let size = vec2(CARD_W, h);
        // Beside what it points at: above the toolbar, left of the right panel, right of the left one.
        let (card, tip) = match (anchor, target) {
            (Some(r), Target::Toolbar) => {
                let c = Rect::from_min_size(pos2(r.center().x - CARD_W / 2.0, r.min.y - 14.0 - h), size);
                (c, Some((pos2(r.center().x, r.min.y - 4.0), Side::Below)))
            }
            (Some(r), Target::Design | Target::Fill) => {
                let c = Rect::from_min_size(pos2(r.min.x - 14.0 - CARD_W, r.center().y - h / 2.0), size);
                (c, Some((pos2(r.min.x - 4.0, r.center().y), Side::Right)))
            }
            (Some(r), _) => {
                let c = Rect::from_min_size(pos2(r.max.x + 14.0, r.center().y - h / 2.0), size);
                (c, Some((pos2(r.max.x + 4.0, r.center().y), Side::Left)))
            }
            (None, _) => (Rect::from_center_size(pos2(screen.center().x, screen.max.y - 140.0 - h / 2.0), size), None),
        };
        let card = keep_inside(card, screen.shrink(8.0));
        // A white card with the menus' shadow.
        for (grow, alpha) in [(10.0, 6), (5.0, 10), (1.5, 18)] {
            p.rect_filled(
                card.expand(grow).translate(vec2(0.0, grow * 0.4)),
                CornerRadius::same((8.0 + grow) as u8),
                Color32::from_black_alpha(alpha),
            );
        }
        p.rect_filled(card, CornerRadius::same(8), t.panel);
        p.rect_stroke(card, CornerRadius::same(8), Stroke::new(0.5, t.border), StrokeKind::Outside);
        if let Some((point, side)) = tip {
            pointer(&p, card, point, side, t.panel, t.border);
        }
        ui.interact(card, Id::unique("tour_card"), Sense::click());
        let x = card.min.x + 16.0;
        p.text(pos2(x, card.min.y + 20.0), Align2::LEFT_CENTER, tr(title), semibold(11.0), t.text);
        let close = Rect::from_center_size(pos2(card.max.x - 20.0, card.min.y + 20.0), vec2(24.0, 24.0));
        let close_r = ui.interact(close, Id::unique("tour_close"), Sense::click());
        crate::a11y::button(&close_r, tr("Close"));
        if close_r.hovered() {
            p.rect_filled(close, CornerRadius::same(5), t.hover);
        }
        icons::paint(&p, close.shrink(4.0), Icon::Close, t.icon);
        p.line_segment([pos2(card.min.x, card.min.y + 40.0), pos2(card.max.x, card.min.y + 40.0)], Stroke::new(1.0, t.border));
        if close_r.clicked() {
            next = Some(None);
        }
        let by = card.min.y + 52.0;
        p.galley(pos2(x, by), body_g.clone(), t.text);
        let row = by + body_g.size().y + 16.0;
        p.text(pos2(x, row + 12.0), Align2::LEFT_CENTER, format!("{} of {}", i + 1, STEPS.len()), regular(11.0), t.text_secondary);
        let last = i + 1 == STEPS.len();
        let label = if last { tr("Done") } else { tr("Next") };
        let nw = p.layout_no_wrap(label.to_string(), semibold(11.0), Color32::WHITE).size().x + 20.0;
        let nr = Rect::from_min_size(pos2(card.max.x - 16.0 - nw, row), vec2(nw, 24.0));
        let n = ui.interact(nr, Id::unique("tour_next"), Sense::click());
        crate::a11y::button(&n, label);
        p.rect_filled(nr, CornerRadius::same(5), if n.hovered() { t.accent_hover } else { t.accent });
        p.text(nr.center(), Align2::CENTER_CENTER, label, semibold(11.0), t.accent_on);
        if n.clicked() {
            next = Some(if last { None } else { Some(i + 1) });
        }
        if i > 0 {
            let bw = p.layout_no_wrap(tr("Back").to_string(), semibold(11.0), t.text).size().x + 20.0;
            let br = Rect::from_min_size(pos2(nr.min.x - 6.0 - bw, row), vec2(bw, 24.0));
            let b = ui.interact(br, Id::unique("tour_back"), Sense::click());
            crate::a11y::button(&b, tr("Back"));
            if b.hovered() {
                p.rect_filled(br, CornerRadius::same(5), t.hover);
            }
            p.text(br.center(), Align2::CENTER_CENTER, tr("Back"), semibold(11.0), t.text);
            if b.clicked() {
                next = Some(Some(i - 1));
            }
        }
    });
    if ctx.input(|inp| inp.key_pressed(egui::Key::Escape)) {
        next = Some(None);
    }
    if let Some(n) = next {
        app.ui.tour = n;
        if n.is_none() {
            app.ui.left_tab = crate::LeftTab::File;
        }
    }
}

#[derive(Clone, Copy)]
enum Side {
    /// The card is above what it points at.
    Below,
    /// The card is left of it.
    Right,
    /// The card is right of it.
    Left,
}

/// The small triangle from the card toward what it points at.
fn pointer(p: &egui::Painter, card: Rect, point: Pos2, side: Side, fill: Color32, edge: Color32) {
    let k = 7.0;
    let pts = match side {
        Side::Below => {
            let x = point.x.clamp(card.min.x + 16.0, card.max.x - 16.0);
            vec![pos2(x - k, card.max.y), pos2(x + k, card.max.y), pos2(x, card.max.y + k)]
        }
        Side::Right => {
            let y = point.y.clamp(card.min.y + 16.0, card.max.y - 16.0);
            vec![pos2(card.max.x, y - k), pos2(card.max.x, y + k), pos2(card.max.x + k, y)]
        }
        Side::Left => {
            let y = point.y.clamp(card.min.y + 16.0, card.max.y - 16.0);
            vec![pos2(card.min.x, y - k), pos2(card.min.x, y + k), pos2(card.min.x - k, y)]
        }
    };
    // Its two outer edges carry the card's border; the side on the card doesn't.
    if let [a, b, c] = pts[..] {
        p.add(Shape::convex_polygon(vec![a, b, c], fill, Stroke::NONE));
        p.line_segment([a, c], Stroke::new(0.5, edge));
        p.line_segment([b, c], Stroke::new(0.5, edge));
    }
}

fn keep_inside(r: Rect, bounds: Rect) -> Rect {
    let dx = (bounds.min.x - r.min.x).max(0.0) - (r.max.x - bounds.max.x).max(0.0);
    let dy = (bounds.min.y - r.min.y).max(0.0) - (r.max.y - bounds.max.y).max(0.0);
    r.translate(vec2(dx, dy))
}
