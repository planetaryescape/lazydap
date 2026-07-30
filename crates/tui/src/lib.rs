//! `lazydap`'s terminal UI.
//!
//! A **client**, not a peer. The crate graph enforces that: this crate may
//! depend on `core`, `protocol` and `config` and nothing else
//! (`ARCHITECTURE.md`, checked by `scripts/check_architecture_boundaries.sh`),
//! so a feature that only the TUI has is not something that can be built here
//! — it has to go through the protocol, which means the CLI gets it too
//! (non-negotiable 2).
//!
//! M9 is the source pane: a file, line numbers, vim keys. No daemon, no DAP,
//! and no state model — M10 adds the reducer, M11 the daemon connection.

mod error;
mod panes;
#[cfg(test)]
mod testing;

pub use error::{Result, TuiError};

use panes::source::SourceView;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::time::Duration;

/// How long a draw waits for a key before redrawing anyway.
///
/// The loop is otherwise entirely input-driven, so this is what puts an upper
/// bound on how stale the screen can be. M10 replaces it with a tick.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The file the TUI opens with.
///
/// Hardcoded, and only until M11: from there the file to show is whichever one
/// the daemon says the program is stopped in.
const FIXTURE: &str = "examples/c-hello/main.c";

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
    // A file that is not there is not a reason to refuse to start: the pane
    // says so and everything else still works.
    let mut source = SourceView::open(FIXTURE)
        .inspect_err(|error| {
            tracing::warn!(target: "tui.source", file = FIXTURE, %error, "could not open the file");
        })
        .ok();
    // `gg` is two keystrokes, so the first one has to be remembered. M10 moves
    // this into the state where it belongs.
    let mut pending_g = false;

    loop {
        terminal.draw(|frame| view(frame, source.as_mut()))?;

        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        let was_pending_g = std::mem::take(&mut pending_g);
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
            KeyCode::Char('g') if was_pending_g => {
                if let Some(source) = source.as_mut() {
                    source.go_to_top();
                }
            }
            KeyCode::Char('g') => pending_g = true,
            _ => {
                if let Some(source) = source.as_mut() {
                    scroll(source, key);
                }
            }
        }
    }
}

/// The keys that move the cursor. Split out so M10 can lift it into the
/// reducer unchanged.
fn scroll(source: &mut SourceView, key: KeyEvent) {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => source.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => source.move_cursor(-1),
        KeyCode::Char('d') if control => source.move_cursor(source.half_page()),
        KeyCode::Char('u') if control => source.move_cursor(-source.half_page()),
        KeyCode::Char('G') => source.go_to_bottom(),
        _ => {}
    }
}

fn view(frame: &mut Frame, source: Option<&mut SourceView>) {
    // The shape the rest of Phase C builds on: panes above, one status row at
    // the bottom that is always exactly one line tall.
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    let where_we_are = match source {
        Some(source) => {
            let position = format!("{}:{}", source.path().display(), source.cursor_line());
            source.render(frame, body, true);
            position
        }
        None => {
            render_empty(frame, body);
            "no source".to_string()
        }
    };

    frame.render_widget(
        Paragraph::new(format!("{where_we_are} · q to quit"))
            .style(Style::default().fg(Color::DarkGray)),
        status,
    );
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let block = Block::default().title("lazydap").borders(Borders::ALL);
    frame.render_widget(Paragraph::new("no source loaded").block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn control(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    fn numbered(count: u32) -> SourceView {
        let body: Vec<String> = (1..=count).map(|line| format!("line {line}")).collect();
        SourceView::from_contents("/tmp/numbers.txt", &body.join("\n"))
    }

    #[test]
    fn j_and_k_move_the_cursor_one_line_at_a_time() {
        let mut source = numbered(10);

        scroll(&mut source, key(KeyCode::Char('j')));
        scroll(&mut source, key(KeyCode::Char('j')));
        assert_eq!(source.cursor_line(), 3);

        scroll(&mut source, key(KeyCode::Char('k')));
        assert_eq!(source.cursor_line(), 2);
    }

    #[test]
    fn the_arrow_keys_do_what_j_and_k_do() {
        let mut source = numbered(10);

        scroll(&mut source, key(KeyCode::Down));
        assert_eq!(source.cursor_line(), 2);

        scroll(&mut source, key(KeyCode::Up));
        assert_eq!(source.cursor_line(), 1);
    }

    #[test]
    fn shift_g_goes_to_the_last_line() {
        let mut source = numbered(10);
        scroll(&mut source, key(KeyCode::Char('G')));
        assert_eq!(source.cursor_line(), 10);
    }

    #[test]
    fn a_bare_d_is_not_half_a_page() {
        // `d` on its own is a vim operator, not a scroll. Treating it as one
        // would make a future `dd`-style binding impossible to add.
        let mut source = numbered(50);
        scroll(&mut source, key(KeyCode::Char('d')));
        assert_eq!(source.cursor_line(), 1);

        scroll(&mut source, control(KeyCode::Char('d')));
        assert!(source.cursor_line() > 1);
    }

    #[test]
    fn an_unbound_key_leaves_the_cursor_alone() {
        let mut source = numbered(10);
        scroll(&mut source, key(KeyCode::Char('z')));
        assert_eq!(source.cursor_line(), 1);
    }

    #[test]
    fn the_empty_state_says_there_is_nothing_to_show_rather_than_drawing_nothing() {
        assert_eq!(
            render(20, 4, |frame| view(frame, None)),
            [
                "┌lazydap───────────┐",
                "│no source loaded  │",
                "└──────────────────┘",
                // Clipped at the pane width, not wrapped onto a second row.
                "no source · q to qui",
            ],
        );
    }

    #[test]
    fn the_status_row_says_which_line_the_cursor_is_on() {
        let mut source = numbered(10);
        source.move_cursor(3);

        let screen = render(40, 5, |frame| view(frame, Some(&mut source)));

        assert_eq!(
            screen[4], "/tmp/numbers.txt:4 · q to quit          ",
            "got: {screen:?}",
        );
    }
}
