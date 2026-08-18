//! Named flag bundles. A profile is a flat text file, one `flag = value` or
//! bare `flag` line per CLI option, in the same hand-rolled spirit as the
//! theme file (see [`crate::display::speedometer::theme`]): no
//! config-parsing dependency, just lines that expand into the ordinary
//! `--flag`/`--flag=value` tokens [`crate::parse_args`]-equivalent code
//! already understands.
//!
//! ```text
//! parser = ping
//! title = ping ms
//! zero
//! kalman
//! kalman-r = 1300
//! ```
//!
//! Selected with `--profile NAME`: `NAME` containing a `/` is read as a
//! literal path, otherwise it's looked up under
//! `~/.config/termtaco/profiles/NAME` (see [`crate::infra::config`]), then
//! among the built-in presets (see [`PRESETS`]).

/// Built-in profile presets, embedded at compile time from the repo's
/// `profiles/*.profile` files, mirroring how theme presets are embedded from
/// `themes/*.theme`.
const PRESETS: &[(&str, &str)] = &[("ping", include_str!("../../profiles/ping.profile"))];

/// Names accepted by `--profile`'s built-in presets, for help/error messages.
pub const PRESET_NAMES: &str = "ping";

/// The embedded content of a built-in profile by name (see [`PRESETS`]).
/// `None` if `name` isn't one of [`PRESET_NAMES`].
pub fn preset_content(name: &str) -> Option<&'static str> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, src)| *src)
}

/// Resolves `name` to a profile's raw content, unparsed: a literal path (if
/// `name` contains a `/`), then `~/.config/termtaco/profiles/NAME`, then a
/// built-in preset. `Err` lists the built-ins, for a name that matched none.
pub fn load(name: &str) -> Result<String, String> {
    if name.contains('/') {
        return std::fs::read_to_string(name).map_err(|e| format!("profile '{name}': {e}"));
    }
    if let Some(content) = crate::infra::config::profile_path(name).and_then(|p| std::fs::read_to_string(p).ok()) {
        return Ok(content);
    }
    preset_content(name)
        .map(str::to_string)
        .ok_or_else(|| format!("unknown profile '{name}' (available: {PRESET_NAMES}, or a path containing '/')"))
}

/// CLI flag names a profile isn't allowed to set: early-exit flags (they'd
/// never let the rest of the command line run) and `--profile` itself
/// (profiles don't nest).
const REJECTED_KEYS: &[&str] = &["h", "help", "help-all", "profile", "print-profile", "print-theme"];

/// Expands a profile's raw content into CLI tokens: a `flag = value` line
/// becomes `--flag=value`, a bare `flag` line becomes `--flag`, in the order
/// written. Blank lines and `#` comments are skipped. `Err` on a
/// [`REJECTED_KEYS`] line.
pub fn expand(content: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((k, v)) => (k.trim(), Some(v.trim())),
            None => (line, None),
        };
        if REJECTED_KEYS.contains(&key) {
            return Err(format!("profile: '{key}' isn't allowed inside a profile"));
        }
        tokens.push(match value {
            Some(v) => format!("--{key}={v}"),
            None => format!("--{key}"),
        });
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_turns_valued_and_bare_lines_into_flags() {
        let tokens = expand("parser = ping\nzero\nkalman\nkalman-r = 1300\n").unwrap();
        assert_eq!(tokens, vec!["--parser=ping", "--zero", "--kalman", "--kalman-r=1300"]);
    }

    #[test]
    fn expand_skips_blank_lines_and_comments() {
        let tokens = expand("# a comment\n\nparser = ping\n  # indented comment\n").unwrap();
        assert_eq!(tokens, vec!["--parser=ping"]);
    }

    #[test]
    fn expand_trims_whitespace_around_key_and_value() {
        let tokens = expand("  parser   =   ping  \n").unwrap();
        assert_eq!(tokens, vec!["--parser=ping"]);
    }

    #[test]
    fn expand_preserves_an_equals_sign_inside_the_value() {
        let tokens = expand("title = a=b\n").unwrap();
        assert_eq!(tokens, vec!["--title=a=b"]);
    }

    #[test]
    fn expand_rejects_recursive_and_early_exit_keys() {
        assert!(expand("profile = ping\n").is_err());
        assert!(expand("help\n").is_err());
        assert!(expand("help-all\n").is_err());
        assert!(expand("print-theme = nord\n").is_err());
    }

    #[test]
    fn ping_preset_expands_cleanly() {
        assert!(expand(preset_content("ping").unwrap()).is_ok());
    }

    #[test]
    fn preset_content_is_none_for_an_unknown_name() {
        assert_eq!(preset_content("bogus"), None);
    }

    #[test]
    fn load_reports_an_unknown_name() {
        assert!(load("bogus").is_err());
    }

    #[test]
    fn load_treats_a_slash_containing_name_as_a_literal_path() {
        assert!(load("does/not/exist").is_err());
    }
}
