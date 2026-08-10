//! Radial dial / speedometer renderer.
//!
//! A 270° sweep: the minimum sits at the lower-left (225°), the maximum at the
//! lower-right (−45°), and the midpoint points straight up. The needle tracks
//! the latest (optionally Kalman-smoothed, optionally inertia-damped) value;
//! graduation ticks (with numbers at the majors) mark the scale, colored ticks
//! annotate window min/max, the mean, and the ±1σ band, and a bold tick marks
//! the latest raw, unfiltered sample.
//!
//! All colors come from a [`Theme`], so the palette can be swapped in one
//! place. Split across five sibling modules: [`geometry`] (pure angle/aspect
//! math), [`scale`] (pure auto-scale selection), [`state`] (the two
//! cross-frame timed state machines — overflow-hold and decaying-max — plus
//! needle-inertia dispatch), [`theme`] (the color palette), and [`draw`] (the
//! drawing routines). This file is just the `Speedometer` struct and its
//! [`Display`] impl, wiring the pieces together each frame.

mod draw;
mod geometry;
mod scale;
mod state;
mod theme;

pub use state::{DEFAULT_MAX_DECAY_TARGET, DEFAULT_OVERFLOW_HOLD};
pub use theme::{preset_content, Theme, PRESET_NAMES};

use std::time::{Duration, Instant};

use ratatui::{
    layout::Rect,
    widgets::{canvas::Canvas, Block, Borders},
    Frame,
};

use crate::display::{Display, DisplayConfig, Reading};
use crate::math::needle::Needle;

pub struct Speedometer {
    theme: Theme,
    /// Optional title shown at the top of the dial (set via `--title`).
    title: Option<String>,
    /// Text shown in the dial's border (set via `--border-label`).
    border_label: String,
    /// Always keep 0 in the scale (set via `--0`), e.g. for a speedometer.
    include_zero: bool,
    /// How long to hold the capped/overflow state before rescaling.
    overflow_hold: Duration,
    /// Time constant for decaying the max tick toward `max_decay_target ×
    /// mean`. `None` (the default) holds the max until it ages out of the
    /// window, same as before this feature existed.
    max_decay: Option<Duration>,
    /// Equilibrium multiplier of the mean the decaying max settles toward.
    max_decay_target: f64,
    /// Held scale + the overflow-hold timer.
    scale: state::ScaleState,
    /// The decaying max's own state.
    decay: state::MaxDecay,
    /// Needle-inertia dispatch + its own clock.
    needle: state::NeedleState,
}

impl Speedometer {
    /// Build a dial from the shared display configuration, reading only the
    /// fields a radial gauge understands. The empty title is normalized to
    /// `None` here so `--title=""` draws no title, same as omitting the flag.
    pub fn new(cfg: &DisplayConfig) -> Self {
        Speedometer {
            theme: cfg.theme_file.as_deref().map(Theme::from_file).unwrap_or_default(),
            title: cfg.title.clone().filter(|t| !t.is_empty()),
            border_label: cfg.border_label.clone(),
            include_zero: cfg.include_zero,
            overflow_hold: cfg.overflow_hold,
            max_decay: cfg.max_decay,
            max_decay_target: cfg.max_decay_target,
            scale: state::ScaleState::default(),
            decay: state::MaxDecay::default(),
            needle: state::NeedleState {
                pointer: cfg.needle_inertia.map(Needle::new),
                last_tick: None,
            },
        }
    }
}

impl Default for Speedometer {
    fn default() -> Self {
        Self::new(&DisplayConfig::default())
    }
}

impl Display for Speedometer {
    fn render(&mut self, frame: &mut Frame, area: Rect, reading: &Reading) {
        self.render_at(frame, area, reading, Instant::now());
    }
}

