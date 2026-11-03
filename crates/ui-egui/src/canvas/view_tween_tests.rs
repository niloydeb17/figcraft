//! Discrete zooms ease from the view they leave to the one they reach; the path keeps the zoom's
//! fixed point still, and scripts and Reduce motion get the view at once.

use figcraft_engine::Session;
use serde_json::json;

use super::view_tween::Look;
use crate::layers_drag_tests::Ui;

fn look(u: &Ui) -> Look {
    let v = &u.app.session.ds().unwrap().view;
    Look { cx: v.cx, cy: v.cy, zoom: v.zoom }
}

#[test]
fn the_path_keeps_the_zooms_fixed_point_still() {
    let (from, to) = (Look { cx: 0.0, cy: 0.0, zoom: 1.0 }, Look { cx: 50.0, cy: 25.0, zoom: 2.0 });
    // Page point (100, 50) sits at the same place on screen in both: 100 right, 50 down of centre.
    let screen = |l: Look| ((100.0 - l.cx) * l.zoom, (50.0 - l.cy) * l.zoom);
    assert_eq!(from.towards(to, 0.0), from);
    let end = from.towards(to, 1.0);
    assert!((end.cx - to.cx).abs() < 1e-9 && (end.cy - to.cy).abs() < 1e-9 && (end.zoom - to.zoom).abs() < 1e-9);
    for e in [0.25, 0.5, 0.75] {
        let mid = from.towards(to, e);
        assert!(mid.zoom > 1.0 && mid.zoom < 2.0, "{e}");
        let (x, y) = screen(mid);
        assert!((x - 100.0).abs() < 1e-6 && (y - 50.0).abs() < 1e-6, "{e}: ({x}, {y})");
    }
    // Junk never reaches the view: the target stands in.
    let junk = Look { cx: f64::NAN, cy: 0.0, zoom: 1.0 };
    assert_eq!(junk.towards(to, 0.5), to);
}

#[test]
fn a_zoom_command_eases_to_its_view_and_lands_on_it() {
    let mut u = Ui::new(Session::new());
    u.app.canvas.smooth_zoom = true;
    u.frame(vec![]);
    let before = look(&u);
    u.app.run("view.zoomIn", json!({}));
    let target = look(&u);
    assert!(target.zoom > before.zoom);
    u.frame(vec![]);
    let first = look(&u);
    assert!((first.zoom - before.zoom).abs() < 1e-9, "it starts where the view was");
    let mut zooms = vec![first.zoom];
    for _ in 0..30 {
        u.frame(vec![]);
        zooms.push(look(&u).zoom);
    }
    assert!(zooms.windows(2).all(|w| w[1] >= w[0]), "it only zooms in: {zooms:?}");
    assert!(zooms.iter().any(|z| *z > before.zoom && *z < target.zoom), "it passes between: {zooms:?}");
    assert_eq!(look(&u), target, "and lands exactly");
}

#[test]
fn reduce_motion_and_harnesses_get_the_view_at_once() {
    for smooth in [false, true] {
        let mut u = Ui::new(Session::new());
        u.app.canvas.smooth_zoom = smooth;
        u.app.session.prefs.reduce_motion = smooth;
        u.frame(vec![]);
        u.app.run("view.zoomIn", json!({}));
        let target = look(&u);
        u.frame(vec![]);
        assert_eq!(look(&u), target, "smooth {smooth}");
    }
}
