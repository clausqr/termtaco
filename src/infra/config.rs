//! Theme selection: `~/.config/termtaco/theme`, a file whose entire content
//! is the theme name (e.g. `color`). Nothing to parse.

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

/// The trimmed content of the theme file, e.g. `Some("color")`. `None` if the
/// file or its containing directories don't exist, or the file is empty —
/// callers fall back to the default theme.
pub fn theme_name() -> Option<String> {
    theme_name_from(&fs::read_to_string(theme_path()?).ok()?)
}

/// Parses `theme_name`'s file format from an already-read string, so the
/// parsing logic is testable without touching the filesystem or env vars.
fn theme_name_from(contents: &str) -> Option<String> {
    let trimmed = contents.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_surrounding_whitespace_and_newline() {
        assert_eq!(theme_name_from("color\n"), Some("color".to_string()));
        assert_eq!(theme_name_from("  color  \n"), Some("color".to_string()));
    }

    #[test]
    fn empty_or_blank_file_is_none() {
        assert_eq!(theme_name_from(""), None);
        assert_eq!(theme_name_from("\n"), None);
        assert_eq!(theme_name_from("   "), None);
    }
}
