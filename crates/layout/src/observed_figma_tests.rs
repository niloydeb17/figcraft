//! Synthetic cases observed in Figma Desktop on 2026-11-04, including changes after creation.
use super::tests::{FakeMeasure, pos, text};
use super::*;

#[test]
fn empty_hug_keeps_its_last_size_after_removing_the_last_child() {
    let mut doc = Document::new("empty");
    let page = doc.first_page().unwrap();
    let frame = doc.create(page, NodeKind::Frame, "Empty", 0.0, 0.0, 375.0, 200.0).unwrap();
    let n = doc.node_mut(frame).unwrap();
    n.layout = Some(AutoLayout { padding: [16.0, 24.0, 20.0, 28.0], ..Default::default() });
    n.child_layout.sizing_h = Sizing::Hug;
    n.child_layout.sizing_v = Sizing::Hug;
    layout_frame(&mut doc, frame, &NoMeasure);
    assert_eq!(pos(&doc, frame), (0.0, 0.0, 375.0, 200.0));
    let child = doc.create(frame, NodeKind::Rectangle, "Child", 0.0, 0.0, 100.0, 40.0).unwrap();
    layout_frame(&mut doc, frame, &NoMeasure);
    assert_eq!(pos(&doc, frame), (0.0, 0.0, 152.0, 76.0));
    doc.remove(child).unwrap();
    layout_frame(&mut doc, frame, &NoMeasure);
    assert_eq!(pos(&doc, frame), (0.0, 0.0, 152.0, 76.0));
    doc.node_mut(frame).unwrap().child_layout.max_w = Some(40.0);
    layout_frame(&mut doc, frame, &NoMeasure);
    assert_eq!(doc.node(frame).unwrap().width, 52.0); // Padding still sets the floor.
}

#[test]
fn a_maximum_wraps_a_hugging_main_axis_before_sizing_the_cross_axis() {
    for horizontal in [true, false] {
        for start in [100.0, 375.0, 768.0, 1280.0, 1920.0] {
            let mut doc = Document::new("wrap");
            let page = doc.first_page().unwrap();
            let frame = doc.create(page, NodeKind::Frame, "Wrapped hug", 0.0, 0.0, start, start).unwrap();
            let n = doc.node_mut(frame).unwrap();
            n.layout = Some(AutoLayout {
                mode: if horizontal { LayoutMode::Horizontal } else { LayoutMode::Vertical },
                wrap: true,
                gap: 12.0,
                cross_gap: 18.0,
                padding: if horizontal { [16.0, 24.0, 20.0, 28.0] } else { [28.0, 20.0, 24.0, 16.0] },
                ..Default::default()
            });
            n.child_layout.sizing_h = Sizing::Hug;
            n.child_layout.sizing_v = Sizing::Hug;
            if horizontal {
                n.child_layout.max_w = Some(350.0);
            } else {
                n.child_layout.max_h = Some(350.0);
            }
            let kids: Vec<_> = [30.0, 40.0, 50.0]
                .into_iter()
                .map(|cross| {
                    let (w, h) = if horizontal { (200.0, cross) } else { (cross, 200.0) };
                    doc.create(frame, NodeKind::Rectangle, "Child", 0.0, 0.0, w, h).unwrap()
                })
                .collect();
            layout_frame(&mut doc, frame, &NoMeasure);
            let n = doc.node(frame).unwrap();
            assert_eq!((n.width, n.height), if horizontal { (350.0, 192.0) } else { (192.0, 350.0) });
            assert_eq!(pos(&doc, kids[1]), if horizontal { (28.0, 64.0, 200.0, 40.0) } else { (64.0, 28.0, 40.0, 200.0) });
            // Removing the maximum allows one unwrapped natural-size line again.
            let n = doc.node_mut(frame).unwrap();
            n.child_layout.max_w = None;
            n.child_layout.max_h = None;
            layout_frame(&mut doc, frame, &NoMeasure);
            let n = doc.node(frame).unwrap();
            assert_eq!((n.width, n.height), if horizontal { (676.0, 86.0) } else { (86.0, 676.0) });
        }
    }
}

#[test]
fn remeasuring_fill_text_preserves_its_height_limits() {
    for mode in [LayoutMode::Horizontal, LayoutMode::Vertical] {
        let mut doc = Document::new("text");
        let page = doc.first_page().unwrap();
        let frame = doc.create(page, NodeKind::Frame, "Parent", 0.0, 0.0, 100.0, 100.0).unwrap();
        let n = doc.node_mut(frame).unwrap();
        n.layout = Some(AutoLayout { mode, padding: [0.0; 4], ..Default::default() });
        n.child_layout.sizing_v = Sizing::Hug;
        let child = text(&mut doc, frame, "Long text");
        let n = doc.node_mut(child).unwrap();
        n.child_layout.sizing_h = Sizing::Fill;
        n.child_layout.sizing_v = Sizing::Hug;
        n.child_layout.max_h = Some(48.0);
        if let NodeKind::Text(t) = &mut n.kind {
            t.auto_resize = TextAutoResize::Height;
        }
        let m = FakeMeasure([(child, (12000.0, 24.0, 18.0))].into_iter().collect());
        for width in [375.0, 768.0, 1280.0, 1920.0, 64.0] {
            set_size(&mut doc, frame, width, 100.0);
            layout_frame(&mut doc, frame, &m);
            assert_eq!(doc.node(child).unwrap().height, 48.0);
            assert_eq!(doc.node(frame).unwrap().height, 48.0);
        }
        doc.node_mut(child).unwrap().child_layout.min_h = Some(72.0);
        layout_frame(&mut doc, frame, &m);
        assert_eq!(doc.node(child).unwrap().height, 72.0); // Conflicting min wins.
    }
}

#[test]
fn wrapping_uses_the_signed_gap_and_no_reserved_space_for_automatic_gaps() {
    for auto in [false, true] {
        let mut doc = Document::new("wrap gaps");
        let page = doc.first_page().unwrap();
        let frame = doc.create(page, NodeKind::Frame, "Wrapped", 0.0, 0.0, if auto { 375.0 } else { 350.0 }, 200.0).unwrap();
        doc.node_mut(frame).unwrap().layout = Some(AutoLayout {
            padding: [16.0, 24.0, 20.0, 28.0],
            gap: if auto { 12.0 } else { -24.0 },
            cross_gap: 18.0,
            wrap: true,
            main_align: if auto { MainAlign::SpaceBetween } else { MainAlign::Min },
            cross_align: if auto { CrossAlign::Center } else { CrossAlign::Min },
            ..Default::default()
        });
        let sizes = if auto { vec![(100.0, 40.0); 3] } else { vec![(140.0, 30.0), (180.0, 60.0), (220.0, 40.0), (80.0, 20.0)] };
        let kids: Vec<_> = sizes.into_iter().map(|(w, h)| doc.create(frame, NodeKind::Rectangle, "Child", 0.0, 0.0, w, h).unwrap()).collect();
        layout_frame(&mut doc, frame, &NoMeasure);
        if auto {
            assert_eq!(pos(&doc, kids[1]), (139.5, 78.0, 100.0, 40.0));
            assert_eq!(pos(&doc, kids[2]), (251.0, 78.0, 100.0, 40.0));
        } else {
            assert_eq!(pos(&doc, kids[1]), (144.0, 16.0, 180.0, 60.0));
            assert_eq!(pos(&doc, kids[2]), (28.0, 94.0, 220.0, 40.0));
        }
    }
}
