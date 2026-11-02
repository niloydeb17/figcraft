//! The hatched spacing highlights: their stripes stay inside the band, run at 45 degrees, keep
//! their phase when the band moves, and are cheap for a huge band.

use egui::{Rect, pos2};

use crate::handles::hatch_lines;

#[test]
fn stripes_run_up_to_the_right_inside_the_band() {
    let r = Rect::from_min_max(pos2(10.0, 20.0), pos2(110.0, 40.0));
    let lines = hatch_lines(r, 4.0);
    assert!(lines.len() > 25, "{}", lines.len());
    for [a, b] in &lines {
        assert!(r.expand(0.01).contains(*a) && r.expand(0.01).contains(*b), "{a:?} {b:?}");
        // x + y is one value along a stripe: 45 degrees, rising to the right.
        assert!(((a.x + a.y) - (b.x + b.y)).abs() < 0.01 && b.x > a.x && b.y < a.y);
        // Stripes sit on whole multiples of the spacing.
        assert!(((a.x + a.y) / 4.0).fract().abs() < 1e-3 || ((a.x + a.y) / 4.0).fract() > 0.999);
    }
}

#[test]
fn the_phase_belongs_to_the_screen_not_to_the_band() {
    let a = hatch_lines(Rect::from_min_max(pos2(0.0, 0.0), pos2(40.0, 12.0)), 4.0);
    let b = hatch_lines(Rect::from_min_max(pos2(0.0, 0.0), pos2(80.0, 12.0)), 4.0);
    // The wider band holds the first one's stripes unchanged.
    for line in &a {
        assert!(b.iter().any(|l| (l[0].x + l[0].y - (line[0].x + line[0].y)).abs() < 1e-3));
    }
}

#[test]
fn junk_and_huge_bands_are_safe() {
    assert!(hatch_lines(Rect::from_min_max(pos2(0.0, 0.0), pos2(0.0, 10.0)), 4.0).is_empty());
    assert!(hatch_lines(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0)), 0.0).is_empty());
    assert!(hatch_lines(Rect::from_min_max(pos2(0.0, 0.0), pos2(10.0, 10.0)), f32::NAN).is_empty());
    assert!(hatch_lines(Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(10.0, 10.0)), 4.0).is_empty());
    let huge = hatch_lines(Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0e7, 1.0e7)), 4.0);
    assert!(huge.len() <= 3001, "{}", huge.len());
}
