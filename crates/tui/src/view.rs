//! Drawing the state. Reads it; does not decide anything about it.
//!
//! `&mut AppState` rather than `&AppState` for one reason, and it is worth
//! knowing: [`SourceView::render`] learns the pane's height as it draws, and
//! that height is what "keep the cursor on screen" and "half a page" are
//! measured in. Computing it outside the layout would mean duplicating the
//! layout. Nothing else here writes to the state (D012, M10's notes).

use crate::state::{AppState, Connection, Focus, Modal, SessionSnapshot};
use lazydap_core::SessionState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// The keys the status row advertises. Short on purpose: a help pane is still
/// unbuilt, and a row that lists everything is a row nobody reads.
const KEYS: &str = "F5 continue · F10 step · b break · a watch · Tab pane · q quit";

/// The keys the status row advertises instead while the REPL has the cursor.
///
/// A different list because the usual one is a lie in there: `q` is a `q`, and
/// somebody who tabbed in needs to be told how to get out before they need to
/// know how to quit.
const REPL_KEYS: &str = "<CR> eval · / adapter command · ^P/^N history · Esc leave";

/// How the width is split. The source pane is the one being read; the panes on
/// the right are being glanced at.
const SOURCE_SHARE: u16 = 65;
const SIDEBAR_SHARE: u16 = 35;

/// How the left column is split between the file and the REPL.
///
/// The REPL is always present rather than appearing when focused. A pane that
/// opens on demand moves everything else on screen at the moment somebody is
/// about to read it, and the scrollback is worth seeing while stepping — it is
/// the record of what you have already asked.
const FILE_SHARE: u16 = 72;
const REPL_SHARE: u16 = 28;

pub fn view(frame: &mut Frame, state: &mut AppState) {
    // Panes above, one status row that is always exactly one line tall.
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());
    let [left, right] = Layout::horizontal([
        Constraint::Percentage(SOURCE_SHARE),
        Constraint::Percentage(SIDEBAR_SHARE),
    ])
    .areas(body);
    let [file, repl] = Layout::vertical([
        Constraint::Percentage(FILE_SHARE),
        Constraint::Percentage(REPL_SHARE),
    ])
    .areas(left);
    // Stack, then scopes, then watches: the frame you pick decides what the two
    // panes below it are showing, so reading downwards follows the causality.
    let [stack, scopes, watches] = Layout::vertical([
        Constraint::Percentage(30),
        Constraint::Percentage(40),
        Constraint::Percentage(30),
    ])
    .areas(right);

    match state.source.as_mut() {
        Some(source) => source.render(
            frame,
            file,
            &state.breakpoints,
            state.focus == Focus::Source,
        ),
        None => render_empty(frame, file, state.focus == Focus::Source),
    }
    state
        .stack
        .render(frame, stack, state.focus == Focus::Stack);
    state
        .scopes
        .render(frame, scopes, state.focus == Focus::Scopes);
    state
        .watches
        .render(frame, watches, state.focus == Focus::Watches);
    state.repl.render(frame, repl, state.focus == Focus::Repl);

    frame.render_widget(
        Paragraph::new(format!("{} · {}", status_text(state), keys(state)))
            .style(Style::default().fg(Color::DarkGray)),
        status,
    );

    // Last, so it is drawn over everything. A prompt that owns the keyboard
    // and is hidden behind a pane is the worst of both.
    if let Some(modal) = state.modal.as_ref() {
        render_modal(frame, body, modal);
    }
}

/// Which key list the status row shows.
fn keys(state: &AppState) -> &'static str {
    match state.focus.is_typing() {
        true => REPL_KEYS,
        false => KEYS,
    }
}

/// A prompt, centred over the panes (M16).
///
/// [`Clear`] first, because ratatui draws over whatever is underneath rather
/// than replacing it — without it the pane's own text shows through the gaps in
/// the box.
fn render_modal(frame: &mut Frame, body: Rect, modal: &Modal) {
    let Modal::AddWatch(input) = modal;

    let [area] = Layout::horizontal([Constraint::Percentage(70)])
        .flex(Flex::Center)
        .areas(body);
    let [area] = Layout::vertical([Constraint::Length(3)])
        .flex(Flex::Center)
        .areas(area);

    let block = Block::default()
        .title("watch expression")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(format!("{}█", input.as_str())).block(block),
        area,
    );
}

