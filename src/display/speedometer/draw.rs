//! Drawing routines: turns already-resolved geometry (angles, radii, colors)
//! into canvas primitives. No state, no decision logic; everything it needs
//! is passed in.

use ratatui::{
    style::{Color, Style},
    text::Span,
    widgets::canvas::{Context, Line as CanvasLine, Points},
};

use super::geometry::{frac_to_angle, polar, value_to_angle, R_ARC};
use super::scale::decimals_for;
use super::theme::Theme;
use crate::math::stats::Stats;

/// Radius for the tick numbers inside the arc.
const R_NUM: f64 = R_ARC - 0.17;

/// Radial span of the graduation ticks along the arc: major ticks (every
/// `MINORS_PER_MAJOR`-th) reach further in than minor ticks; both run out to
/// the arc itself.
const MAJOR_TICK_R: f64 = R_ARC - 0.11;
const MINOR_TICK_R: f64 = R_ARC - 0.06;

/// How many graduation ticks make up one major interval (i.e. every
/// `MINORS_PER_MAJOR`-th tick is major): one constant sizes the minor tick
/// spacing *and* picks which ticks are major, so the two can't drift apart.
const MINORS_PER_MAJOR: i64 = 5;

/// Radial span of the five fixed quarter-point reference markers, drawn just
/// outside the arc rim.
const MARKER_IN: f64 = R_ARC + 0.02;
const MARKER_OUT: f64 = R_ARC + 0.09;
/// The markers sit at min, quarter, mid, three-quarter, and full scale:
/// `MARKER_COUNT` intervals between the five of them.
const MARKER_COUNT: usize = 4;

/// Radial spans for the three stat-tick tiers (see [`draw_stat_ticks`]), each
/// longer than the last so min/max < band < mean read at a glance even in a
/// single color. `TICK_BAND_IN` numerically coincides with `MAJOR_TICK_R`:
/// unrelated ticks that happen to share a length; kept as separate named
/// constants rather than aliased, since there's no reason a future change to
/// one should move the other.
const TICK_MINMAX_IN: f64 = R_ARC - 0.08;
const TICK_MINMAX_OUT: f64 = R_ARC + 0.02;
const TICK_BAND_IN: f64 = R_ARC - 0.11;
const TICK_BAND_OUT: f64 = R_ARC + 0.05;
const TICK_MEAN_IN: f64 = R_ARC - 0.14;
const TICK_MEAN_OUT: f64 = R_ARC + 0.07;

/// How far short of the arc rim the needle's tip stops.
const NEEDLE_TIP_R: f64 = R_ARC - 0.05;

/// Half a character's width in dial units, used to roughly center printed
/// text (tick numbers, the title) on its anchor point: `ctx.print` anchors
/// at the text's left edge, not its center.
const HALF_CHAR_W: f64 = 0.03;

/// Screen position of the big value label under the hub.
const VALUE_POS: (f64, f64) = (-0.18, -0.32);

/// Screen position of the small last-measurement label, directly under the
/// big value label (see `draw_last_measurement`).
const LAST_MEASUREMENT_POS: (f64, f64) = (-0.18, -0.40);

/// Vertical position of the optional title, across the top of the dial face.
const TITLE_Y: f64 = 0.42;

/// Shared vertical position for both LEDs (overflow lower-right, stale
/// lower-left) so they sit on the same visual baseline; each has its own
/// horizontal position.
const LED_Y: f64 = -0.10;
const OVERFLOW_LED_X: f64 = 0.40;
const STALE_LED_X: f64 = -0.62;

/// Draw the arc scale as a chain of short line segments.
pub(super) fn draw_arc(ctx: &mut Context, theme: &Theme) {
    const N: usize = 120;
    let mut prev = polar(R_ARC, frac_to_angle(0.0));
    for i in 1..=N {
        let p = polar(R_ARC, frac_to_angle(i as f64 / N as f64));
        ctx.draw(&CanvasLine {
            x1: prev.0,
            y1: prev.1,
            x2: p.0,
            y2: p.1,
            color: theme.arc,
        });
        prev = p;
    }
}

