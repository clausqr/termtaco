//! Locates and reads the theme file: `~/.config/termtaco/theme`. Parsing its
//! `key = color` lines into a [`crate::display::speedometer::Theme`] is the
//! theme module's job — this is just the filesystem lookup.

use std::fs;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/termtaco/theme`, or `$HOME/.config/termtaco/theme` when
/// `XDG_CONFIG_HOME` isn't set. `None` if neither env var is available.
fn theme_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("termtaco").join("theme"))
}

/// The raw content of the theme file, unparsed. `None` if it doesn't exist —
/// callers fall back to the default theme.
pub fn theme_file() -> Option<String> {
    fs::read_to_string(theme_path()?).ok()
}
