//! `Options::solo` (Dev Mode's focus view): only one layer of the page is drawn, where it is, also
//! by the retained renderer.

use std::sync::Arc;

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, NodeKind};

use crate::{Options, Retained, View, render_view};

fn pixel(p: &crate::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let c = p.pixel(x, y).map(|c| c.demultiply()).map(|c| [c.red(), c.green(), c.blue(), c.alpha()]);
    c.unwrap_or_default()
}

#[test]
fn solo_draws_one_layer_in_place() {
    let mut d = Document::new("solo");
    let page = d.first_page().unwrap();
    let sec = d.create(page, NodeKind::Section { contents_hidden: false }, "Section", 0.0, 0.0, 400.0, 200.0).unwrap();
    let a = d.create(sec, NodeKind::Frame, "A", 20.0, 20.0, 100.0, 100.0).unwrap();
    let b = d.create(page, NodeKind::Rectangle, "B", 200.0, 20.0, 100.0, 100.0).unwrap();
    d.get_mut(a).unwrap().fills = vec![Paint::solid(Color::from_hex("#FF0000").unwrap())];
    d.get_mut(b).unwrap().fills = vec![Paint::solid(Color::from_hex("#0000FF").unwrap())];
    d.get_mut(sec).unwrap().fills = vec![Paint::solid(Color::from_hex("#00FF00").unwrap())];
    let view = View { origin: (0.0, 0.0), scale: 1.0, width: 400, height: 200 };
    let all = render_view(&d, page, &view, &Options::default()).unwrap();
    assert_eq!(pixel(&all, 250, 50)[2], 255, "B drawn");
    assert_eq!(pixel(&all, 10, 10)[1], 255, "the section drawn");
    let solo = Options { solo: Some(a), ..Default::default() };
    let one = render_view(&d, page, &view, &solo).unwrap();
    assert_eq!(pixel(&one, 50, 50), [255, 0, 0, 255], "A where it is (inside its section)");
    let bg = Color::DEFAULT_PAGE.to_rgba8();
    assert_eq!(pixel(&one, 250, 50), bg, "B left out");
    assert_eq!(pixel(&one, 10, 10), bg, "the section around A left out");
    // A layer of another page (or the page itself) draws the page as usual.
    for other in [page, figcraft_doc::NodeId(999)] {
        let p = render_view(&d, page, &view, &Options { solo: Some(other), ..Default::default() }).unwrap();
        assert_eq!(pixel(&p, 250, 50)[2], 255);
    }
    // The retained renderer redraws when solo changes.
    let doc = Arc::new(d);
    let mut r = Retained::default();
    let (_, p, _) = r.render(&doc, page, &view, &Options::default()).unwrap();
    assert_eq!(pixel(p, 250, 50)[2], 255);
    let (_, p, _) = r.render(&doc, page, &view, &solo).unwrap();
    assert_eq!(pixel(p, 250, 50), bg);
}

/// `Options::lifted` (layers dragged on the canvas): only they are drawn, where they are, over
/// nothing; the rest of the page is drawn without them (`skip`), so the two put together are the
/// page.
#[test]
fn lifted_layers_draw_alone_over_nothing() {
    let mut d = Document::new("lifted");
    let page = d.first_page().unwrap();
    let sec = d.create(page, NodeKind::Section { contents_hidden: false }, "Section", 0.0, 0.0, 150.0, 150.0).unwrap();
    let a = d.create(sec, NodeKind::Frame, "A", 20.0, 20.0, 100.0, 100.0).unwrap();
    let b = d.create(page, NodeKind::Rectangle, "B", 200.0, 20.0, 100.0, 100.0).unwrap();
    d.get_mut(a).unwrap().fills = vec![Paint::solid(Color::from_hex("#FF0000").unwrap())];
    d.get_mut(b).unwrap().fills = vec![Paint::solid(Color::from_hex("#0000FF").unwrap())];
    let view = View { origin: (0.0, 0.0), scale: 1.0, width: 400, height: 200 };
    let lifted = Options { lifted: vec![sec], ..Default::default() };
    let up = render_view(&d, page, &view, &lifted).unwrap();
    assert_eq!(pixel(&up, 50, 50), [255, 0, 0, 255], "the section's frame where it is");
    assert_eq!(pixel(&up, 250, 50)[3], 0, "B left out");
    assert_eq!(pixel(&up, 350, 180)[3], 0, "no page background");
    let rest = render_view(&d, page, &view, &Options { skip: vec![sec], ..Default::default() }).unwrap();
    assert_eq!(pixel(&rest, 50, 50), Color::DEFAULT_PAGE.to_rgba8(), "the rest without the section");
    assert_eq!(pixel(&rest, 250, 50)[2], 255);
    // The retained renderer redraws when the lifted layers change.
    let doc = Arc::new(d);
    let mut r = Retained::default();
    let (_, p, _) = r.render(&doc, page, &view, &Options::default()).unwrap();
    assert_eq!(pixel(p, 250, 50)[2], 255);
    let (_, p, _) = r.render(&doc, page, &view, &lifted).unwrap();
    assert_eq!(pixel(p, 250, 50)[3], 0);
}