/// Graduation ticks at every `step/5` along the scale, with longer brighter
/// major ticks at each multiple of `step` and short minor ticks between.
pub(super) fn draw_scale_ticks(ctx: &mut Context, lo: f64, hi: f64, step: f64, theme: &Theme) {
    let minor = step / MINORS_PER_MAJOR as f64;
    let n = ((hi - lo) / minor).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * minor;
        let theta = value_to_angle(v, lo, hi);
        let (r_inner, color) = if k % MINORS_PER_MAJOR == 0 {
            (MAJOR_TICK_R, theme.tick_major)
        } else {
            (MINOR_TICK_R, theme.tick_minor)
        };
        let a = polar(r_inner, theta);
        let b = polar(R_ARC, theta);
        ctx.draw(&CanvasLine {
            x1: a.0,
            y1: a.1,
            x2: b.0,
            y2: b.1,
            color,
        });
    }
}

/// Overflow LED on the lower-right of the dial face. Lit red with an "OVF" tag
/// while the gauge is pinned at full scale; a dim dot otherwise (an unlit LED).
pub(super) fn draw_led(ctx: &mut Context, on: bool, theme: &Theme) {
    if on {
        ctx.print(OVERFLOW_LED_X, LED_Y, Span::styled("● OVF", Style::default().fg(theme.alarm)));
    } else {
        ctx.print(OVERFLOW_LED_X, LED_Y, Span::styled("●", Style::default().fg(theme.led_off)));
    }
}

/// Optional title across the top of the dial face, centered.
pub(super) fn draw_title(ctx: &mut Context, title: Option<&str>, theme: &Theme) {
    if let Some(t) = title {
        ctx.print(
            -HALF_CHAR_W * t.len() as f64,
            TITLE_Y,
            Span::styled(t.to_string(), Style::default().fg(theme.title)),
        );
    }
}

/// Yellow stale LED on the lower-left of the dial face, lit when the feed has
/// gone quiet (mirrors the red overflow LED on the lower-right).
pub(super) fn draw_stale(ctx: &mut Context, stale: bool, theme: &Theme) {
    if stale {
        ctx.print(STALE_LED_X, LED_Y, Span::styled("● STALE", Style::default().fg(theme.stale)));
    }
}

/// Five fixed reference markers at the quarter points of the scale (min (0),
/// quarter, mid, three-quarter, and full (max)), drawn just outside the arc
/// rim so they read regardless of the value graduations inside.
pub(super) fn draw_markers(ctx: &mut Context, theme: &Theme) {
    for i in 0..=MARKER_COUNT {
        let theta = frac_to_angle(i as f64 / MARKER_COUNT as f64);
        let a = polar(MARKER_IN, theta);
        let b = polar(MARKER_OUT, theta);
        ctx.draw(&CanvasLine {
            x1: a.0,
            y1: a.1,
            x2: b.0,
            y2: b.1,
            color: theme.marker,
        });
    }
}

/// Numeric labels at each major tick (multiple of `step`), centered just
/// inside the arc, printed with just enough decimals for the step.
pub(super) fn draw_tick_numbers(ctx: &mut Context, lo: f64, hi: f64, step: f64, theme: &Theme) {
    let dec = decimals_for(step);
    let n = ((hi - lo) / step).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * step;
        let (x, y) = polar(R_NUM, value_to_angle(v, lo, hi));
        let label = format!("{v:.dec$}");
        // Roughly center the text on the tick (print anchors at the left edge).
        ctx.print(
            x - HALF_CHAR_W * label.len() as f64,
            y - HALF_CHAR_W,
            Span::styled(label, Style::default().fg(theme.tick_label)),
        );
    }
}

/// Radial span of the Kalman-uncertainty corona: a thin ring just outside the
/// arc rim, past the reference markers and stat ticks, rather than a wedge
/// filled in behind the needle.
const COVARIANCE_BAND_IN: f64 = R_ARC + 0.05;
const COVARIANCE_BAND_OUT: f64 = R_ARC + 0.15;

