//! `termtaco` — a terminal tachometer; a tiny TUI speedometer for streaming values.
//!
//! Reads one float per line from stdin (lenient: by default it extracts the
//! first number it finds; `--parser` picks another strategy, e.g. `ping`),
//! maintains running min/max/mean/stddev over a window, and renders a
//! live radial dial. Three layers: `math` (stats), `infra` (stdin/terminal/
//! loop), and `display` (pluggable renderers, default speedometer).

mod display;
mod infra;
mod math;

use std::io;
use std::process::ExitCode;
use std::time::Duration;

struct Args {
    window: usize,
    display: String,
    parser: infra::input::Parser,
    title: Option<String>,
    border_label: String,
    include_zero: bool,
    frame: Duration,
    stale_after: Duration,
    overflow_hold: Duration,
    kalman: bool,
    kalman_q: f64,
    kalman_r: f64,
    max_decay: Option<Duration>,
    max_decay_target: f64,
    needle_inertia: Option<Duration>,
    /// Raw content of the `--theme` preset, resolved and validated at parse
    /// time. `None` falls back to `~/.config/termtaco/theme`, then the
    /// built-in default.
    theme_file: Option<String>,
}

// CLI defaults for the runtime knobs. The overflow-hold and max-decay-target
// defaults live in `display::speedometer` (the single source of truth for
// what the dial itself defaults to) and are cited from there rather than
// duplicated here.
const DEFAULT_WINDOW: usize = 200;
const DEFAULT_FPS: f64 = 30.0;
const DEFAULT_STALE_SECS: f64 = 3.0;
// Empty by default: no border label unless --border-label is given.
const DEFAULT_BORDER_LABEL: &str = "";
const DEFAULT_KALMAN_Q: f64 = 0.001;
const DEFAULT_KALMAN_R: f64 = 0.1;
// 0 = off, the needle snaps straight to the reading (today's behavior).
const DEFAULT_NEEDLE_INERTIA_SECS: f64 = 0.0;

const HELP: &str = "\
termtaco — a terminal tachometer; a tiny TUI speedometer for streaming values

USAGE:
    <producer> | termtaco [OPTIONS]

OPTIONS:
    --window N           samples retained for stats (default: 200)
    --display NAME       renderer to use (default: speedometer)
    --parser SPEC        how to extract the value from each line (default: first)
                           first      first number on the line
                           last       last number on the line
                           nth:N      N-th number on the line (1-based)
                           key:NAME   number after 'NAME=' or 'NAME:'
                           ping       RTT from ping output (the time= field)
    --title TEXT         title shown at the top of the dial
    --border-label TEXT  text in the dial's border (default: none)
    --0, --zero          always keep 0 in the scale (e.g. a speedometer)
    --fps N              refresh rate, frames per second (default: 30)
    --stale-after SECS   silence before the reading is flagged stale (default: 3)
    --overflow-hold SECS hold a capped reading this long before rescaling (default: 1)
    --kalman             smooth the needle/value with a Kalman filter (stats stay raw)
    --kalman-q Q         Kalman process noise variance per second (default: 0.001)
    --kalman-r R         Kalman measurement noise variance (default: 0.1)
    --max-decay SECS     decay the max tick toward max-decay-target x mean once
                          idle, instead of holding until it exits the window
    --max-decay-target M equilibrium multiplier of the mean for --max-decay (default: 2.0)
    --needle-inertia SECS
                         give the needle mass: it lags the reading and settles
                          over ~5x SECS (default: 0, the needle snaps)
    --theme NAME         built-in color preset (default: bw)
                           bw, color, catppuccin-mocha, dracula, gruvbox,
                           nord, solarized-dark, tokyo-night
    -h, --help           print this help

