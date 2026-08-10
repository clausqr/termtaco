//! Angle/aspect geometry: mapping values to positions on the dial's 270° arc.
//!
//! The dial is drawn in a fixed 1:1 (round) aspect: terminal cells are about
//! twice as tall as they are wide, so the canvas bounds are widened on the
//! longer axis to keep the circle from rendering as an ellipse.

use std::f64::consts::PI;

use ratatui::layout::Rect;

/// Math-convention angles (0° = +x, CCW positive). 225° is the scale minimum
/// (lower-left), −45° the scale maximum (lower-right); the midpoint of the
/// 270° sweep between them points straight up.
pub(super) const THETA_MIN: f64 = 5.0 * PI / 4.0;
pub(super) const THETA_MAX: f64 = -PI / 4.0;

/// Arc radius in dial units (the round `[-1, 1]` disc).
pub(super) const R_ARC: f64 = 0.85;

/// Terminal cell height-to-width ratio. Cells are ~2× taller than wide; this
/// is what we compensate for to keep the dial round.
pub(super) const CELL_ASPECT: f64 = 2.0;

/// Symmetric canvas half-extents that render the dial as a true circle for the
/// given draw area. The shorter visual axis is normalized to `1.0`; the longer
/// one is widened proportionally so a unit circle is not squashed.
pub(super) fn aspect_bounds(area: Rect) -> (f64, f64) {
    // Inner area excludes the 1-cell block border on each side.
    let w = area.width.saturating_sub(2).max(1) as f64; // cells wide
    let h = area.height.saturating_sub(2).max(1) as f64; // cells tall
    let visual_w = w; // 1 unit per cell width
    let visual_h = h * CELL_ASPECT; // cells are taller
    let side = visual_w.min(visual_h);
    (visual_w / side, visual_h / side)
}

/// Map a value in `[lo, hi]` to its angle on the arc (radians), clamped.
pub(super) fn value_to_angle(v: f64, lo: f64, hi: f64) -> f64 {
    let span = (hi - lo).max(f64::EPSILON);
    let t = ((v - lo) / span).clamp(0.0, 1.0);
    THETA_MIN + t * (THETA_MAX - THETA_MIN)
}

/// Angle (radians) for a normalized position `t` in `[0, 1]` along the sweep.
pub(super) fn frac_to_angle(t: f64) -> f64 {
    THETA_MIN + t * (THETA_MAX - THETA_MIN)
}

pub(super) fn polar(r: f64, theta: f64) -> (f64, f64) {
    (r * theta.cos(), r * theta.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    #[test]
    fn endpoints_map_to_sweep_extremes() {
        approx(value_to_angle(0.0, 0.0, 10.0), THETA_MIN);
        approx(value_to_angle(10.0, 0.0, 10.0), THETA_MAX);
    }

    #[test]
    fn midpoint_points_straight_up() {
        // Halfway through a 225°→−45° sweep is 90° (straight up).
        approx(value_to_angle(5.0, 0.0, 10.0), PI / 2.0);
    }

    #[test]
    fn out_of_range_clamps() {
        approx(value_to_angle(-5.0, 0.0, 10.0), THETA_MIN);
        approx(value_to_angle(99.0, 0.0, 10.0), THETA_MAX);
    }

    #[test]
    fn aspect_makes_a_round_dial() {
        // Wide pane: x half-extent widens, y stays 1.0 (y is the limiting axis).
        let (hx, hy) = aspect_bounds(Rect::new(0, 0, 70, 30));
        assert!(hx > 1.0 && (hy - 1.0).abs() < 1e-9);
        // Tall-narrow pane: roles swap.
        let (hx2, hy2) = aspect_bounds(Rect::new(0, 0, 20, 40));
        assert!(hy2 > 1.0 && (hx2 - 1.0).abs() < 1e-9);
        // Roundness invariant: half_y/half_x == CELL_ASPECT * H / W.
        let (w, h) = (68.0, 28.0);
        let (hx3, hy3) = aspect_bounds(Rect::new(0, 0, 70, 30));
        approx(hy3 / hx3, CELL_ASPECT * h / w);
    }
}
