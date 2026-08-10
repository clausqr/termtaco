//! Minimal config file support: `~/.config/termtaco/config`, currently just a
//! `theme = NAME` line. No config-parsing dependency — the format is
//! deliberately tiny (`key = value` per line, `#` comments, blank lines
//! ignored) since `theme` is the only setting it needs to carry today.

use std::fs;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/termtaco/config`, or `$HOME/.config/termtaco/config` when
/// `XDG_CONFIG_HOME` isn't set. `None` if neither env var is available.
fn config_path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("termtaco").join("config"))
}

/// The value of the `theme` key in the config file, e.g. `Some("color")` for
/// a file containing `theme = color`. `None` if the file, key, or config
/// directory is missing — callers fall back to the default theme.
pub fn theme_name() -> Option<String> {
    let path = config_path()?;
    theme_name_from(&fs::read_to_string(path).ok()?)
}

/// Parses `theme_name`'s file format from an already-read string, so the
/// parsing logic is testable without touching the filesystem.
fn theme_name_from(contents: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (key, value) = line.split_once('=')?;
        (key.trim() == "theme").then(|| value.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_theme_key() {
        assert_eq!(theme_name_from("theme = color"), Some("color".to_string()));
        assert_eq!(theme_name_from("theme=color"), Some("color".to_string()));
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let src = "# a comment\n\ntheme = color\n";
        assert_eq!(theme_name_from(src), Some("color".to_string()));
    }

    #[test]
    fn missing_key_is_none() {
        assert_eq!(theme_name_from(""), None);
        assert_eq!(theme_name_from("# nothing here\n"), None);
        assert_eq!(theme_name_from("other = value\n"), None);
    }

    #[test]
    fn takes_first_match() {
        let src = "theme = color\ntheme = bw\n";
        assert_eq!(theme_name_from(src), Some("color".to_string()));
    }
}