/// The left-hand end of the status row: what the daemon is doing, or what went
/// wrong. A notice wins, because it is the thing the user has not seen yet.
fn status_text(state: &AppState) -> String {
    // A connection that is not there outranks everything, including a notice:
    // every other thing the row could say is about a daemon it cannot reach,
    // and would read as current.
    match &state.connection {
        Connection::Reconnecting { attempt } => {
            return format!("reconnecting… (attempt {attempt})");
        }
        Connection::Connected => {}
    }
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
                text.push_str(&format!(
                    " at {}:{}",
                    file_name(&location.path),
                    location.line,
                ));
            }
            text
        }
        SessionState::Running => "running".to_string(),
        other => other.as_str().to_string(),
    }
}

/// The file name alone. The status row has one line, and the interesting part
/// of a path forty characters long is the end of it.
fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn render_empty(frame: &mut Frame, area: Rect, focused: bool) {
    let block = Block::default()
        .title("lazydap")
        .borders(Borders::ALL)
        .border_style(crate::panes::border_style(focused));
    frame.render_widget(Paragraph::new("no source loaded").block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::Msg;
    use crate::state::{Connection, Location};
    use crate::testing::render;
    use crate::update::update;
    use lazydap_core::{
        Breakpoint, BreakpointId, BreakpointStatus, PauseReason, Scope, SessionId, SourceRef,
        StackFrame, Variable,
    };
    use lazydap_protocol::{Event, Response};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    const FILE: &str = "/tmp/numbers.txt";

    /// Wide and tall enough that all five panes have room to say something.
    /// Smaller than this and the snapshots test the truncation rather than the
    /// layout.
    const WIDTH: u16 = 64;
    const HEIGHT: u16 = 16;

    fn loaded(lines: u32) -> AppState {
        let body: Vec<String> = (1..=lines).map(|line| format!("line {line}")).collect();
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                id: 0,
                path: PathBuf::from(FILE),
                contents: Ok(body.join("\n")),
            },
        );
        state
    }

    fn screen(state: &mut AppState) -> Vec<String> {
        render(WIDTH, HEIGHT, |frame| view(frame, state))
    }

    fn status_row(state: &mut AppState) -> String {
        render(46, 5, |frame| view(frame, state))[4].clone()
    }

    fn frame(id: i64, name: &str, line: u32) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: Some(SourceRef {
                name: None,
                path: Some(PathBuf::from(FILE)),
                source_reference: None,
            }),
            line,
            column: 1,
        }
    }

    fn breakpoint(line: u32, verified: bool) -> BreakpointStatus {
        BreakpointStatus {
            breakpoint: Breakpoint {
                id: BreakpointId(1),
                source: PathBuf::from(FILE),
                line,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
                enabled: true,
            },
            verified,
            adapter_line: None,
            message: None,
        }
    }

    /// A program stopped on a breakpoint with its stack and its locals in,
    /// which is the screen the whole of Phase D is for.
    fn at_a_breakpoint() -> AppState {
        let session_id = SessionId::new();
        // Drawn once before anything arrives, as the real loop does: the panes
        // learn their height from a draw, and without one the marker would
        // scroll to the top of a viewport they think is zero rows tall.
        let mut state = loaded(20);
        screen(&mut state);
        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));

        let stack_id = state.latest_stack;
        let (state, _) = update(
            state,
            Msg::DaemonResponse {
                id: stack_id,
                response: Box::new(Response::StackTrace {
                    frames: vec![frame(1, "main", 19), frame(2, "_start", 3)],
                    total: Some(2),
                }),
            },
        );

        let scopes_id = state.latest_scopes;
        let (state, _) = update(
            state,
            Msg::DaemonResponse {
                id: scopes_id,
                response: Box::new(Response::Scopes(vec![Scope {
                    name: "Locals".to_string(),
                    variables_reference: 1000,
                    expensive: false,
                    named_variables: None,
                    indexed_variables: None,
                }])),
            },
        );

        // Expand Locals, as `Tab Tab <CR>` does.
        let (state, _) = update(state, Msg::Key(key(KeyCode::Tab)));
        let (state, _) = update(state, Msg::Key(key(KeyCode::Tab)));
        let (state, cmd) = update(state, Msg::Key(key(KeyCode::Enter)));
        let id = match cmd {
            crate::msg::Cmd::SendIpc { id, .. } => id,
            other => unreachable!("expected a variables request, got: {other:?}"),
        };
        let (mut state, _) = update(
            state,
            Msg::DaemonResponse {
                id,
                response: Box::new(Response::Variables(vec![Variable {
                    name: "x".to_string(),
                    value: "5".to_string(),
                    type_name: Some("int".to_string()),
                    variables_reference: 0,
                    named_variables: None,
                    indexed_variables: None,
                }])),
            },
        );
        state.breakpoints = vec![breakpoint(19, true)];
        state
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn stopped(session_id: SessionId) -> Event {
        Event::Stopped {
            session_id,
            thread_id: Some(1),
            reason: PauseReason::Breakpoint,
            raw_reason: None,
            all_threads_stopped: true,
            hit_breakpoint_ids: Vec::new(),
        }
    }

    #[test]
    fn nothing_loaded_yet_still_draws_every_pane_and_a_status_row() {
        assert_eq!(
            render(34, 5, |frame| view(frame, &mut AppState::default())),
            [
                "┌lazydap─────────────┐┌stack─────┐",
                // Too short for most of these panes to have an inside. They
                // still draw, which is the point: no arithmetic underflows.
                "│no source loaded    │┌scopes────┐",
                "└────────────────────┘└──────────┘",
                "┌repl────────────────┐┌watches───┐",
                // Clipped at the terminal width, not wrapped onto a second row.
                "no session · F5 continue · F10 ste",
            ],
        );
    }

    #[test]
    fn a_loaded_file_is_drawn_with_its_line_numbers() {
        let mut state = loaded(3);
        let drawn = screen(&mut state);

        assert!(
            drawn[0].starts_with("┌source · /tmp/numbers.txt"),
            "got: {}",
            drawn[0],
        );
        assert!(drawn[1].starts_with("│   1 line 1"), "got: {}", drawn[1]);
        assert!(drawn[2].starts_with("│   2 line 2"), "got: {}", drawn[2]);
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
            path: PathBuf::from(FILE),
            line: 3,
        });
        // Drawn once before the marker arrives, as the real loop does: the
        // pane learns its height from a draw, and without one it would scroll
        // the marker to the top of a viewport it thinks is zero rows tall.
        screen(&mut state);
        state.source.as_mut().expect("a loaded file").set_marker(3);

        let drawn = screen(&mut state);

        assert!(drawn[3].starts_with("│ ▶  3 line 3"), "got: {}", drawn[3]);
        // The last row, always: the status row is the one thing the layout
        // pins to a fixed height.
        let status = drawn.last().expect("a status row");
        assert!(
            status.starts_with("paused (breakpoint) at numbers.txt:3 · "),
            "got: {status}",
        );
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
            path: PathBuf::from(FILE),
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
                id: 0,
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("no such file".to_string()),
            },
        );

        assert!(status_row(&mut state).starts_with("/tmp/gone.c: no such file · "));
    }

    // --- The Phase D screen ------------------------------------------------

    #[test]
    fn a_program_stopped_on_a_breakpoint_shows_its_stack_and_its_locals() {
        // The whole of Phase D and E in one frame: the marker on line 19, a
        // breakpoint sign beside it, two frames in the stack pane, the value of
        // `x` in the scopes pane, and the two panes M16 and M17 added.
        let mut state = at_a_breakpoint();

        assert_eq!(
            screen(&mut state),
            [
                "┌source · /tmp/numbers.txt───────────────┐┌stack───────────────┐",
                "│   11 line 11                           ││numbers.txt:19 main │",
                "│   12 line 12                           ││numbers.txt:3 _start│",
                "│   13 line 13                           ││                    │",
                "│   14 line 14                           │└────────────────────┘",
                "│   15 line 15                           │┌scopes──────────────┐",
                "│   16 line 16                           ││▾ Locals            │",
                "│   17 line 17                           ││    x = 5 : int     │",
                "│   18 line 18                           ││                    │",
                "│●▶ 19 line 19                           ││                    │",
                "└────────────────────────────────────────┘└────────────────────┘",
                "┌repl────────────────────────────────────┐┌watches─────────────┐",
                "│>                                       ││no watches — a to ad│",
                "│                                        ││                    │",
                "└────────────────────────────────────────┘└────────────────────┘",
                "paused (breakpoint) at numbers.txt:19 · F5 continue · F10 step ·",
            ],
        );
    }

    #[test]
    fn the_gutter_sign_is_there_only_while_the_breakpoint_is() {
        let mut state = at_a_breakpoint();
        let with = screen(&mut state);
        assert!(with[9].starts_with("│●▶ 19 line 19"), "got: {}", with[9]);

        state.breakpoints.clear();
        let without = screen(&mut state);
        assert!(
            without[9].starts_with("│ ▶ 19 line 19"),
            "the column stays so nothing shifts sideways, got: {}",
            without[9],
        );
    }

    #[test]
    fn a_daemon_that_went_away_says_reconnecting_rather_than_anything_older() {
        // Everything else the row could say is about a daemon it cannot reach,
        // and would read as current.
        let (mut state, _) = update(at_a_breakpoint(), Msg::DaemonGone);

        assert!(
            status_row(&mut state).starts_with("reconnecting… (attempt 1) · "),
            "got: {}",
            status_row(&mut state),
        );

        // However many attempts in. There is no giving-up state to fall into:
        // every attempt can start a daemon, so it keeps counting for as long as
        // the user leaves the TUI open.
        state.connection = Connection::Reconnecting { attempt: 4 };
        assert!(status_row(&mut state).starts_with("reconnecting… (attempt 4) · "));

        state.connection = Connection::Reconnecting { attempt: 400 };
        assert!(status_row(&mut state).starts_with("reconnecting… (attempt 400) · "));
    }

    #[test]
    fn the_focused_pane_is_the_one_the_keys_are_going_to() {
        // Only the border colour changes, which a symbol comparison cannot
        // see — so this checks the state the border is drawn from instead,
        // and that drawing every focus is safe.
        let mut state = at_a_breakpoint();
        for focus in [Focus::Source, Focus::Stack, Focus::Scopes] {
            state.focus = focus;
            assert_eq!(screen(&mut state).len(), HEIGHT as usize);
        }
    }

    #[test]
    fn the_status_row_advertises_the_keys_phase_d_added() {
        let mut state = loaded(3);
        let drawn = render(80, 6, |frame| view(frame, &mut state));
        assert!(drawn[5].contains("b break"), "got: {}", drawn[5]);
        assert!(drawn[5].contains("Tab pane"), "got: {}", drawn[5]);
    }

    #[test]
    fn the_panes_are_still_drawn_when_the_terminal_is_too_small_for_them() {
        // A terminal nobody would use, and the one that finds the arithmetic
        // that underflows.
        for (width, height) in [(1, 1), (4, 2), (10, 3), (20, 4)] {
            let mut state = at_a_breakpoint();
            assert_eq!(
                render(width, height, |frame| view(frame, &mut state)).len(),
                height as usize,
                "{width}x{height}",
            );
        }
    }

    // --- M16 and M17 on screen ---------------------------------------------

    /// The breakpoint screen, plus two watches answered and a REPL exchange.
    fn with_watches_and_repl() -> AppState {
        let mut state = at_a_breakpoint();

        state.watches.replace(vec![
            lazydap_core::Watch {
                id: lazydap_core::WatchId(1),
                expression: "counter".to_string(),
                label: None,
            },
            lazydap_core::Watch {
                id: lazydap_core::WatchId(2),
                expression: "tokens[pos]".to_string(),
                label: None,
            },
        ]);
        let round = state.watches.begin_round();
        state.watches.record(
            round,
            lazydap_core::WatchId(1),
            lazydap_core::WatchValue::Value(lazydap_core::EvalResult {
                value: "42".to_string(),
                type_name: Some("int".to_string()),
                variables_reference: 0,
            }),
        );
        state.watches.record(
            round,
            lazydap_core::WatchId(2),
            lazydap_core::WatchValue::Error("out of scope".to_string()),
        );

        for c in "x + 1".chars() {
            state.repl.push_char(c);
        }
        let (entry, _) = state.repl.submit().expect("a submission");
        state.repl.answer(
            entry,
            crate::panes::repl::ReplOutput::Value(lazydap_core::EvalResult {
                value: "6".to_string(),
                type_name: Some("int".to_string()),
                variables_reference: 0,
            }),
        );
        state
    }

    /// A terminal with room for the REPL to show an exchange *and* its prompt.
    ///
    /// At [`HEIGHT`] the REPL gets two rows inside its border, which is enough
    /// to draw but not enough to demonstrate the scrollback.
    fn tall_screen(state: &mut AppState) -> Vec<String> {
        render(WIDTH, 20, |frame| view(frame, state))
    }

    #[test]
    fn the_watches_pane_shows_a_value_and_an_error_side_by_side() {
        // Both halves of M16 in one frame: an expression that evaluated, and
        // one that did not. The errored row stays — the same expression is
        // usually back in scope a few steps later.
        let mut state = with_watches_and_repl();
        let drawn = tall_screen(&mut state);

        let value = drawn
            .iter()
            .find(|row| row.contains("counter = 42 : int"))
            .expect("the answered watch");
        assert!(value.ends_with('│'), "inside the pane: {value}");

        assert!(
            drawn.iter().any(|row| row.contains("tokens[pos] = out of")),
            "and the refused one keeps its row: {drawn:?}",
        );
    }

    #[test]
    fn the_repl_shows_what_was_asked_and_what_came_back_above_the_prompt() {
        let mut state = with_watches_and_repl();
        state.focus = Focus::Repl;
        let drawn = tall_screen(&mut state);

        let asked = drawn
            .iter()
            .position(|row| row.contains("> x + 1"))
            .expect("the line that was typed");
        assert!(
            drawn[asked + 1].contains("6 : int"),
            "the answer sits under it: {}",
            drawn[asked + 1],
        );
        assert!(
            drawn[asked + 2].contains("> █"),
            "and the prompt is below that, with a cursor because the pane has \
             the keys: {}",
            drawn[asked + 2],
        );
    }

    #[test]
    fn the_status_row_says_how_to_get_out_while_the_repl_has_the_keys() {
        // The usual list is a lie in there: `q` is a character, so somebody who
        // tabbed in needs to be told the way out before the way to quit.
        let mut state = with_watches_and_repl();
        state.focus = Focus::Repl;

        let drawn = render(120, 6, |frame| view(frame, &mut state));
        let status = drawn.last().expect("a status row");
        assert!(status.contains("Esc leave"), "got: {status}");
        assert!(status.contains("/ adapter command"), "got: {status}");
        assert!(!status.contains("q quit"), "got: {status}");
    }

    #[test]
    fn an_open_prompt_is_drawn_over_the_panes_rather_than_behind_them() {
        // `Clear` first, or the pane's own text shows through the box.
        let mut state = with_watches_and_repl();
        state.modal = Some(crate::state::Modal::AddWatch(
            crate::panes::input::TextInput::new("tokens[pos]"),
        ));

        let drawn = screen(&mut state);
        let box_row = drawn
            .iter()
            .find(|row| row.contains("watch expression"))
            .expect("the prompt is drawn");
        assert!(box_row.contains('┌'), "got: {box_row}");

        let typed = drawn
            .iter()
            .find(|row| row.contains("tokens[pos]█"))
            .expect("what has been typed, with a cursor");
        assert!(
            !typed.contains("line 1"),
            "the file behind it must not show through: {typed}",
        );
    }

    #[test]
    fn a_request_the_reducer_makes_from_a_draw_is_not_a_thing() {
        // The view decides nothing (D012). Drawing twice must leave the state
        // saying exactly what it said before.
        let mut state = at_a_breakpoint();
        screen(&mut state);
        let before = state.next_request;
        screen(&mut state);

        assert_eq!(state.next_request, before);
        assert!(matches!(
            update(at_a_breakpoint(), Msg::Tick).1,
            crate::msg::Cmd::None,
        ));
    }
}
