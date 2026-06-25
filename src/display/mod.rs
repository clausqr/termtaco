//! Display layer: the plugin boundary. A [`Display`] turns a [`Stats`] snapshot
//! into pixels on a ratatui [`Frame`]. New visualizations implement this trait
//! and register a constructor in [`make`]; the infra layer is untouched.

use ratatui::{layout::Rect, Frame};

use crate::math::stats::Stats;

pub mod speedometer;

/// A renderer for a stats snapshot. Takes `&mut self` so a renderer can carry
/// state across frames (e.g. a held scale or an overflow timer).
pub trait Display {
    /// Draw the current stats into `area` of `frame`.
    fn render(&mut self, frame: &mut Frame, area: Rect, stats: &Stats);
}

/// Construct a display by name. Returns `None` for an unknown name so the
/// caller can report the available options.
pub fn make(name: &str) -> Option<Box<dyn Display>> {
    match name {
        "speedometer" | "dial" | "gauge" => Some(Box::<speedometer::Speedometer>::default()),
        _ => None,
    }
}

/// Names accepted by [`make`], for help/error messages.
pub const AVAILABLE: &[&str] = &["speedometer"];
