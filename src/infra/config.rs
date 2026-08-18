//! Locates and reads config files under `~/.config/termtaco/`: the theme
//! file and named profiles. Parsing their content is each caller's job (the
//! theme module's `key = color` lines, [`crate::infra::profile`]'s
//! `flag = value` lines); this is just the filesystem lookup.

use std::fs;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/termtaco`, or `$HOME/.config/termtaco` when
/// `XDG_CONFIG_HOME` isn't set. `None` if neither env var is available.
fn config_dir() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(base.join("termtaco"))
}

/// The raw content of the theme file (`<config_dir>/theme`), unparsed.
/// `None` if it doesn't exist; callers fall back to the default theme.
pub fn theme_file() -> Option<String> {
    fs::read_to_string(config_dir()?.join("theme")).ok()
}

/// Path to a named profile file: `<config_dir>/profiles/NAME`. Whether it
/// exists is the caller's concern, same as [`theme_file`].
pub fn profile_path(name: &str) -> Option<PathBuf> {
    Some(config_dir()?.join("profiles").join(name))
}
