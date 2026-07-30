//! `lazydap`'s terminal UI.
//!
//! A **client**, not a peer. The crate graph enforces that: this crate may
//! depend on `core`, `protocol` and `config` and nothing else
//! (`ARCHITECTURE.md`, checked by `scripts/check_architecture_boundaries.sh`),
//! so a feature that only the TUI has is not something that can be built here
//! — it has to go through the protocol, which means the CLI gets it too
//! (non-negotiable 2).
//!
//! M8 is the render loop on its own: no daemon, no DAP, no state model. M9
//! adds the source pane, M10 the reducer, M11 the daemon connection.

mod error;
#[cfg(test)]
mod testing;

pub use error::{Result, TuiError};

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::Alignment;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::Duration;

/// How long a draw waits for a key before redrawing anyway.
///
/// The loop is otherwise entirely input-driven, so this is what puts an upper
/// bound on how stale the screen can be. M10 replaces it with a tick.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Run the TUI until the user quits.
///
/// Takes the terminal over — raw mode, alternate screen — and gives it back
/// whatever happens, including a panic: `ratatui::try_init` installs a hook
/// that restores it before unwinding. A debugger that leaves your shell in raw
/// mode when it crashes is worse than no debugger.
pub fn run() -> Result<()> {
    tracing::debug!(target: "tui.lifecycle", "entering the TUI");
    let mut terminal = ratatui::try_init()?;

    let result = run_loop(&mut terminal);

    // Unconditional: a loop that failed still borrowed the terminal.
    ratatui::restore();
    tracing::debug!(target: "tui.lifecycle", "left the TUI");
    result
}

fn run_loop(terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(view)?;

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        if let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && is_quit(key)
        {
            return Ok(());
        }
    }
}

fn view(frame: &mut Frame) {
    let block = Block::default()
        .title("lazydap")
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Cyan));
    let text = Paragraph::new(Line::from("press q to quit"))
        .alignment(Alignment::Center)
        .block(block);
    frame.render_widget(text, frame.area());
}

/// Whether a keypress means "leave".
///
/// Split out because it is the one piece of M8 with a decision in it, and a
/// test can reach it without a terminal.
fn is_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn q_and_escape_both_leave_but_an_ordinary_letter_does_not() {
        assert!(is_quit(key(KeyCode::Char('q'))));
        assert!(is_quit(key(KeyCode::Esc)));
        assert!(!is_quit(key(KeyCode::Char('j'))));
    }

    #[test]
    fn the_empty_screen_says_what_it_is_and_how_to_leave() {
        assert_eq!(
            render(24, 4, view),
            [
                "┌lazydap───────────────┐",
                "│    press q to quit   │",
                "│                      │",
                "└──────────────────────┘",
            ],
        );
    }
}