/// How many radial spokes make up the uncertainty corona: dense enough that
/// adjacent spokes' Braille dots touch and it reads as a shaded arc segment
/// rather than a row of individual hairlines.
const COVARIANCE_BAND_SPOKES: usize = 32;

/// Greyed-out corona segment just outside the rim, spanning `center ±
/// half_width` (the Kalman filter's current position estimate and its
/// uncertainty), drawn only with `--kalman`. Literally marks where the true
/// value probably is, distinct from the min/max/mean/±1σ *data* ticks
/// further in: this is the filter's own confidence in `center`, not the
/// window's spread.
pub(super) fn draw_covariance_band(ctx: &mut Context, center: f64, half_width: f64, lo: f64, hi: f64, color: Color) {
    if half_width <= 0.0 {
        return;
    }
    for i in 0..=COVARIANCE_BAND_SPOKES {
        let t = i as f64 / COVARIANCE_BAND_SPOKES as f64;
        let v = (center - half_width + t * 2.0 * half_width).clamp(lo, hi);
        draw_radial(ctx, value_to_angle(v, lo, hi), COVARIANCE_BAND_IN, COVARIANCE_BAND_OUT, 0.0, color);
    }
}

/// A tick through the covariance corona at exactly `center` (the Kalman
/// estimate the band is centered on). Distinct from the needle: with
/// `--needle-inertia` the needle lags behind this, so the two visibly
/// diverge, the same way the raw tick already diverges from both under
/// smoothing.
pub(super) fn draw_kalman_center_tick(ctx: &mut Context, center: f64, lo: f64, hi: f64, color: Color) {
    draw_tick(ctx, center, lo, hi, COVARIANCE_BAND_IN, COVARIANCE_BAND_OUT, color);
}

/// Draw the needle from the hub to just inside the arc, in `color` (white
/// normally, alarm red on overflow).
pub(super) fn draw_needle(ctx: &mut Context, v: f64, lo: f64, hi: f64, color: Color) {
    let tip = polar(NEEDLE_TIP_R, value_to_angle(v, lo, hi));
    ctx.draw(&CanvasLine {
        x1: 0.0,
        y1: 0.0,
        x2: tip.0,
        y2: tip.1,
        color,
    });
}

/// A radial line at angle `theta`, spanning `r_inner..r_outer`, offset `off`
/// canvas units perpendicular to the radius (`0.0` = centered on the radial
/// line). The offset is what lets a tick be drawn several dots wide, see
/// [`draw_raw_tick`]. Bounds are isotropic in canvas units (see
/// [`super::geometry::aspect_bounds`]), so a fixed offset gives a
/// constant-width bar at any radius or pane size.
fn draw_radial(ctx: &mut Context, theta: f64, r_inner: f64, r_outer: f64, off: f64, color: Color) {
    let (nx, ny) = (-theta.sin(), theta.cos()); // unit normal to the radius
    let a = polar(r_inner, theta);
    let b = polar(r_outer, theta);
    ctx.draw(&CanvasLine {
        x1: a.0 + nx * off,
        y1: a.1 + ny * off,
        x2: b.0 + nx * off,
        y2: b.1 + ny * off,
        color,
    });
}

/// A short radial tick at `value`'s angle, spanning `r_inner..r_outer`.
fn draw_tick(ctx: &mut Context, value: f64, lo: f64, hi: f64, r_inner: f64, r_outer: f64, color: Color) {
    draw_radial(ctx, value_to_angle(value, lo, hi), r_inner, r_outer, 0.0, color);
}

/// The latest raw, unfiltered sample: the boldest mark on the rim, the
/// longest radial span, pushed furthest past the arc, and drawn as several
/// parallel lines so it reads as a thick bar rather than a hairline. Where
/// the needle shows the filtered/inertial reading, this is where the last
/// real measurement actually landed, so the two visibly diverge under
/// smoothing (`--kalman`, `--needle-inertia`), and coincide (reading as a
/// fatter needle tip) when neither is enabled.
pub(super) fn draw_raw_tick(ctx: &mut Context, value: f64, lo: f64, hi: f64, color: Color) {
    const R_IN: f64 = R_ARC - 0.13;
    const R_OUT: f64 = R_ARC + 0.10;
    const HALF_WIDTH: i32 = 1; // 3 parallel lines
    const SPACING: f64 = 0.02;
    let theta = value_to_angle(value, lo, hi);
    for k in -HALF_WIDTH..=HALF_WIDTH {
        draw_radial(ctx, theta, R_IN, R_OUT, k as f64 * SPACING, color);
    }
}

