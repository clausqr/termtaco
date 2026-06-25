//! Terminal lifecycle: enter/leave raw-mode alternate screen, plus a panic
//! hook so a crash never leaves the operator's terminal wedged.

use std::io::{self, Stdout};

use crossterm::{
    cursor::Show,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Enter raw mode + the alternate screen and build the ratatui terminal.
///
/// Self-cleaning: if any step after `enable_raw_mode` fails, raw mode is
/// disabled (and the alternate screen left) before returning the error, so a
/// partial setup never leaves the operator's terminal wedged.
pub fn setup() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut out = io::stdout();
    if let Err(e) = execute!(out, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(e);
    }
    Terminal::new(CrosstermBackend::new(out)).map_err(|e| {
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        let _ = disable_raw_mode();
        e
    })
}

/// Restore the terminal to its original state.
pub fn restore(term: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen)?;
    term.show_cursor()
}

/// Install a panic hook that best-effort restores the terminal before the
/// default hook prints the panic message.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
        default_hook(info);
    }));
}
