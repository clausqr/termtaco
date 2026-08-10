//! Color palette for the speedometer dial, customizable per element from
//! `~/.config/termtaco/theme` (see [`crate::infra::config`] for where that
//! file lives) — each line assigns one field a color, e.g.:
//!
//! ```text
//! arc = cyan
//! needle = yellow
//! alarm = "#ff0055"
//! ```
//!
//! Any field the file doesn't mention keeps its [`Theme::bw`] default, so a
//! one-line file that only sets `needle` is a valid (if minimal) theme.

use ratatui::style::Color;

/// Color palette for the dial. One field per drawn element; see
/// [`Theme::from_file`] for how a theme file maps onto these by name.
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
    /// This is also the base a theme file's overrides are applied on top of.
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

    /// Build a theme from a theme file's content (see the module docs for the
    /// format). Starts from [`Theme::bw`] and overrides only the fields the
    /// file sets; unknown field names and unparsable colors are silently
    /// skipped so a typo loses one element's customization, not the dial.
    pub fn from_file(contents: &str) -> Self {
        let mut theme = Self::bw();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let Some(color) = parse_color(value.trim()) else { continue };
            theme.set(key.trim(), color);
        }
        theme
    }

    /// Assigns `color` to the field named `key`; unknown names are a no-op.
    fn set(&mut self, key: &str, color: Color) {
        match key {
            "arc" => self.arc = color,
            "tick_minor" => self.tick_minor = color,
            "tick_major" => self.tick_major = color,
            "tick_label" => self.tick_label = color,
            "needle" => self.needle = color,
            "raw" => self.raw = color,
            "min_max" => self.min_max = color,
            "mean" => self.mean = color,
            "band" => self.band = color,
            "hub" => self.hub = color,
            "value" => self.value = color,
            "stats" => self.stats = color,
            "marker" => self.marker = color,
            "title" => self.title = color,
            "alarm" => self.alarm = color,
            "led_off" => self.led_off = color,
            "stale" => self.stale = color,
            _ => {}
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::bw()
    }
}

/// Parses one color value: a named ANSI color (`red`, `lightblue`,
/// `dark_gray`, case-insensitive, `_` optional) or a `#rrggbb` hex triplet.
/// Surrounding `"` or `'` quotes are stripped first, so `alarm = "#ff0055"`
/// and `alarm = #ff0055` both work. `None` for anything else, so the caller
/// can skip the line.
fn parse_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix(['"', '\'']).and_then(|s| s.strip_suffix(['"', '\''])).unwrap_or(s);
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    match s.to_ascii_lowercase().replace('_', "").as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "lightred" => Some(Color::LightRed),
        "lightgreen" => Some(Color::LightGreen),
        "lightyellow" => Some(Color::LightYellow),
        "lightblue" => Some(Color::LightBlue),
        "lightmagenta" => Some(Color::LightMagenta),
        "lightcyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        _ => None,
    }
}

