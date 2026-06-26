//! `gauge` — a tiny TUI speedometer for streaming values.
//!
//! Reads one float per line from stdin (lenient: extracts the first number it
//! finds), maintains running min/max/mean/stddev over a window, and renders a
//! live radial dial. Three layers: `math` (stats), `infra` (stdin/terminal/
//! loop), and `display` (pluggable renderers, default speedometer).

mod display;
mod infra;
mod math;

use std::io;
use std::process::ExitCode;

struct Args {
    window: usize,
    display: String,
    title: Option<String>,
    include_zero: bool,
}

const HELP: &str = "\
gauge — a tiny TUI speedometer for streaming values

USAGE:
    <producer> | gauge [OPTIONS]

OPTIONS:
    --window N        samples retained for stats (default: 200)
    --display NAME    renderer to use (default: speedometer)
    --title TEXT      title shown at the top of the dial
    --0, --zero       always keep 0 in the scale (e.g. a speedometer)
    -h, --help        print this help

Reads one float per line from stdin; the first number on each line is used,
so it sits downstream of output like `average rate: 33.746`.
Quit with q, Esc, or Ctrl-C.";

fn parse_args() -> Result<Args, ExitCode> {
    let mut window = 200usize;
    let mut display = String::from("speedometer");
    let mut title: Option<String> = None;
    let mut include_zero = false;
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
            "--title" => {
                title = it.next().or_else(|| {
                    eprintln!("--title needs a value");
                    std::process::exit(2);
                });
            }
            s if s.starts_with("--title=") => {
                title = Some(s["--title=".len()..].to_string());
            }
            "--0" | "--zero" => {
                include_zero = true;
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
        title,
        include_zero,
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
    display.set_include_zero(args.include_zero);

    match run(&mut *display, args.window) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("gauge: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Set up the terminal, run the loop, and always restore on the way out.
fn run(display: &mut dyn display::Display, window: usize) -> io::Result<()> {
    infra::terminal::install_panic_hook();
    let mut term = infra::terminal::setup()?;
    let res = infra::app::run(&mut term, display, window);
    let restore = infra::terminal::restore(&mut term);
    res.and(restore)
}
