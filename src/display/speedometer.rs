//! Radial dial / speedometer renderer.
//!
//! A 270° sweep: the minimum sits at the lower-left (225°), the maximum at the
//! lower-right (−45°), and the midpoint points straight up. The needle tracks
//! the latest value; graduation ticks (with numbers at the majors) mark the
//! scale, and colored ticks annotate window min/max, the mean, and the ±1σ band.
//!
//! The dial is drawn in a fixed 1:1 (round) aspect: terminal cells are about
//! twice as tall as they are wide, so the canvas bounds are widened on the
//! longer axis to keep the circle from rendering as an ellipse.
//!
//! All colors come from a [`Theme`], so the palette can be swapped in one place.

use std::f64::consts::PI;

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::{
        canvas::{Canvas, Context, Line as CanvasLine, Points},
        Block, Borders,
    },
    Frame,
};

use crate::display::Display;
use crate::math::stats::Stats;

/// Math-convention angles (0° = +x, CCW positive).
const THETA_MIN: f64 = 5.0 * PI / 4.0; // 225° — scale minimum, lower-left
const THETA_MAX: f64 = -PI / 4.0; //       −45° — scale maximum, lower-right
const R_ARC: f64 = 0.85; // arc radius in dial units (the round [-1, 1] disc)
const R_NUM: f64 = R_ARC - 0.17; // radius for the numbers inside the arc

/// Terminal cell height-to-width ratio. Cells are ~2× taller than wide; this
/// is what we compensate for to keep the dial round.
const CELL_ASPECT: f64 = 2.0;

/// Color palette for the dial. Swap `Theme::default` (or add a constructor) to
/// re-theme everything in one place.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub arc: Color,
    pub tick_minor: Color,
    pub tick_major: Color,
    pub tick_label: Color,
    pub needle: Color,
    pub min_max: Color,
    pub mean: Color,
    pub band: Color,
    pub hub: Color,
    pub value: Color,
    pub stats: Color,
    pub marker: Color,
    pub title: Color,
    pub alarm: Color,
    pub led_off: Color,
    pub stale: Color,
}

impl Theme {
    /// Default theme for a dark terminal: the gauge (arc, ticks, numbers,
    /// markers, needle, value, title) is white; `alarm` red is reserved for the
    /// overflow state (LED lit, needle and value turn red).
    pub const fn dark() -> Self {
        Theme {
            arc: Color::White,
            tick_minor: Color::White,
            tick_major: Color::White,
            tick_label: Color::White,
            needle: Color::White,
            min_max: Color::White,
            mean: Color::White,
            band: Color::White,
            hub: Color::White,
            value: Color::White,
            stats: Color::Gray,
            marker: Color::White,
            title: Color::White,
            alarm: Color::LightRed,
            led_off: Color::DarkGray,
            stale: Color::Yellow,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// How long the gauge stays pinned at full scale (overflow LED lit) before it
/// rescales to fit a value that has run past the top of the scale.
const OVERFLOW_HOLD: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Default)]
pub struct Speedometer {
    theme: Theme,
    /// Optional title shown at the top of the dial (set via `--title`).
    title: Option<String>,
    /// Always keep 0 in the scale (set via `--0`), e.g. for a speedometer.
    include_zero: bool,
    /// Feed has gone quiet; the reading is frozen, not live.
    stale: bool,
    /// The currently displayed scale `(lo, hi, step)`, held across frames so an
    /// overflow can pin the needle before the scale follows.
    scale: Option<(f64, f64, f64)>,
    /// When the current run of overflow began (needle past full scale).
    overflow_since: Option<std::time::Instant>,
}

impl Display for Speedometer {
    fn set_title(&mut self, title: String) {
        self.title = if title.is_empty() { None } else { Some(title) };
    }

    fn set_stale(&mut self, stale: bool) {
        self.stale = stale;
    }

    fn set_include_zero(&mut self, v: bool) {
        self.include_zero = v;
    }

