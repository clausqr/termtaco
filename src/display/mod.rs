//! Display layer: the plugin boundary. A [`Display`] turns a [`Reading`] into
//! pixels on a ratatui [`Frame`]. New visualizations implement this trait
//! and register a constructor in [`make`]; the infra layer is untouched.

use ratatui::{layout::Rect, Frame};

use crate::math::stats::Stats;

pub mod speedometer;

/// Everything that changes frame to frame: the pure window statistics plus the
/// two derived values the window itself cannot compute. Assembled at the
/// infra/display boundary (`infra::app::run`) from a [`crate::infra::feed::Feed`]
/// snapshot and handed to [`Display::render`] as one argument.
///
/// `Copy` and small, so passing it per frame costs nothing — and keeping
/// `smoothed`/`stale` here rather than as fields on `Stats` or as `&mut self`
/// setters is what lets [`crate::math::stats::Window`] stay honestly pure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// Pure window statistics, exactly as `Window::stats()` computed them.
    pub stats: Stats,
    /// `stats.last` after the caller's smoothing filter, or `stats.last`
    /// unchanged when smoothing is off. The needle and the big value label
    /// track this; the bold raw tick tracks `stats.last`, so the two visibly
    /// diverge under `--kalman`.
    pub smoothed: f64,
    /// The feed has gone quiet: the reading is frozen, not live.
    pub stale: bool,
}

/// A renderer for a stats snapshot. Takes `&mut self` so a renderer can carry
/// state across frames (e.g. a held scale or an overflow timer).
pub trait Display {
    /// Draw the current reading into `area` of `frame`.
    fn render(&mut self, frame: &mut Frame, area: Rect, reading: &Reading);
}

/// Construction-time configuration for a renderer, assembled once from the CLI
/// and handed to [`make`].
///
/// Deliberately a plain data bag covering the union of every renderer's knobs:
/// each renderer reads the fields it understands and ignores the rest. That
/// keeps renderer-specific concepts (needle inertia, overflow hold) out of the
/// [`Display`] trait, so adding a knob for one renderer doesn't grow the API
/// surface every other renderer has to inherit and no-op.
///
/// Per-frame data (the current reading, staleness) does not belong here — it
/// travels through [`Display::render`].
pub struct DisplayConfig {
    /// Title drawn on the display face. Empty is treated as absent.
    pub title: Option<String>,
    /// Text shown in the display's border.
    pub border_label: String,
    /// Always keep 0 in the scale, even if the data never reaches it.
    pub include_zero: bool,
    /// How long to hold a capped/overflow reading before rescaling to fit.
    pub overflow_hold: std::time::Duration,
    /// Time constant over which the displayed max decays toward
    /// `max_decay_target × mean`. `None` disables decay.
    pub max_decay: Option<std::time::Duration>,
    /// Equilibrium multiplier of the mean the decaying max settles toward.
    pub max_decay_target: f64,
    /// Time constant for a needle with mass. `None` snaps immediately.
    pub needle_inertia: Option<std::time::Duration>,
    /// Theme name from the theme file (see [`crate::infra::config`]), e.g.
    /// `"color"`. `None`, or a name the renderer doesn't recognize, falls
    /// back to that renderer's default theme.
    pub theme: Option<String>,
}

impl Default for DisplayConfig {
    /// Every effect off, matching the CLI's own defaults; the two non-trivial
    /// values are cited from the speedometer rather than duplicated.
    fn default() -> Self {
        DisplayConfig {
            title: None,
            border_label: String::new(),
            include_zero: false,
            overflow_hold: speedometer::DEFAULT_OVERFLOW_HOLD,
            max_decay: None,
            max_decay_target: speedometer::DEFAULT_MAX_DECAY_TARGET,
            needle_inertia: None,
            theme: None,
        }
    }
}

/// Construct a display by name. Returns `None` for an unknown name so the
/// caller can report the available options.
pub fn make(name: &str, cfg: &DisplayConfig) -> Option<Box<dyn Display>> {
    match name {
        "speedometer" | "dial" | "gauge" => Some(Box::new(speedometer::Speedometer::new(cfg))),
        _ => None,
    }
}

/// Names accepted by [`make`], for help/error messages.
pub const AVAILABLE: &[&str] = &["speedometer"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_accepts_the_documented_names_and_rejects_others() {
        let cfg = DisplayConfig::default();
        for name in ["speedometer", "dial", "gauge"] {
            assert!(make(name, &cfg).is_some(), "{name} should be a known display");
        }
        assert!(make("sparkline", &cfg).is_none());
        assert!(make("", &cfg).is_none());
        for name in AVAILABLE {
            assert!(make(name, &cfg).is_some(), "AVAILABLE lists {name} but make rejects it");
        }
    }
}