impl Speedometer {
    /// Same as [`Display::render`], but takes the current instant explicitly
    /// rather than reading the clock itself — this is what lets the
    /// overflow-hold and max-decay timed transitions (both in [`state`]) be
    /// unit tested deterministically. The trait's `render` is a one-line
    /// call to this with `Instant::now()`.
    fn render_at(&mut self, frame: &mut Frame, area: Rect, reading: &Reading, now: Instant) {
        let stats = reading.stats;
        let smoothed = reading.smoothed;
        let stale = reading.stale;
        let theme = self.theme;
        let title = self.title.clone();
        let border_label = self.border_label.clone();

        // Target scale that would fit the current window; the held scale and
        // the overflow-hold-then-rescale logic live in `ScaleState`.
        let target = scale::nice_scale(&stats, self.include_zero);
        let ((lo, hi, step), overflow) = self.scale.step(target, smoothed, self.overflow_hold, now);
        let (half_x, half_y) = geometry::aspect_bounds(area);

        // Max tick: held rigidly at the window's raw max by default, or
        // exponentially decayed toward `max_decay_target × mean` once armed
        // via `--max-decay` (see `MaxDecay`), snapping back up instantly on a
        // fresh extreme.
        let display_max = self.decay.step(&stats, self.max_decay, self.max_decay_target, now);

        // Needle position: the reading itself by default, or — with
        // --needle-inertia — a critically-damped mass chasing that reading
        // (see `NeedleState`), so the pointer lags and settles like a real
        // gauge instead of teleporting. The raw tick and the value label are
        // unaffected; only the pointer has mass.
        let needle_value = self.needle.step(smoothed, now);

        // Stale greys the needle (the marker hand) and lights the yellow stale
        // LED. Otherwise red is the alarm (overflow): the needle and value join
        // the LED in red; everything else stays white.
        let needle_color = if stale {
            theme.stats
        } else if overflow {
            theme.alarm
        } else {
            theme.needle
        };
        let value_color = if overflow { theme.alarm } else { theme.value };
        // The raw tick is a liveness indicator like the needle (greys when
        // stale), but doesn't join the overflow alarm — it just marks where
        // the last real sample landed, whether or not that's off-scale.
        let raw_color = if stale { theme.stats } else { theme.raw };

        let block = if border_label.is_empty() {
            Block::default().borders(Borders::ALL)
        } else {
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {border_label} "))
        };
        let canvas = Canvas::default()
            .block(block)
            .marker(ratatui::symbols::Marker::Braille)
            // Symmetric bounds keep the dial centered; the longer visual axis
            // gets a half-extent > 1.0 so a unit circle stays round.
            .x_bounds([-half_x, half_x])
            .y_bounds([-half_y, half_y])
            .paint(move |ctx| {
                draw::draw_arc(ctx, &theme);
                draw::draw_scale_ticks(ctx, lo, hi, step, &theme);
                draw::draw_tick_numbers(ctx, lo, hi, step, &theme);
                draw::draw_markers(ctx, &theme);
                draw::draw_stat_ticks(ctx, &stats, display_max, lo, hi, &theme);
                draw::draw_raw_tick(ctx, stats.last, lo, hi, raw_color);
                draw::draw_needle(ctx, needle_value, lo, hi, needle_color);
                draw::draw_labels(ctx, smoothed, step, value_color);
                draw::draw_title(ctx, title.as_deref(), &theme);
                draw::draw_led(ctx, overflow, &theme);
                draw::draw_stale(ctx, stale, &theme);
            });

        frame.render_widget(canvas, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::stats::Stats;

    fn stats(last: f64, min: f64, max: f64) -> Stats {
        Stats { last, min, max, mean: (min + max) / 2.0, stddev: 1.0, count: 200 }
    }

    /// A live reading whose smoothed value equals the raw sample — the common case.
    fn reading(last: f64, min: f64, max: f64) -> Reading {
        Reading { stats: stats(last, min, max), smoothed: last, stale: false }
    }

    /// Raw sample and smoothed value deliberately split, for the tests that pin
    /// which of the two each dial element tracks.
    fn reading_split(last: f64, smoothed: f64, min: f64, max: f64) -> Reading {
        Reading { stats: stats(last, min, max), smoothed, stale: false }
    }

    fn render_text(display: &mut Speedometer, r: &Reading) -> String {
        use ratatui::{backend::TestBackend, Terminal};
        let mut terminal = Terminal::new(TestBackend::new(70, 30)).unwrap();
        terminal.draw(|f| display.render(f, f.size(), r)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol.as_str())
            .collect()
    }

    #[test]
    fn renders_to_buffer_without_panic() {
        let mut display = Speedometer::default();
        let text = render_text(&mut display, &reading(33.7, 30.0, 42.0));
        assert!(text.contains("33.70"), "current value label missing");
        // Scale snaps to 30..45 with major numbers 30/35/40/45.
        assert!(text.contains("45") && text.contains("40"), "tick numbers missing");
        // The min/max/mean/sd text block and the sample count are gone (stats
        // live on the arc; no n-count corner label).
        assert!(!text.contains("mean") && !text.contains("sd "), "stats block should be removed");
        assert!(!text.contains("n 200"), "sample count should be removed");
    }

    #[test]
    fn value_label_gets_more_decimals_at_small_scale() {
        // Same magnitude as scale::tests::nice_scale_rounds_to_fives_at_magnitude:
        // min/max of 0.0005/0.0023 snaps to a step of 0.0005 (4 decimals).
        // A flat {:.2} would truncate 0.0015 to "0.00"; the label should show
        // as much precision as the tick labels do.
        let mut display = Speedometer::default();
        let text = render_text(&mut display, &reading(0.0015, 0.0005, 0.0023));
        assert!(text.contains("0.0015"), "value label should show 0.0015 at this scale, got: {text}");
        assert!(!text.contains("0.00 "), "value label should not be truncated to 0.00");
    }

    #[test]
    fn value_label_keeps_a_two_decimal_floor_at_coarse_scale() {
        // step=5 here (see nice_scale_rounds_to_fives_at_magnitude): decimals_for
        // alone would give 0 decimals, rounding 33.7 to "34". The 2-decimal
        // floor keeps the reading's actual fractional detail visible.
        let mut display = Speedometer::default();
        let text = render_text(&mut display, &reading(33.7, 30.0, 42.0));
        assert!(text.contains("33.70"), "value label should keep at least 2 decimals, got: {text}");
    }

    #[test]
    fn border_label_defaults_off_and_can_be_set() {
        let mut off = Speedometer::default();
        assert!(!render_text(&mut off, &reading(33.0, 30.0, 42.0)).contains("RPM"));
        let mut on = Speedometer::new(&DisplayConfig { border_label: "RPM".to_string(), ..Default::default() });
        assert!(render_text(&mut on, &reading(33.0, 30.0, 42.0)).contains("RPM"));
    }

    #[test]
    fn title_is_rendered() {
        let mut display =
            Speedometer::new(&DisplayConfig { title: Some("THROUGHPUT".to_string()), ..Default::default() });
        let text = render_text(&mut display, &reading(33.0, 30.0, 42.0));
        assert!(text.contains("THROUGHPUT"), "title should be drawn");
    }

    #[test]
    fn overflow_caps_and_lights_led() {
        let mut display = Speedometer::default();
        // Establish a scale of 30..45.
        let _ = render_text(&mut display, &reading(33.0, 30.0, 42.0));
        // A value past full scale lights the overflow LED (held, not rescaled).
        let text = render_text(&mut display, &reading(99.0, 30.0, 99.0));
        assert!(text.contains("OVF"), "overflow LED should be lit");
        assert!(display.scale.overflow_since.is_some(), "overflow timer should be armed");
        assert_eq!(display.scale.current.unwrap().1, 45.0, "scale should still be held at 45");
    }

    #[test]
    fn stale_shows_banner() {
        let mut display = Speedometer::default();
        let text = render_text(&mut display, &Reading { stale: true, ..reading(33.0, 30.0, 42.0) });
        assert!(text.contains("STALE"), "stale banner should be drawn");
    }

    #[test]
    fn raw_tick_tracks_the_raw_sample() {
        // Same smoothed value (so the needle/value label are identical), but a
        // different raw last — the raw tick should still make the two frames
        // differ.
        let mut display = Speedometer::default();
        let a = render_text(&mut display, &reading_split(31.0, 36.0, 30.0, 42.0));
        let mut display2 = Speedometer::default();
        let b = render_text(&mut display2, &reading_split(41.0, 36.0, 30.0, 42.0));
        assert_ne!(a, b, "raw tick should move with the raw sample independent of the smoothed reading");
    }

    #[test]
    fn value_label_shows_the_smoothed_reading() {
        let mut display = Speedometer::default();
        let text = render_text(&mut display, &reading_split(40.0, 32.0, 30.0, 45.0));
        assert!(text.contains("32.00"), "value label should show the smoothed reading");
        assert!(!text.contains("40.00"), "value label should not show the raw reading");
    }

    #[test]
    fn needle_inertia_defaults_off() {
        let display = Speedometer::default();
        assert!(display.needle.pointer.is_none(), "needle inertia should be off by default");
    }

    #[test]
    fn needle_inertia_first_frame_starts_at_the_reading() {
        let mut display = Speedometer::new(&DisplayConfig {
            needle_inertia: Some(Duration::from_millis(250)),
            ..Default::default()
        });
        let _ = render_text(&mut display, &reading(33.0, 30.0, 42.0));
        assert!(display.needle.last_tick.is_some(), "needle clock should be armed after the first frame");
    }

    #[test]
    fn config_maps_onto_the_dial_fields() {
        let cfg = DisplayConfig {
            title: Some("RPM".to_string()),
            border_label: "engine".to_string(),
            include_zero: true,
            overflow_hold: Duration::from_millis(250),
            max_decay: Some(Duration::from_secs(7)),
            max_decay_target: 3.5,
            needle_inertia: Some(Duration::from_millis(120)),
            theme_file: Some("arc = cyan\n".to_string()),
        };
        let d = Speedometer::new(&cfg);
        assert_eq!(d.title.as_deref(), Some("RPM"));
        assert_eq!(d.border_label, "engine");
        assert!(d.include_zero);
        assert_eq!(d.overflow_hold, Duration::from_millis(250));
        assert_eq!(d.max_decay, Some(Duration::from_secs(7)));
        assert_eq!(d.max_decay_target, 3.5);
        assert!(d.needle.pointer.is_some(), "needle inertia should arm the pointer");
        assert_eq!(d.theme.arc, ratatui::style::Color::Cyan, "theme file's arc override should apply");
    }

    #[test]
    fn missing_theme_file_falls_back_to_default() {
        let cfg = DisplayConfig { theme_file: None, ..Default::default() };
        let d = Speedometer::new(&cfg);
        assert_eq!(d.theme.arc, Theme::default().arc);
    }

    #[test]
    fn empty_title_stays_none() {
        // --title="" should draw no title, the same as omitting the flag — the
        // normalization the deleted set_title used to do.
        let d = Speedometer::new(&DisplayConfig { title: Some(String::new()), ..Default::default() });
        assert!(d.title.is_none());
    }

    #[test]
    fn default_config_preserves_the_dial_defaults() {
        // Guards the real risk of this refactor: DisplayConfig::default() silently
        // changing what the dial does when no flags are given.
        let d = Speedometer::default();
        assert!(d.title.is_none());
        assert!(d.border_label.is_empty());
        assert!(!d.include_zero);
        assert_eq!(d.overflow_hold, DEFAULT_OVERFLOW_HOLD);
        assert_eq!(d.max_decay, None);
        assert_eq!(d.max_decay_target, DEFAULT_MAX_DECAY_TARGET);
        assert!(d.needle.pointer.is_none());
    }
}
