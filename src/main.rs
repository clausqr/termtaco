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
}

// CLI defaults for the runtime knobs.
const DEFAULT_WINDOW: usize = 200;
const DEFAULT_FPS: f64 = 30.0;
const DEFAULT_STALE_SECS: f64 = 3.0;
const DEFAULT_OVERFLOW_HOLD_SECS: f64 = 1.0;
// Empty by default: no border label unless --border-label is given.
const DEFAULT_BORDER_LABEL: &str = "";

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
    -h, --help           print this help

Reads one float per line from stdin; by default the first number on each line
is used, so it sits downstream of output like `average rate: 33.746`. Pick a
different --parser when the value isn't first, e.g. `ping <host> | termtaco
--parser ping` for a live latency dial.
Quit with q, Esc, or Ctrl-C.";

fn parse_args() -> Result<Args, ExitCode> {
    let mut window = DEFAULT_WINDOW;
    let mut display = String::from("speedometer");
    let mut parser = infra::input::Parser::First;
    let mut title: Option<String> = None;
    let mut border_label = String::from(DEFAULT_BORDER_LABEL);
    let mut include_zero = false;
    let mut fps = DEFAULT_FPS;
    let mut stale_secs = DEFAULT_STALE_SECS;
    let mut overflow_secs = DEFAULT_OVERFLOW_HOLD_SECS;
    let mut it = std::env::args().skip(1);

    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Err(ExitCode::SUCCESS);
            }
            "--window" => {
                window = parse_window(it.next().as_deref())?;
            }
            s if s.starts_with("--window=") => {
                window = parse_window(Some(&s["--window=".len()..]))?;
            }
            "--display" => {
                display = it.next().ok_or_else(|| {
                    eprintln!("--display needs a name (one of: {})", display::AVAILABLE.join(", "));
                    ExitCode::from(2)
                })?;
            }
            s if s.starts_with("--display=") => {
                display = s["--display=".len()..].to_string();
            }
            "--parser" => {
                parser = parse_parser(it.next().as_deref())?;
            }
            s if s.starts_with("--parser=") => {
                parser = parse_parser(Some(&s["--parser=".len()..]))?;
            }
            "--title" => {
                title = it.next().or_else(|| {
                    eprintln!("--title needs a value");
                    std::process::exit(2);
                });
            }
            s if s.starts_with("--title=") => {
                title = Some(s["--title=".len()..].to_string());
            }
            "--border-label" => {
                border_label = match it.next() {
                    Some(v) => v,
                    None => {
                        eprintln!("--border-label needs a value");
                        return Err(ExitCode::from(2));
                    }
                };
            }
            s if s.starts_with("--border-label=") => {
                border_label = s["--border-label=".len()..].to_string();
            }
            "--0" | "--zero" => {
                include_zero = true;
            }
            "--fps" => fps = parse_f64("--fps", it.next().as_deref(), 1.0, 240.0)?,
            s if s.starts_with("--fps=") => {
                fps = parse_f64("--fps", Some(&s["--fps=".len()..]), 1.0, 240.0)?
            }
            "--stale-after" => {
                stale_secs = parse_f64("--stale-after", it.next().as_deref(), 0.1, 86_400.0)?
            }
            s if s.starts_with("--stale-after=") => {
                stale_secs = parse_f64("--stale-after", Some(&s["--stale-after=".len()..]), 0.1, 86_400.0)?
            }
            "--overflow-hold" => {
                overflow_secs = parse_f64("--overflow-hold", it.next().as_deref(), 0.0, 86_400.0)?
            }
            s if s.starts_with("--overflow-hold=") => {
                overflow_secs = parse_f64("--overflow-hold", Some(&s["--overflow-hold=".len()..]), 0.0, 86_400.0)?
            }
            other => {
                eprintln!("unknown argument: {other}\n\n{HELP}");
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

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(code) => return code,
    };

    let mut display = match display::make(&args.display) {
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

    if let Some(t) = args.title {
        display.set_title(t);
    }
    display.set_border_label(args.border_label.clone());
    display.set_include_zero(args.include_zero);
    display.set_overflow_hold(args.overflow_hold);

    let cfg = infra::app::LoopConfig {
        window: args.window,
        frame: args.frame,
        stale_after: args.stale_after,
        border_label: args.border_label,
        parser: args.parser,
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
