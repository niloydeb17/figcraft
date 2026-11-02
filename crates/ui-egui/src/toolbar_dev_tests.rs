//! The toolbar in Dev Mode: the pill slides to its own row of tools and shrinks round the mode
//! switcher; its buttons (Move, Copy colors, Measurement, Annotation, Comment) do their jobs; the
//! shape tools' flyout ends with Image/video….

use egui::{Event, Modifiers, PointerButton, Pos2};
use figcraft_engine::{Session, ToolId};
use serde_json::json;

use crate::layers_drag_tests::Ui;
use crate::toolbar::{anim_state, bar_width, dev_cell_centre, pill_rect, shape_flyout_points};
use crate::{RightTab, canvas};

fn click(u: &mut Ui, p: Pos2) {
    for pressed in [true, false] {
        u.frame(vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    }
    u.frame(vec![]);
}

fn pill(u: &Ui) -> egui::Rect {
    pill_rect(u.app.canvas.rect, bar_width(&u.app))
}

/// A file in Dev Mode, with the animation run out.
fn dev() -> Ui {
    let mut u = Ui::new(Session::new());
    u.app.ui.right_tab = RightTab::Inspect;
    u.frames(80);
    u
}

impl Ui {
    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }
}

#[test]
fn switching_to_dev_mode_slides_the_rows_then_shrinks_the_pill() {
    let mut u = Ui::new(Session::new());
    u.frames(5);
    let (slide, grow, design) = anim_state(&u.ctx).expect("drawn");
    assert_eq!((slide, grow), (0.0, 0.0));
    u.app.ui.right_tab = RightTab::Inspect;
    // A few frames in: the rows are on their way, the pill hasn't started to shrink.
    u.frames(4);
    let (slide, grow, width) = anim_state(&u.ctx).expect("drawn");
    assert!(slide > 0.1 && slide < 1.0, "slide {slide}");
    assert_eq!((grow, width), (0.0, design));
    // Halfway the rows are in and the pill is shrinking.
    u.frames(26);
    let (slide, grow, width) = anim_state(&u.ctx).expect("drawn");
    assert_eq!(slide, 1.0);
    assert!(grow > 0.0 && grow < 1.0 && width < design && width > 347.0, "grow {grow}, width {width}");
    // Then it is the Dev Mode pill, 347 wide.
    u.frames(60);
    assert_eq!(anim_state(&u.ctx), Some((1.0, 1.0, 347.0)));
    // And back.
    u.app.ui.right_tab = RightTab::Design;
    u.frames(80);
    assert_eq!(anim_state(&u.ctx), Some((0.0, 0.0, design)));
}

#[test]
fn the_slide_ends_before_the_pill_does() {
    assert_eq!(crate::toolbar::mode_progress_for_tests(0.0), (0.0, 0.0));
    assert_eq!(crate::toolbar::mode_progress_for_tests(1.0), (1.0, 1.0));
    let (slide, grow) = crate::toolbar::mode_progress_for_tests(0.5);
    assert_eq!(slide, 1.0);
    assert!(grow < 1.0);
}

#[test]
fn measurement_holds_alt_for_the_pointer_in_dev_mode_only() {
    let mut u = dev();
    let at = egui::pos2(700.0, 400.0);
    let ev = |u: &Ui| canvas::pointer_event(&u.app, figcraft_engine::PointerKind::Move, at, &Modifiers::NONE);
    assert!(!ev(&u).alt);
    u.app.ui.dev.measure_on = true;
    assert!(ev(&u).alt);
    u.app.ui.right_tab = RightTab::Design;
    assert!(!ev(&u).alt);
}

#[test]
fn the_dev_buttons_measure_copy_colors_annotate_and_comment() {
    let mut u = dev();
    let p = pill(&u);
    assert_eq!(p.width(), 347.0);
    // Measurement toggles.
    click(&mut u, dev_cell_centre(p, 2));
    assert!(u.app.ui.dev.measure_on);
    click(&mut u, dev_cell_centre(p, 2));
    assert!(!u.app.ui.dev.measure_on);
    // Copy colors starts the sampling eyedropper; Move gives it up.
    click(&mut u, dev_cell_centre(p, 1));
    assert!(u.app.ui.dev.copy_colors && u.app.session.tool.pick_only);
    assert_eq!(u.app.session.tool.tool, ToolId::Eyedropper);
    click(&mut u, dev_cell_centre(p, 0));
    assert!(!u.app.ui.dev.copy_colors);
    assert_eq!(u.app.session.tool.tool, ToolId::Move);
    // Comment is a tool.
    click(&mut u, dev_cell_centre(p, 4));
    assert_eq!(u.app.session.tool.tool, ToolId::Comment);
    // Annotation needs a layer.
    click(&mut u, dev_cell_centre(p, 3));
    assert_eq!(u.app.session.message.as_deref(), Some("Select a layer to annotate"));
}

#[test]
fn entering_dev_mode_from_a_drawing_tool_moves_instead() {
    let mut u = Ui::new(Session::new());
    u.app.run("tool.set", json!({"tool": "rectangle"}));
    u.frames(3);
    let p = pill(&u);
    // The mode switcher's Inspect cell is the last of its four at the pill's right end.
    let inspect = Pos2::new(p.max.x - 8.0 - 2.0 - 14.0, p.center().y);
    click(&mut u, inspect);
    assert_eq!(u.app.ui.right_tab, RightTab::Inspect);
    assert_eq!(u.app.session.tool.tool, ToolId::Move);
}

#[test]
fn the_shape_flyout_ends_with_image_video() {
    let mut u = Ui::new(Session::new());
    u.frames(3);
    let (caret, last) = shape_flyout_points(pill(&u));
    click(&mut u, caret);
    assert_eq!(u.app.ui.tool_flyout, Some("shape"));
    assert!(u.app.pending_dialog.is_none());
    // Its last row is Image/video…: it closes the flyout and asks for a file (the test services
    // cancel the dialog on the next frame, so look in the frame of the click).
    for pressed in [true, false] {
        let button = Event::PointerButton { pos: last, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        u.frame(vec![Event::PointerMoved(last), button]);
    }
    assert_eq!(u.app.ui.tool_flyout, None);
    assert!(u.app.pending_dialog.is_some(), "the place-image dialog is open");
    assert_ne!(u.app.session.tool.tool, ToolId::Star);
}

#[test]
fn copy_colors_puts_the_sampled_colour_on_the_clipboard() {
    let mut u = dev();
    let p = pill(&u);
    click(&mut u, dev_cell_centre(p, 1));
    assert!(u.app.ui.dev.copy_colors);
    // The eyedropper's click: it samples, then gives way to Move.
    u.app.session.tool.picked = Some(figcraft_color::Color::from_rgba8(255, 128, 0, 255));
    u.app.run("tool.move", json!({}));
    let out = u.frame_output(vec![]);
    assert_eq!(u.app.session.message.as_deref().map(str::to_uppercase), Some("COPIED #FF8000".to_string()));
    assert!(!u.app.ui.dev.copy_colors && u.app.session.tool.picked.is_none());
    let copied =
        out.platform_output.commands.iter().any(|c| matches!(c, egui::output::OutputCommand::CopyText(t) if t.eq_ignore_ascii_case("#FF8000")));
    assert!(copied, "{:?}", out.platform_output.commands);
}