/// Min/max, mean, and the ±1σ band, drawn on top of the graduation ticks.
/// `display_max` is `s.max` unless a decaying max (`--max-decay`) has pulled
/// it below the raw window max.
pub(super) fn draw_stat_ticks(ctx: &mut Context, s: &Stats, display_max: f64, lo: f64, hi: f64, theme: &Theme) {
    draw_tick(ctx, s.min, lo, hi, TICK_MINMAX_IN, TICK_MINMAX_OUT, theme.min_max);
    draw_tick(ctx, display_max, lo, hi, TICK_MINMAX_IN, TICK_MINMAX_OUT, theme.min_max);

    // Mean and the ±1σ band are longer and protrude further past the rim than
    // the graduation ticks and the min/max marks, so they read at a glance even
    // when everything is the same color.
    let band_lo = (s.mean - s.stddev).clamp(lo, hi);
    let band_hi = (s.mean + s.stddev).clamp(lo, hi);
    draw_tick(ctx, band_lo, lo, hi, TICK_BAND_IN, TICK_BAND_OUT, theme.band);
    draw_tick(ctx, band_hi, lo, hi, TICK_BAND_IN, TICK_BAND_OUT, theme.band);
    draw_tick(ctx, s.mean, lo, hi, TICK_MEAN_IN, TICK_MEAN_OUT, theme.mean);

    ctx.draw(&Points {
        coords: &[(0.0, 0.0)],
        color: theme.hub,
    });
}

/// The current (smoothed) value under the hub. The distributional stats (min,
/// max, mean, ±1σ) are shown as tick marks on the arc, not as a text block.
/// Uses at least the tick labels' own precision (`decimals_for(step)`) so a
/// sub-millisecond latency dial (small `step`) doesn't get truncated to
/// `0.00`; floored at 2 decimals so a coarse-step dial (e.g. step = 5) still
/// shows the reading's actual fractional detail instead of a rounded whole
/// number.
/// `uncertainty` appends "± N" in a dimmer color when Kalman-smoothing is on
/// and has something to show (`Some` and > 0; a freshly-seeded filter with
/// no uncertainty estimate yet stays silent rather than print a misleading
/// "± 0.00"), from [`crate::math::kalman::Kalman::uncertainty`]: the
/// filter's own confidence in `value`, printed right on the reading it
/// qualifies.
pub(super) fn draw_labels(
    ctx: &mut Context,
    value: f64,
    step: f64,
    uncertainty: Option<f64>,
    value_color: Color,
    uncertainty_color: Color,
) {
    let dec = decimals_for(step).max(2);
    let (x, y) = VALUE_POS;
    ctx.print(x, y, Span::styled(format!("{value:.dec$}"), Style::default().fg(value_color)));
    if let Some(u) = uncertainty.filter(|u| *u > 0.0) {
        let value_text = format!("{value:.dec$}");
        ctx.print(
            x + HALF_CHAR_W * 2.0 * value_text.len() as f64,
            y,
            Span::styled(format!("± {u:.dec$}"), Style::default().fg(uncertainty_color)),
        );
    }
}

/// The last raw measurement, printed under the big value label, only drawn
/// with `--kalman`, where the value label shows the filtered estimate and
/// this shows what the filter actually saw last, so both track the same
/// spot the bold raw tick marks on the arc (see `draw_raw_tick`).
pub(super) fn draw_last_measurement(ctx: &mut Context, value: f64, step: f64, color: Color) {
    let dec = decimals_for(step).max(2);
    let (x, y) = LAST_MEASUREMENT_POS;
    ctx.print(x, y, Span::styled(format!("{value:.dec$}"), Style::default().fg(color)));
}
