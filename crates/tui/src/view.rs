//! Drawing the state. Reads it; does not decide anything about it.
//!
//! `&mut AppState` rather than `&AppState` for one reason, and it is worth
//! knowing: [`SourceView::render`] learns the pane's height as it draws, and
//! that height is what "keep the cursor on screen" and "half a page" are
//! measured in. Computing it outside the layout would mean duplicating the
//! layout. Nothing else here writes to the state (D012, M10's notes).

use crate::state::{AppState, SessionSnapshot};
use lazydap_core::SessionState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

/// The keys the status row advertises. Short on purpose: a help pane is Phase
/// D's job, and a row that lists everything is a row nobody reads.
const KEYS: &str = "F5 continue · F10 step · q quit";

pub fn view(frame: &mut Frame, state: &mut AppState) {
    // Panes above, one status row that is always exactly one line tall.
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    match state.source.as_mut() {
        Some(source) => source.render(frame, body, true),
        None => render_empty(frame, body),
    }

    frame.render_widget(
        Paragraph::new(format!("{} · {KEYS}", status_text(state)))
            .style(Style::default().fg(Color::DarkGray)),
        status,
    );
}

/// The left-hand end of the status row: what the daemon is doing, or what went
/// wrong. A notice wins, because it is the thing the user has not seen yet.
fn status_text(state: &AppState) -> String {
    if let Some(notice) = state.notice.as_deref() {
        return notice.to_string();
    }
    match state.session.as_ref() {
        Some(session) => session_text(state, session),
        // Not an error. A daemon with no session is the normal state of one
        // that has just started, and the CLI is how a session begins.
        None => "no session".to_string(),
    }
}

fn session_text(state: &AppState, session: &SessionSnapshot) -> String {
    match session.state {
        // Both halves are optional and for different reasons. The reason is
        // missing when the TUI joined a session that was already stopped — a
        // snapshot says the state, not the story behind it. The location is
        // missing for the fraction of a second between the stop and the stack
        // coming back. Neither is a reason to say less than is known.
        SessionState::Paused => {
            let mut text = "paused".to_string();
            if let Some(reason) = session.reason.as_ref() {
                text.push_str(&format!(" ({})", reason.as_str()));
            }
            if let Some(location) = state.location.as_ref() {
                text.push_str(&format!(" at {}:{}", short(&location.path), location.line,));
            }
            text
        }
        SessionState::Running => "running".to_string(),
        other => other.as_str().to_string(),
    }
}

/// The file name alone. The status row has one line, and the interesting part
/// of a path forty characters long is the end of it.
fn short(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let block = Block::default().title("lazydap").borders(Borders::ALL);
    frame.render_widget(Paragraph::new("no source loaded").block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::Msg;
    use crate::state::Location;
    use crate::testing::render;
    use crate::update::update;
    use lazydap_core::{PauseReason, SessionId};
    use lazydap_protocol::Event;
    use std::path::PathBuf;

    fn loaded(lines: u32) -> AppState {
        let body: Vec<String> = (1..=lines).map(|line| format!("line {line}")).collect();
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/numbers.txt"),
                contents: Ok(body.join("\n")),
            },
        );
        state
    }

    fn status_row(state: &mut AppState) -> String {
        render(46, 5, |frame| view(frame, state))[4].clone()
    }

    #[test]
    fn nothing_loaded_yet_still_draws_a_pane_and_a_status_row() {
        assert_eq!(
            render(34, 4, |frame| view(frame, &mut AppState::default())),
            [
                "┌lazydap─────────────────────────┐",
                "│no source loaded                │",
                "└────────────────────────────────┘",
                // Clipped at the pane width, not wrapped onto a second row.
                "no session · F5 continue · F10 ste",
            ],
        );
    }

    #[test]
    fn a_loaded_file_is_drawn_with_its_line_numbers() {
        let mut state = loaded(3);
        let screen = render(34, 5, |frame| view(frame, &mut state));

        assert_eq!(screen[0], "┌source · /tmp/numbers.txt───────┐");
        assert_eq!(screen[1], "│  1 line 1                      │");
        assert_eq!(screen[2], "│  2 line 2                      │");
    }

    #[test]
    fn a_paused_program_is_drawn_with_a_marker_and_named_in_the_status_row() {
        let mut state = loaded(20);
        state.session = Some(SessionSnapshot {
            id: SessionId::new(),
            state: SessionState::Paused,
            thread_id: Some(1),
            reason: Some(PauseReason::Breakpoint),
        });
        state.location = Some(Location {
            path: PathBuf::from("/tmp/numbers.txt"),
            line: 3,
        });
        // Drawn once before the marker arrives, as the real loop does: the
        // pane learns its height from a draw, and without one it would scroll
        // the marker to the top of a viewport it thinks is zero rows tall.
        render(46, 6, |frame| view(frame, &mut state));
        state.source.as_mut().expect("a loaded file").set_marker(3);

        let screen = render(46, 6, |frame| view(frame, &mut state));

        assert_eq!(screen[3], "│▶  3 line 3                                 │");
        assert_eq!(screen[5], "paused (breakpoint) at numbers.txt:3 · F5 cont");
    }

    #[test]
    fn a_running_program_says_so_and_shows_no_marker() {
        let session_id = SessionId::new();
        let mut state = loaded(20);
        (state, _) = update(
            state,
            Msg::DaemonEvent(Event::Continued {
                session_id,
                thread_id: Some(1),
                all_threads_continued: true,
            }),
        );

        assert!(status_row(&mut state).starts_with("running · "));
    }

    #[test]
    fn joining_a_session_that_was_already_stopped_still_says_where_it_is() {
        // A snapshot says the state, not the story behind it — there is no
        // stop reason to report. Falling back to a bare "paused" would throw
        // away the one thing the user most wants to see.
        let mut state = loaded(20);
        state.session = Some(SessionSnapshot {
            id: SessionId::new(),
            state: SessionState::Paused,
            thread_id: None,
            reason: None,
        });
        state.location = Some(Location {
            path: PathBuf::from("/tmp/numbers.txt"),
            line: 19,
        });

        assert!(status_row(&mut state).starts_with("paused at numbers.txt:19 · "));
    }

    #[test]
    fn a_stop_with_the_stack_still_in_flight_still_says_paused() {
        let mut state = loaded(20);
        (state, _) = update(
            state,
            Msg::DaemonEvent(Event::Stopped {
                session_id: SessionId::new(),
                thread_id: Some(1),
                reason: PauseReason::Entry,
                raw_reason: None,
                all_threads_stopped: true,
                hit_breakpoint_ids: Vec::new(),
            }),
        );

        assert!(status_row(&mut state).starts_with("paused (entry) · "));
    }

    #[test]
    fn a_file_that_would_not_open_says_so_where_the_session_would_be() {
        let (mut state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("no such file".to_string()),
            },
        );

        assert!(status_row(&mut state).starts_with("/tmp/gone.c: no such file · "));
    }
}
