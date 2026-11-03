use super::*;
use figcraft_doc::{LineHeight, TextData};

#[test]
fn maximum_height_truncates_auto_text_in_measurement_and_rendering() {
    scan_system_fonts();
    for mode in [TextAutoResize::Height, TextAutoResize::WidthAndHeight] {
        let mut t = TextData::new("First line\nSecond line\nThird line\nFourth line");
        t.auto_resize = mode;
        t.style.line_height = LineHeight::Pixels(24.0);
        let mut n = Node::new(figcraft_doc::NodeId(1), NodeKind::Text(t), "Limited text", 375.0, 200.0);
        n.child_layout.max_h = Some(48.0);
        let (_, measured_height) = auto_size(&n).unwrap();
        assert_eq!(measured_height, 48.0);
        n.height = measured_height;
        let (l, _) = layout_node(&n).unwrap();
        assert_eq!(l.lines.len(), 2);
        assert!(l.truncated);
        assert_eq!(l.height, 48.0);
        n.child_layout.max_h = None;
        assert_eq!(auto_size(&n).unwrap().1, 96.0);
        assert!(!layout_node(&n).unwrap().0.truncated);
    }
}
