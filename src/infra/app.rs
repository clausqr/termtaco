//! The render loop. Ingests values, maintains the window, and repaints the
//! chosen [`Display`] at the configured frame rate. Decoupled from any specific
//! renderer; all runtime knobs arrive via [`LoopConfig`].

use std::io;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::display::{Display, Reading};
use crate::infra::feed::Feed;
use crate::infra::input;
use crate::infra::terminal::Tui;
use crate::math::kalman::Kalman;

/// Floor on how long `event::poll` is asked to wait, so a frame deadline
/// that's already passed (or nearly has) still yields a nonzero, non-busy
/// wait rather than a zero-duration poll spinning the loop.
const MIN_POLL_WAIT: Duration = Duration::from_millis(1);

/// Runtime knobs for the render loop, all sourced from the CLI.
pub struct LoopConfig {
    /// Samples retained in the statistics window.
    pub window: usize,
    /// Minimum time between repaints (the inverse of the refresh rate).
    pub frame: Duration,
    /// Idle time after the last sample before the reading is flagged stale.
    pub stale_after: Duration,
    /// Border label, also used for the pre-data placeholder block.
    pub border_label: String,
    /// How to extract a value from each input line.
    pub parser: input::Parser,
    /// Smooth the displayed value with a Kalman filter (stats stay raw).
    pub kalman: bool,
    /// Kalman process noise variance, used when `kalman` is set.
    pub kalman_q: f64,
    /// Kalman measurement noise variance, used when `kalman` is set.
    pub kalman_r: f64,
    /// Some display effects keep moving between measurements (the Kalman
    /// estimate extrapolating along its velocity, the needle settling toward
    /// a target with `--needle-inertia`), so repaint every frame rather than
    /// only when new data arrives.
    pub animate: bool,
}

/// Run the loop until the user quits or stdin closes and they quit.
///
/// `display` is any renderer; the loop never inspects it beyond `render`.
/// Ingestion, filtering, and staleness bookkeeping live in [`Feed`]: this
/// function is orchestration: drive `Feed` each pass, repaint when it says
/// to, and handle terminal I/O (which `Feed` deliberately knows nothing
/// about, so its decision logic stays testable without a real tty).
pub fn run(term: &mut Tui, display: &mut dyn Display, cfg: &LoopConfig) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<f64>();
    // rx is held here; on return it drops and the reader's next send fails,
    // which is what stops the reader thread.
    let _reader = input::spawn_reader(tx, cfg.parser.clone());

    let mut feed = Feed::new(cfg.window, cfg.stale_after, cfg.kalman.then(|| Kalman::new(cfg.kalman_q, cfg.kalman_r)));
    let mut last_draw = Instant::now() - cfg.frame;
    let mut dirty = true;

    loop {
        // `|=`, not `if`: both must run every pass regardless of the other's
        // result; draining is independent of whether the filter/staleness
        // state also wants a repaint this pass.
        dirty |= feed.drain(&rx);
        dirty |= feed.tick(cfg.animate);

        // Repaint when a frame is due.
        if dirty && last_draw.elapsed() >= cfg.frame {
            match feed.snapshot() {
                Some((stats, smoothed)) => {
                    let reading = Reading {
                        stats,
                        smoothed,
                        stale: feed.stale(),
                        kalman_uncertainty: feed.kalman_uncertainty(),
                    };
                    term.draw(|f| display.render(f, f.size(), &reading))?;
                }
                None => {
                    let stdin_closed = feed.stdin_closed();
                    term.draw(|f| draw_placeholder(f, &cfg.border_label, stdin_closed))?;
                }
            }
            last_draw = Instant::now();
            dirty = false;
        }

        // Wait for a key (or the next frame deadline) on the controlling
        // tty: stdin is busy carrying data, so events come from the tty.
        let wait = cfg.frame.saturating_sub(last_draw.elapsed()).max(MIN_POLL_WAIT);
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(k)
                    if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat =>
                {
                    match (k.code, k.modifiers) {
                        (KeyCode::Char('q'), _) => break,
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Esc, _) => break,
                        (KeyCode::Char('t'), _) => {
                            display.cycle_theme();
                            dirty = true;
                        }
                        _ => {}
                    }
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
    }
    Ok(())
}

/// The pre-data / stdin-closed placeholder screen.
fn draw_placeholder(f: &mut Frame, border_label: &str, stdin_closed: bool) {
    let msg = if stdin_closed {
        "stdin closed with no data, press q to quit"
    } else {
        "waiting for data on stdin…"
    };
    let mut block = Block::default().borders(Borders::ALL);
    if !border_label.is_empty() {
        block = block.title(format!(" {border_label} "));
    }
    let placeholder = Paragraph::new(msg).block(block);
    f.render_widget(placeholder, f.size());
}
