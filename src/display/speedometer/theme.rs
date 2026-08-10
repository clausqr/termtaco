//! Color palette for the speedometer dial.

use ratatui::style::Color;

/// Color palette for the dial. Swap `Theme::default` (or add a constructor) to
/// re-theme everything in one place.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub arc: Color,
    pub tick_minor: Color,
    pub tick_major: Color,
    pub tick_label: Color,
    pub needle: Color,
    /// The bold tick marking the latest raw, unfiltered sample.
    pub raw: Color,
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
            raw: Color::White,
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