Reads one float per line from stdin; by default the first number on each line
is used, so it sits downstream of output like `average rate: 33.746`. Pick a
different --parser when the value isn't first, e.g. `ping <host> | termtaco
--parser ping` for a live latency dial.

--theme picks a built-in palette; for a fully custom one, write
~/.config/termtaco/theme (one `field = color` line per dial element) — see
themes/ in the repo for examples. --theme overrides that file when both are
given.
Quit with q, Esc, or Ctrl-C.";

/// The value attached to a flag: the inline part of `--flag=value`, or the
/// next argv entry for the `--flag value` form. `None` when neither exists,
/// which each caller turns into that flag's own error message.
fn flag_value(inline: Option<&str>, it: &mut impl Iterator<Item = String>) -> Option<String> {
    match inline {
        Some(v) => Some(v.to_string()),
        None => it.next(),
    }
}

fn parse_args<I: IntoIterator<Item = String>>(argv: I) -> Result<Args, ExitCode> {
    let mut window = DEFAULT_WINDOW;
    let mut display = String::from("speedometer");
    let mut parser = infra::input::Parser::First;
    let mut title: Option<String> = None;
    let mut border_label = String::from(DEFAULT_BORDER_LABEL);
    let mut include_zero = false;
    let mut fps = DEFAULT_FPS;
    let mut stale_secs = DEFAULT_STALE_SECS;
    let mut overflow_secs = display::speedometer::DEFAULT_OVERFLOW_HOLD.as_secs_f64();
    let mut kalman = false;
    let mut kalman_q = DEFAULT_KALMAN_Q;
    let mut kalman_r = DEFAULT_KALMAN_R;
    let mut max_decay_secs: Option<f64> = None;
    let mut max_decay_target = display::speedometer::DEFAULT_MAX_DECAY_TARGET;
    let mut needle_inertia_secs = DEFAULT_NEEDLE_INERTIA_SECS;
    let mut theme_file: Option<String> = None;
    let mut it = argv.into_iter();

    while let Some(raw) = it.next() {
        // `--flag=value` -> ("--flag", Some("value")); a bare `--flag` -> ("--flag", None).
        // Split on the *first* '=' so a value containing '=' survives intact
        // (`--title=a=b` sets the title to "a=b").
        let (key, inline) = match raw.find('=') {
            Some(i) => (&raw[..i], Some(&raw[i + 1..])),
            None => (raw.as_str(), None),
        };
        match (key, inline) {
            ("-h", None) | ("--help", None) => {
                println!("{HELP}");
                return Err(ExitCode::SUCCESS);
            }
            ("--window", v) => window = parse_window(flag_value(v, &mut it).as_deref())?,
            ("--display", v) => {
                display = flag_value(v, &mut it).ok_or_else(|| {
                    eprintln!("--display needs a name (one of: {})", display::AVAILABLE.join(", "));
                    ExitCode::from(2)
                })?;
            }
            ("--parser", v) => parser = parse_parser(flag_value(v, &mut it).as_deref())?,
            ("--title", v) => {
                title = Some(flag_value(v, &mut it).ok_or_else(|| {
                    eprintln!("--title needs a value");
                    ExitCode::from(2)
                })?);
            }
            ("--border-label", v) => {
                border_label = flag_value(v, &mut it).ok_or_else(|| {
                    eprintln!("--border-label needs a value");
                    ExitCode::from(2)
                })?;
            }
            ("--0", None) | ("--zero", None) => include_zero = true,
            ("--fps", v) => fps = parse_f64("--fps", flag_value(v, &mut it).as_deref(), 1.0, 240.0)?,
            ("--stale-after", v) => {
                stale_secs = parse_f64("--stale-after", flag_value(v, &mut it).as_deref(), 0.1, 86_400.0)?
            }
            ("--overflow-hold", v) => {
                overflow_secs = parse_f64("--overflow-hold", flag_value(v, &mut it).as_deref(), 0.0, 86_400.0)?
            }
            ("--kalman", None) => kalman = true,
            ("--kalman-q", v) => kalman_q = parse_f64("--kalman-q", flag_value(v, &mut it).as_deref(), 1e-9, 1e9)?,
            ("--kalman-r", v) => kalman_r = parse_f64("--kalman-r", flag_value(v, &mut it).as_deref(), 1e-9, 1e9)?,
            ("--max-decay", v) => {
                max_decay_secs = Some(parse_f64("--max-decay", flag_value(v, &mut it).as_deref(), 0.01, 86_400.0)?)
            }
            ("--max-decay-target", v) => {
                max_decay_target =
                    parse_f64("--max-decay-target", flag_value(v, &mut it).as_deref(), 0.0, 1000.0)?
            }
            ("--needle-inertia", v) => {
                needle_inertia_secs =
                    parse_f64("--needle-inertia", flag_value(v, &mut it).as_deref(), 0.0, 60.0)?
            }
            ("--theme", v) => theme_file = Some(parse_theme(flag_value(v, &mut it).as_deref())?),
            _ => {
                eprintln!("unknown argument: {raw}\n\n{HELP}");
                return Err(ExitCode::from(2));
            }
        }
    }

    if display.is_empty() {
        eprintln!("--display needs a name (one of: {})", display::AVAILABLE.join(", "));
        return Err(ExitCode::from(2));
    }

    Ok(Args {
        window,
        display,
        parser,
        title,
        border_label,
        include_zero,
        frame: Duration::from_secs_f64(1.0 / fps),
        stale_after: Duration::from_secs_f64(stale_secs),
        overflow_hold: Duration::from_secs_f64(overflow_secs),
        kalman,
        kalman_q,
        kalman_r,
        max_decay: max_decay_secs.map(Duration::from_secs_f64),
        max_decay_target,
        needle_inertia: (needle_inertia_secs > 0.0).then(|| Duration::from_secs_f64(needle_inertia_secs)),
        theme_file,
    })
}

/// Parse the `--theme` spec, or report a clear error listing the presets.
fn parse_theme(v: Option<&str>) -> Result<String, ExitCode> {
    let name = v.ok_or_else(|| {
        eprintln!("--theme needs a name (one of: {})", display::speedometer::PRESET_NAMES);
        ExitCode::from(2)
    })?;
    display::speedometer::preset_content(name).map(str::to_string).ok_or_else(|| {
        eprintln!("unknown theme '{name}' (available: {})", display::speedometer::PRESET_NAMES);
        ExitCode::from(2)
    })
}

/// Upper bound on `--window`. The window is pre-allocated as a `VecDeque`, so
/// an absurd value would eagerly reserve gigabytes; cap it at a sane maximum.
const MAX_WINDOW: usize = 1_000_000;

fn parse_window(v: Option<&str>) -> Result<usize, ExitCode> {
    match v.and_then(|v| v.parse::<usize>().ok()) {
        Some(n) if (1..=MAX_WINDOW).contains(&n) => Ok(n),
        _ => {
            eprintln!("--window needs an integer between 1 and {MAX_WINDOW}");
            Err(ExitCode::from(2))
        }
    }
}

/// Parse the `--parser` spec, or report a clear error.
fn parse_parser(v: Option<&str>) -> Result<infra::input::Parser, ExitCode> {
    let spec = v.ok_or_else(|| {
        eprintln!("--parser needs a spec (one of: {})", infra::input::PARSER_SPECS);
        ExitCode::from(2)
    })?;
    infra::input::Parser::from_spec(spec).map_err(|e| {
        eprintln!("--parser: {e}");
        ExitCode::from(2)
    })
}

/// Parse a finite `f64` flag within `[min, max]`, or report a clear error.
fn parse_f64(name: &str, v: Option<&str>, min: f64, max: f64) -> Result<f64, ExitCode> {
    match v.and_then(|v| v.parse::<f64>().ok()) {
        Some(n) if n.is_finite() && (min..=max).contains(&n) => Ok(n),
        _ => {
            eprintln!("{name} needs a number between {min} and {max}");
            Err(ExitCode::from(2))
        }
    }
}

/// Whether any active effect keeps changing between measurements (Kalman
/// extrapolation, needle settling, max decay), and therefore needs the
/// render loop to repaint every frame instead of only on new data.
fn needs_animation(kalman: bool, needle_inertia: Option<Duration>, max_decay: Option<Duration>) -> bool {
    kalman || needle_inertia.is_some() || max_decay.is_some()
}

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let display_cfg = display::DisplayConfig {
        title: args.title,
        // Cloned: LoopConfig below also needs border_label, for the pre-data placeholder.
        border_label: args.border_label.clone(),
        include_zero: args.include_zero,
        overflow_hold: args.overflow_hold,
        max_decay: args.max_decay,
        max_decay_target: args.max_decay_target,
        needle_inertia: args.needle_inertia,
        // --theme wins over ~/.config/termtaco/theme when both are given.
        theme_file: args.theme_file.or_else(infra::config::theme_file),
    };

    let mut display = match display::make(&args.display, &display_cfg) {
        Some(d) => d,
        None => {
            eprintln!(
                "unknown display: {} (available: {})",
                args.display,
                display::AVAILABLE.join(", ")
            );
            return ExitCode::from(2);
        }
    };

    let cfg = infra::app::LoopConfig {
        window: args.window,
        frame: args.frame,
        stale_after: args.stale_after,
        border_label: args.border_label,
        parser: args.parser,
        kalman: args.kalman,
        kalman_q: args.kalman_q,
        kalman_r: args.kalman_r,
        animate: needs_animation(args.kalman, args.needle_inertia, args.max_decay),
    };

    match run(&mut *display, &cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("termtaco: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Set up the terminal, run the loop, and always restore on the way out.
fn run(display: &mut dyn display::Display, cfg: &infra::app::LoopConfig) -> io::Result<()> {
    infra::terminal::install_panic_hook();
    let mut term = infra::terminal::setup()?;
    let res = infra::app::run(&mut term, display, cfg);
    let restore = infra::terminal::restore(&mut term);
    res.and(restore)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(argv: &[&str]) -> Result<Args, ExitCode> {
        parse_args(argv.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults_when_no_args() {
        let a = args(&[]).unwrap();
        assert_eq!(a.window, DEFAULT_WINDOW);
        assert_eq!(a.display, "speedometer");
        assert_eq!(a.parser, infra::input::Parser::First);
        assert_eq!(a.title, None);
        assert_eq!(a.border_label, "");
        assert!(!a.include_zero);
        assert!(!a.kalman);
        assert_eq!(a.max_decay, None);
        assert_eq!(a.needle_inertia, None);
        assert_eq!(a.theme_file, None);
    }

    #[test]
    fn long_and_inline_forms_agree() {
        let a = args(&["--window", "100"]).unwrap();
        let b = args(&["--window=100"]).unwrap();
        assert_eq!(a.window, 100);
        assert_eq!(a.window, b.window);
    }

    #[test]
    fn unknown_flag_errors() {
        assert!(args(&["--bogus"]).is_err());
    }

    #[test]
    fn valueless_flag_rejects_an_inline_value() {
        // --kalman takes no value, so --kalman=true isn't the flag matching
        // with None — it falls through to the unknown-argument error.
        assert!(args(&["--kalman=true"]).is_err());
    }

    #[test]
    fn window_out_of_range_errors() {
        assert!(args(&["--window", "0"]).is_err());
        assert!(args(&["--window", "abc"]).is_err());
    }

    #[test]
    fn title_without_a_value_returns_err_not_exit() {
        // Before this refactor, a missing --title value called
        // std::process::exit(2) directly, which would have killed the test
        // process rather than returning an error — this test could not have
        // existed until that was fixed.
        assert!(args(&["--title"]).is_err());
    }

    #[test]
    fn title_inline_value_may_contain_equals() {
        let a = args(&["--title=a=b"]).unwrap();
        assert_eq!(a.title, Some("a=b".to_string()));
    }

    #[test]
    fn parser_spec_round_trips() {
        let a = args(&["--parser", "ping"]).unwrap();
        assert_eq!(a.parser, infra::input::Parser::Ping);
    }

    #[test]
    fn parser_spec_rejects_garbage() {
        assert!(args(&["--parser", "bogus"]).is_err());
    }

    #[test]
    fn theme_preset_round_trips() {
        let a = args(&["--theme", "nord"]).unwrap();
        assert_eq!(a.theme_file.as_deref(), display::speedometer::preset_content("nord"));
    }

    #[test]
    fn theme_preset_rejects_garbage() {
        assert!(args(&["--theme", "bogus"]).is_err());
    }

    #[test]
    fn theme_without_a_value_returns_err_not_exit() {
        assert!(args(&["--theme"]).is_err());
    }

    #[test]
    fn needle_inertia_zero_means_none() {
        let a = args(&["--needle-inertia", "0"]).unwrap();
        assert_eq!(a.needle_inertia, None);
        let b = args(&["--needle-inertia", "0.5"]).unwrap();
        assert_eq!(b.needle_inertia, Some(Duration::from_secs_f64(0.5)));
    }

    #[test]
    fn numeric_bounds_are_enforced_on_both_forms() {
        assert!(args(&["--kalman-q", "-1"]).is_err());
        assert!(args(&["--kalman-q=-1"]).is_err());
    }

    #[test]
    fn needs_animation_truth_table() {
        assert!(!needs_animation(false, None, None), "no active effect should not force animation");
        assert!(needs_animation(true, None, None), "kalman alone should animate");
        assert!(
            needs_animation(false, Some(Duration::from_millis(1)), None),
            "needle inertia alone should animate"
        );
        assert!(
            needs_animation(false, None, Some(Duration::from_secs(1))),
            "max decay alone should animate — this is the bug being fixed"
        );
        assert!(needs_animation(true, Some(Duration::from_millis(1)), Some(Duration::from_secs(1))));
    }
}