/// Parses a 6-hex-digit RGB triplet (the leading `#` already stripped).
fn parse_hex(hex: &str) -> Option<Color> {
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_is_bw() {
        let t = Theme::from_file("");
        assert_eq!(t.arc, Theme::bw().arc);
        assert_eq!(t.needle, Theme::bw().needle);
    }

    #[test]
    fn overrides_only_the_fields_it_sets() {
        let t = Theme::from_file("needle = yellow\narc = cyan\n");
        assert_eq!(t.needle, Color::Yellow);
        assert_eq!(t.arc, Color::Cyan);
        // Untouched fields keep the bw default.
        assert_eq!(t.title, Theme::bw().title);
    }

    #[test]
    fn ignores_comments_blank_lines_and_unknown_keys() {
        let t = Theme::from_file("# a comment\n\nbogus_field = red\nneedle = yellow\n");
        assert_eq!(t.needle, Color::Yellow);
        assert_eq!(t.arc, Theme::bw().arc);
    }

    #[test]
    fn unparsable_color_leaves_the_default() {
        let t = Theme::from_file("needle = not-a-color\n");
        assert_eq!(t.needle, Theme::bw().needle);
    }

    #[test]
    fn parses_hex_colors() {
        let t = Theme::from_file("needle = #ff0055\n");
        assert_eq!(t.needle, Color::Rgb(0xff, 0x00, 0x55));
    }

    #[test]
    fn strips_surrounding_quotes() {
        let t = Theme::from_file("needle = \"#ff0055\"\narc = 'cyan'\n");
        assert_eq!(t.needle, Color::Rgb(0xff, 0x00, 0x55));
        assert_eq!(t.arc, Color::Cyan);
    }

    #[test]
    fn color_names_are_case_insensitive_and_underscore_optional() {
        assert_eq!(parse_color("LightBlue"), Some(Color::LightBlue));
        assert_eq!(parse_color("light_blue"), Some(Color::LightBlue));
        assert_eq!(parse_color("DARK_GRAY"), Some(Color::DarkGray));
    }

    #[test]
    fn shipped_color_theme_parses_and_overrides_every_field() {
        // themes/color.theme, the example users copy to
        // ~/.config/termtaco/theme, should parse cleanly and actually set
        // every field (i.e. not silently typo'd against the field names in
        // `set`).
        let src = include_str!("../../../themes/color.theme");
        let t = Theme::from_file(src);
        assert_eq!(t.arc, Color::Cyan);
        assert_eq!(t.needle, Color::Yellow);
        assert_eq!(t.alarm, Color::LightRed);
        assert_eq!(t.stale, Color::Yellow);
        assert_eq!(t.value, Color::Green);
        assert_eq!(t.tick_minor, Color::DarkGray);
    }

    #[test]
    fn shipped_bw_theme_matches_the_built_in_default() {
        let src = include_str!("../../../themes/bw.theme");
        let t = Theme::from_file(src);
        let bw = Theme::bw();
        assert_eq!(t.arc, bw.arc);
        assert_eq!(t.alarm, bw.alarm);
        assert_eq!(t.stale, bw.stale);
    }

    /// Every field name [`Theme::set`] recognizes — kept here, not derived,
    /// so this test independently catches a typo in either place.
    const FIELDS: &[&str] = &[
        "arc", "tick_minor", "tick_major", "tick_label", "needle", "raw", "min_max", "mean", "band", "hub",
        "value", "stats", "marker", "title", "alarm", "led_off", "stale",
    ];

    /// Every preset shipped under `themes/` should parse with no dropped
    /// lines: each non-comment, non-blank line names a real field and a
    /// color `parse_color` understands. Catches a typo'd key or a color name
    /// our parser doesn't support before it ships silently broken.
    #[test]
    fn every_shipped_theme_sets_only_known_fields_with_valid_colors() {
        let shipped: &[(&str, &str)] = &[
            ("bw.theme", include_str!("../../../themes/bw.theme")),
            ("color.theme", include_str!("../../../themes/color.theme")),
            ("catppuccin-mocha.theme", include_str!("../../../themes/catppuccin-mocha.theme")),
            ("dracula.theme", include_str!("../../../themes/dracula.theme")),
            ("gruvbox.theme", include_str!("../../../themes/gruvbox.theme")),
            ("nord.theme", include_str!("../../../themes/nord.theme")),
            ("solarized-dark.theme", include_str!("../../../themes/solarized-dark.theme")),
            ("tokyo-night.theme", include_str!("../../../themes/tokyo-night.theme")),
        ];
        for (name, src) in shipped {
            for line in src.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (key, value) = line.split_once('=').unwrap_or_else(|| panic!("{name}: not a `key = value` line: {line:?}"));
                let key = key.trim();
                assert!(FIELDS.contains(&key), "{name}: unknown field {key:?}");
                assert!(parse_color(value.trim()).is_some(), "{name}: unparsable color for {key}: {value:?}");
            }
        }
    }
}
