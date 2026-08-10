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
    /// markers, needle, value, title) is white; `alarm` red and `stale` yellow
    /// are reserved as semantic accents (overflow, staleness), not decoration.
    pub const fn bw() -> Self {
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

    /// A colorful palette, opt-in via `~/.config/termtaco/theme` (see
    /// [`crate::infra::config`]). Distinct hues per element so the gauge
    /// itself carries more information at a glance.
    pub const fn color() -> Self {
        Theme {
            arc: Color::Cyan,
            tick_minor: Color::DarkGray,
            tick_major: Color::Cyan,
            tick_label: Color::White,
            needle: Color::Yellow,
            raw: Color::LightBlue,
            min_max: Color::Blue,
            mean: Color::Green,
            band: Color::Magenta,
            hub: Color::White,
            value: Color::Green,
            stats: Color::Gray,
            marker: Color::Blue,
            title: Color::Cyan,
            alarm: Color::LightRed,
            led_off: Color::DarkGray,
            stale: Color::Yellow,
        }
    }

    /// Resolve a theme by name (e.g. from the theme file). Unrecognized names
    /// return `None` so the caller can fall back to the default.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "bw" => Some(Self::bw()),
            "color" | "colour" => Some(Self::color()),
            _ => None,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::bw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_name_resolves_known_themes() {
        assert!(matches!(Theme::from_name("bw"), Some(_)));
        assert!(matches!(Theme::from_name("color"), Some(_)));
        assert!(matches!(Theme::from_name("colour"), Some(_)));
    }

    #[test]
    fn from_name_rejects_unknown() {
        assert!(Theme::from_name("neon").is_none());
        assert!(Theme::from_name("").is_none());
    }

    #[test]
    fn default_is_bw() {
        let default = Theme::default();
        let bw = Theme::bw();
        assert_eq!(default.arc, bw.arc);
        assert_eq!(default.needle, bw.needle);
    }
}