    fn render(&mut self, frame: &mut Frame, area: Rect, stats: &Stats) {
        let stats = *stats;
        let theme = self.theme;
        let title = self.title.clone();
        let stale = self.stale;

        // Target scale that would fit the current window.
        let target = nice_scale(&stats, self.include_zero);
        let (_, cur_hi, _) = *self.scale.get_or_insert(target);

        // Overflow = the live value has run past the top of the displayed scale.
        // Hold there (needle capped, LED lit) for OVERFLOW_HOLD, then rescale.
        let overflow = if stats.last > cur_hi {
            let now = std::time::Instant::now();
            let since = *self.overflow_since.get_or_insert(now);
            if now.duration_since(since) >= OVERFLOW_HOLD {
                self.scale = Some(target); // held long enough → rescale to fit
                self.overflow_since = None;
                false
            } else {
                true // still pinned at full scale
            }
        } else {
            // In range: track the target (handles shrinking / downward moves).
            self.overflow_since = None;
            self.scale = Some(target);
            false
        };

        let (lo, hi, step) = self.scale.unwrap();
        let (half_x, half_y) = aspect_bounds(area);

        // Stale dims the needle and value to grey (frozen reading). Otherwise
        // red is reserved for the alarm (overflow): the needle and value join
        // the LED in red; everything else stays white.
        let needle_color = if stale {
            theme.stats
        } else if overflow {
            theme.alarm
        } else {
            theme.needle
        };
        let value_color = if stale {
            theme.stats
        } else if overflow {
            theme.alarm
        } else {
            theme.value
        };

        let canvas = Canvas::default()
            .block(Block::default().borders(Borders::ALL).title(" gauge "))
            .marker(ratatui::symbols::Marker::Braille)
            // Symmetric bounds keep the dial centered; the longer visual axis
            // gets a half-extent > 1.0 so a unit circle stays round.
            .x_bounds([-half_x, half_x])
            .y_bounds([-half_y, half_y])
            .paint(move |ctx| {
                draw_arc(ctx, &theme);
                draw_scale_ticks(ctx, lo, hi, step, &theme);
                draw_tick_numbers(ctx, lo, hi, step, &theme);
                draw_markers(ctx, &theme);
                draw_stat_ticks(ctx, &stats, lo, hi, &theme);
                draw_needle(ctx, stats.last, lo, hi, needle_color);
                draw_labels(ctx, &stats, half_x, half_y, value_color, &theme);
                draw_title(ctx, title.as_deref(), &theme);
                draw_led(ctx, overflow, &theme);
                draw_stale(ctx, stale, &theme);
            });

        frame.render_widget(canvas, area);
    }
}

/// Symmetric canvas half-extents that render the dial as a true circle for the
/// given draw area. The shorter visual axis is normalized to `1.0`; the longer
/// one is widened proportionally so a unit circle is not squashed.
fn aspect_bounds(area: Rect) -> (f64, f64) {
    // Inner area excludes the 1-cell block border on each side.
    let w = area.width.saturating_sub(2).max(1) as f64; // cells wide
    let h = area.height.saturating_sub(2).max(1) as f64; // cells tall
    let visual_w = w; // 1 unit per cell width
    let visual_h = h * CELL_ASPECT; // cells are taller
    let side = visual_w.min(visual_h);
    (visual_w / side, visual_h / side)
}

/// Map a value in `[lo, hi]` to its angle on the arc (radians), clamped.
fn value_to_angle(v: f64, lo: f64, hi: f64) -> f64 {
    let span = (hi - lo).max(f64::EPSILON);
    let t = ((v - lo) / span).clamp(0.0, 1.0);
    THETA_MIN + t * (THETA_MAX - THETA_MIN)
}

/// Angle (radians) for a normalized position `t` in `[0, 1]` along the sweep.
fn frac_to_angle(t: f64) -> f64 {
    THETA_MIN + t * (THETA_MAX - THETA_MIN)
}

fn polar(r: f64, theta: f64) -> (f64, f64) {
    (r * theta.cos(), r * theta.sin())
}

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
fn decimals_for(step: f64) -> usize {
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
fn nice_scale(s: &Stats, include_zero: bool) -> (f64, f64, f64) {
    let mut dmin = s.min;
    let mut dmax = s.max;
    if include_zero {
        dmin = dmin.min(0.0);
        dmax = dmax.max(0.0);
    }
    let raw_span = if (dmax - dmin).abs() < 1e-12 {
        dmax.abs().max(1.0) // constant data: scale from the value's magnitude
    } else {
        dmax - dmin
    };
    let step = nice5(raw_span / 4.0);
    let mut lo = (dmin / step).floor() * step;
    let mut hi = (dmax / step).ceil() * step;
    if hi - lo < step {
        lo -= step;
        hi += step;
    }
    (lo, hi, step)
}

/// Draw the arc scale as a chain of short line segments.
fn draw_arc(ctx: &mut Context, theme: &Theme) {
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
fn draw_scale_ticks(ctx: &mut Context, lo: f64, hi: f64, step: f64, theme: &Theme) {
    let minor = step / 5.0;
    let n = ((hi - lo) / minor).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * minor;
        let theta = value_to_angle(v, lo, hi);
        let (r_inner, color) = if k % 5 == 0 {
            (R_ARC - 0.11, theme.tick_major)
        } else {
            (R_ARC - 0.06, theme.tick_minor)
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
fn draw_led(ctx: &mut Context, on: bool, theme: &Theme) {
    const LED_X: f64 = 0.40;
    const LED_Y: f64 = -0.10;
    if on {
        ctx.print(LED_X, LED_Y, Span::styled("● OVF", Style::default().fg(theme.alarm)));
    } else {
        ctx.print(LED_X, LED_Y, Span::styled("●", Style::default().fg(theme.led_off)));
    }
}

/// Optional title across the top of the dial face, centered.
fn draw_title(ctx: &mut Context, title: Option<&str>, theme: &Theme) {
    if let Some(t) = title {
        ctx.print(
            -0.03 * t.len() as f64,
            0.42,
            Span::styled(t.to_string(), Style::default().fg(theme.title)),
        );
    }
}

/// "STALE" banner shown when the feed has gone quiet, just above the value.
fn draw_stale(ctx: &mut Context, stale: bool, theme: &Theme) {
    if stale {
        ctx.print(-0.15, 0.12, Span::styled("STALE", Style::default().fg(theme.stale)));
    }
}

/// Five fixed reference markers at the quarter points of the scale — min (0),
/// quarter, mid, three-quarter, and full (max) — drawn just outside the arc
/// rim so they read regardless of the value graduations inside.
fn draw_markers(ctx: &mut Context, theme: &Theme) {
    for i in 0..=4 {
        let theta = frac_to_angle(i as f64 / 4.0);
        let a = polar(R_ARC + 0.02, theta);
        let b = polar(R_ARC + 0.09, theta);
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
fn draw_tick_numbers(ctx: &mut Context, lo: f64, hi: f64, step: f64, theme: &Theme) {
    let dec = decimals_for(step);
    let n = ((hi - lo) / step).round() as i64;
    for k in 0..=n {
        let v = lo + k as f64 * step;
        let (x, y) = polar(R_NUM, value_to_angle(v, lo, hi));
        let label = format!("{v:.dec$}");
        // Roughly center the text on the tick (print anchors at the left edge).
        ctx.print(
            x - 0.03 * label.len() as f64,
            y - 0.03,
            Span::styled(label, Style::default().fg(theme.tick_label)),
        );
    }
}

/// Draw the needle from the hub to just inside the arc, in `color` (white
/// normally, alarm red on overflow).
fn draw_needle(ctx: &mut Context, v: f64, lo: f64, hi: f64, color: Color) {
    let tip = polar(R_ARC - 0.05, value_to_angle(v, lo, hi));
    ctx.draw(&CanvasLine {
        x1: 0.0,
        y1: 0.0,
        x2: tip.0,
        y2: tip.1,
        color,
    });
}

/// A short radial tick at `value`'s angle, spanning `r_inner..r_outer`.
fn draw_tick(ctx: &mut Context, value: f64, lo: f64, hi: f64, r_inner: f64, r_outer: f64, color: Color) {
    let theta = value_to_angle(value, lo, hi);
    let a = polar(r_inner, theta);
    let b = polar(r_outer, theta);
    ctx.draw(&CanvasLine {
        x1: a.0,
        y1: a.1,
        x2: b.0,
        y2: b.1,
        color,
    });
}

/// Min/max, mean, and the ±1σ band — drawn on top of the graduation ticks.
fn draw_stat_ticks(ctx: &mut Context, s: &Stats, lo: f64, hi: f64, theme: &Theme) {
    draw_tick(ctx, s.min, lo, hi, R_ARC - 0.08, R_ARC + 0.02, theme.min_max);
    draw_tick(ctx, s.max, lo, hi, R_ARC - 0.08, R_ARC + 0.02, theme.min_max);

    let band_lo = (s.mean - s.stddev).clamp(lo, hi);
    let band_hi = (s.mean + s.stddev).clamp(lo, hi);
    draw_tick(ctx, band_lo, lo, hi, R_ARC - 0.06, R_ARC + 0.01, theme.band);
    draw_tick(ctx, band_hi, lo, hi, R_ARC - 0.06, R_ARC + 0.01, theme.band);
    draw_tick(ctx, s.mean, lo, hi, R_ARC - 0.06, R_ARC + 0.01, theme.mean);

    ctx.draw(&Points {
        coords: &[(0.0, 0.0)],
        color: theme.hub,
    });
}

/// The current value under the hub, plus the sample count in the top-right
/// corner. The distributional stats (min, max, mean, ±1σ) are shown as tick
/// marks on the arc, not as a text block.
fn draw_labels(ctx: &mut Context, s: &Stats, half_x: f64, half_y: f64, value_color: Color, theme: &Theme) {
    ctx.print(
        -0.18,
        -0.32,
        Span::styled(format!("{:.2}", s.last), Style::default().fg(value_color)),
    );
    ctx.print(
        half_x - 0.30,
        half_y - 0.08,
        Span::styled(format!("n {}", s.count), Style::default().fg(theme.stats)),
    );
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

    fn stats(last: f64, min: f64, max: f64) -> Stats {
        Stats { last, min, max, mean: (min + max) / 2.0, stddev: 1.0, count: 200 }
    }

    fn render_text(display: &mut Speedometer, s: &Stats) -> String {
        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(70, 30)).unwrap();
        terminal.draw(|f| display.render(f, f.size(), s)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol.as_str())
            .collect()
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
    fn renders_to_buffer_without_panic() {
        let mut display = Speedometer::default();
        let text = render_text(&mut display, &stats(33.7, 30.0, 42.0));
        assert!(text.contains("33.70"), "current value label missing");
        assert!(text.contains("n 200"), "count label missing");
        // Scale snaps to 30..45 with major numbers 30/35/40/45.
        assert!(text.contains("45") && text.contains("40"), "tick numbers missing");
        // The min/max/mean/sd text block is gone (stats live on the arc).
        assert!(!text.contains("mean") && !text.contains("sd "), "stats block should be removed");
    }

    #[test]
    fn title_is_rendered() {
        let mut display = Speedometer::default();
        display.set_title("THROUGHPUT".to_string());
        let text = render_text(&mut display, &stats(33.0, 30.0, 42.0));
        assert!(text.contains("THROUGHPUT"), "title should be drawn");
    }

    #[test]
    fn overflow_caps_and_lights_led() {
        let mut display = Speedometer::default();
        // Establish a scale of 30..45.
        let _ = render_text(&mut display, &stats(33.0, 30.0, 42.0));
        // A value past full scale lights the overflow LED (held, not rescaled).
        let text = render_text(&mut display, &stats(99.0, 30.0, 99.0));
        assert!(text.contains("OVF"), "overflow LED should be lit");
        assert!(display.overflow_since.is_some(), "overflow timer should be armed");
        assert_eq!(display.scale.unwrap().1, 45.0, "scale should still be held at 45");
    }

    #[test]
    fn stale_shows_banner() {
        let mut display = Speedometer::default();
        display.set_stale(true);
        let text = render_text(&mut display, &stats(33.0, 30.0, 42.0));
        assert!(text.contains("STALE"), "stale banner should be drawn");
    }

    #[test]
    fn degenerate_window_gets_a_band() {
        let (lo, hi, _) = nice_scale(&stats(4.0, 4.0, 4.0), false);
        assert!(lo < 4.0 && hi > 4.0, "band should bracket the value");
    }
}
