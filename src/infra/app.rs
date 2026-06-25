//! The render loop. Ingests values, maintains the window, and repaints the
//! chosen [`Display`] at ~30 fps. Decoupled from any specific renderer.

use std::io;
use std::sync::mpsc::{self, TryRecvError};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::display::Display;
use crate::infra::input;
use crate::infra::terminal::Tui;
use crate::math::stats::Window;

const FRAME: Duration = Duration::from_millis(33); // ~30 fps

/// Run the loop until the user quits or stdin closes and they quit.
///
/// `display` is any renderer; the loop never inspects it beyond `render`.
pub fn run(term: &mut Tui, display: &mut dyn Display, window_cap: usize) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<f64>();
    // rx is held here; on return it drops and the reader's next send fails,
    // which is what stops the reader thread.
    let _reader = input::spawn_reader(tx);

    let mut window = Window::new(window_cap);
    let mut last_draw = Instant::now() - FRAME;
    let mut dirty = true;
    let mut stdin_closed = false;

    loop {
        // 1. Drain all pending samples so we never lag a fast producer.
        loop {
            match rx.try_recv() {
                Ok(v) => {
                    window.push(v);
                    dirty = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // stdin reached EOF. Keep the last frame, but remember it
                    // so the placeholder can stop claiming we're still waiting.
                    if !stdin_closed {
                        stdin_closed = true;
                        dirty = true;
                    }
                    break;
                }
            }
        }

        // 2. Repaint when a frame is due.
        if dirty && last_draw.elapsed() >= FRAME {
            match window.stats() {
                Some(stats) => {
                    term.draw(|f| display.render(f, f.size(), &stats))?;
                }
                None => {
                    let msg = if stdin_closed {
                        "stdin closed with no data — press q to quit"
                    } else {
                        "waiting for data on stdin…"
                    };
                    term.draw(|f| {
                        let placeholder = Paragraph::new(msg).block(
                            Block::default().borders(Borders::ALL).title(" gauge "),
                        );
                        f.render_widget(placeholder, f.size());
                    })?;
                }
            }
            last_draw = Instant::now();
            dirty = false;
        }

        // 3. Wait for a key (or the next frame deadline) on the controlling
        //    tty — stdin is busy carrying data, so events come from the tty.
        let wait = FRAME
            .saturating_sub(last_draw.elapsed())
            .max(Duration::from_millis(1));
        if event::poll(wait)? {
            match event::read()? {
                Event::Key(k)
                    if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat =>
                {
                    match (k.code, k.modifiers) {
                        (KeyCode::Char('q'), _) => break,
                        (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                        (KeyCode::Esc, _) => break,
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
