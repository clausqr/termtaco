//! Auto-scale selection: choosing round tick bounds/step from the window's
//! stats, and how many decimals to print them with.

use crate::math::stats::Stats;

/// Target number of major graduation intervals across the scale's span —
/// `nice_scale` sizes its step so the range divides into roughly this many.
const TARGET_MAJOR_INTERVALS: f64 = 4.0;

/// Relative tolerance for treating a window's max and min as equal (an
/// effectively-constant window), rather than a fixed absolute gap — an
/// absolute threshold would misclassify genuine variation at very small data
/// magnitudes (e.g. sub-nanosecond timings) as constant.
const DEGENERATE_SPAN_EPS: f64 = 1e-9;

/// Round `x` (> 0) to a "nice" number on the `{1, 5, 10} × 10ᵏ` ladder,
/// nearest. This is magnitude-aware: 3 → 5, 8.4 → 10, 0.00045 → 0.0005,
/// 9998 → 10000. Biased to 5 (rather than the usual 1-2-5) so ticks land on
/// round multiples of 5 at the value's own scale.
fn nice5(x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    let exp = x.log10().floor();
    let base = 10f64.powf(exp);
    let frac = x / base; // in [1, 10)
    // Geometric thresholds between 1, 5 and 10: √5 ≈ 2.236, √50 ≈ 7.071.
    let nf = if frac < 2.236_067_977 {
        1.0
    } else if frac < 7.071_067_812 {
        5.0
    } else {
        10.0
    };
    nf * base
}

/// Number of decimal places needed to print multiples of `step` exactly.
pub(super) fn decimals_for(step: f64) -> usize {
    if step >= 1.0 {
        0
    } else {
        (-step.log10().floor()) as usize
    }
}

/// "Nice" auto-scale whose ticks land on round multiples of 5 at the data's
/// own magnitude. Picks a major `step` sized for ~4 intervals over the window
/// range (via [`nice5`]), then floors/ceils the bounds to it. Returns
/// `(lo, hi, step)`. A near-constant window is sized from the value's own
/// magnitude and expanded a step on each side so the needle isn't pinned.
///
/// With `include_zero`, the origin is folded into the range first (so the gauge
/// always shows 0, like a speedometer) and the step is sized for the extended
/// range. Zero stays on the tick grid since it is a multiple of any step.
pub(super) fn nice_scale(s: &Stats, include_zero: bool) -> (f64, f64, f64) {
    let mut dmin = s.min;
    let mut dmax = s.max;
    if include_zero {
        dmin = dmin.min(0.0);
        dmax = dmax.max(0.0);
    }
    let scale_ref = dmax.abs().max(dmin.abs()).max(1.0);
    let raw_span = if (dmax - dmin).abs() <= DEGENERATE_SPAN_EPS * scale_ref {
        dmax.abs().max(1.0) // constant data: scale from the value's magnitude
    } else {
        dmax - dmin
    };
    let step = nice5(raw_span / TARGET_MAJOR_INTERVALS);
    let mut lo = (dmin / step).floor() * step;
    let mut hi = (dmax / step).ceil() * step;
    if hi - lo < step {
        lo -= step;
        hi += step;
    }
    (lo, hi, step)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "expected {b}, got {a}");
    }

    fn stats(last: f64, min: f64, max: f64) -> Stats {
        Stats { last, min, max, mean: (min + max) / 2.0, stddev: 1.0, count: 200 }
    }

    #[test]
    fn nice_scale_rounds_to_fives_at_magnitude() {
        // 18 → 20 at the units scale.
        let (lo, hi, step) = nice_scale(&stats(12.0, 6.0, 18.0), false);
        approx(lo, 5.0);
        approx(hi, 20.0);
        approx(step, 5.0);

        // 49995 → 50000 at the ten-thousands scale.
        let (_, hi2, step2) = nice_scale(&stats(40000.0, 10000.0, 49995.0), false);
        approx(hi2, 50000.0);
        approx(step2, 10000.0);

        // 0.0023 → 0.0025 at the ten-thousandths scale.
        let (_, hi3, step3) = nice_scale(&stats(0.0015, 0.0005, 0.0023), false);
        approx(hi3, 0.0025);
        approx(step3, 0.0005);
    }

    #[test]
    fn include_zero_anchors_scale_at_origin() {
        // Without it, data 30..42 scales to 30..45.
        let (lo, _, _) = nice_scale(&stats(33.0, 30.0, 42.0), false);
        approx(lo, 30.0);
        // With --0, the origin is folded in and the step sizes for 0..42.
        let (lo0, hi0, step0) = nice_scale(&stats(33.0, 30.0, 42.0), true);
        approx(lo0, 0.0);
        approx(hi0, 50.0);
        approx(step0, 10.0);
    }

    #[test]
    fn nice5_examples() {
        approx(nice5(3.0), 5.0);
        approx(nice5(8.4), 10.0);
        approx(nice5(9998.0), 10000.0);
        approx(nice5(0.00045), 0.0005);
    }

    #[test]
    fn degenerate_window_gets_a_band() {
        let (lo, hi, _) = nice_scale(&stats(4.0, 4.0, 4.0), false);
        assert!(lo < 4.0 && hi > 4.0, "band should bracket the value");
    }
}
