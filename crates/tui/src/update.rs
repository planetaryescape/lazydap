//! The reducer: `(State, Msg) -> (State, Cmd)`.
//!
//! Pure. No I/O, no clock, no terminal, no socket — anything that touches the
//! world is returned as a [`Cmd`] for the loop to run. That is the whole point
//! of D012: every state transition in the TUI is reachable from a test,
//! including the ones a debug session would take minutes to reproduce.

use crate::msg::{Cmd, Msg};
use crate::panes::source::SourceView;
use crate::state::{AppState, Location, SessionSnapshot};
use lazydap_core::{EndReason, SessionState, StackFrame, StepKind};
use lazydap_protocol::{Event, Request, Response, WaitMode};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn update(state: AppState, msg: Msg) -> (AppState, Cmd) {
    match msg {
        // Nothing to store: every draw asks the frame for its own area. The
        // message exists so the loop wakes and repaints now rather than at the
        // next tick, which is the difference between a resize that looks
        // instant and one that looks stuck.
        Msg::Resize | Msg::Tick => (state, Cmd::None),
        Msg::SourceLoaded { path, contents } => source_loaded(state, path, contents),
        // Terminals with the kitty protocol on report releases and repeats as
        // well as presses. Acting on all three turns one keystroke into three.
        Msg::Key(key) if key.kind != KeyEventKind::Press => (state, Cmd::None),
        Msg::Key(key) => key_press(state, key),
        Msg::DaemonEvent(event) => daemon_event(state, event),
        Msg::DaemonResponse { id, response } => {
            tracing::debug!(target: "tui.ipc", id, "the daemon answered");
            daemon_response(state, *response)
        }
        Msg::DaemonFailed { id, error } => {
            tracing::warn!(target: "tui.ipc", id, %error, "the daemon refused a request");
            (with_notice(state, error.message), Cmd::None)
        }
        Msg::DaemonGone => {
            let mut state = with_notice(state, "the daemon went away".to_string());
            state.session = None;
            state.location = None;
            clear_marker(&mut state);
            (state, Cmd::None)
        }
    }
}

fn key_press(mut state: AppState, key: KeyEvent) -> (AppState, Cmd) {
    // Every key clears the pending prefix, including the one that consumes it.
    // `gj` is not `gg`, and must not leave the `g` armed for the next key.
    let awaiting_g = std::mem::take(&mut state.awaiting_g);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return (state, Cmd::Quit),

        // Moving the program. Each of these is the request behind a
        // subcommand: F5 is `lazydap continue`, F10 is `lazydap step`, F11 is
        // `step-in`, shift-F11 is `step-out`. Same daemon, same request, same
        // outcome — which is the rule, not a coincidence (non-negotiable 2).
        KeyCode::F(5) | KeyCode::Char('c') => return execute(state, Movement::Continue),
        KeyCode::F(10) | KeyCode::Char('n') => {
            return execute(state, Movement::Step(StepKind::Over));
        }
        KeyCode::F(11) if shift => return execute(state, Movement::Step(StepKind::Out)),
        KeyCode::F(11) => return execute(state, Movement::Step(StepKind::In)),

        // Moving the view.
        KeyCode::Char('g') if awaiting_g => with_source(&mut state, SourceView::go_to_top),
        KeyCode::Char('g') => state.awaiting_g = true,
        KeyCode::Char('G') => with_source(&mut state, SourceView::go_to_bottom),
        KeyCode::Char('j') | KeyCode::Down => with_source(&mut state, |s| s.move_cursor(1)),
        KeyCode::Char('k') | KeyCode::Up => with_source(&mut state, |s| s.move_cursor(-1)),
        KeyCode::Char('d') if control => with_source(&mut state, |s| s.move_cursor(s.half_page())),
        KeyCode::Char('u') if control => with_source(&mut state, |s| s.move_cursor(-s.half_page())),
        _ => {}
    }

    (state, Cmd::None)
}

/// The ways the TUI can move the program.
enum Movement {
    Continue,
    Step(StepKind),
}

/// Ask the daemon to move the program, if there is one to move.
///
/// Always fire-and-forget. `--wait` exists so a *script* can turn the event
/// stream back into one blocking call; the TUI already has the stream, and a
/// waiting request would freeze the screen for exactly as long as the program
/// runs (`docs/blueprint/10-async-to-sync.md`).
fn execute(mut state: AppState, movement: Movement) -> (AppState, Cmd) {
    let Some(session) = state.session.as_ref().filter(|s| s.state.is_live()) else {
        state.notice = Some("no live session — run `lazydap launch` first".to_string());
        return (state, Cmd::None);
    };

    let request = match movement {
        Movement::Continue => Request::Continue {
            session_id: session.id,
            thread_id: session.thread_id,
            wait: WaitMode::NoWait,
            all_threads: false,
        },
        Movement::Step(kind) => Request::Step {
            session_id: session.id,
            thread_id: session.thread_id,
            kind,
            wait: WaitMode::NoWait,
        },
    };

    // The state stays as it is until the daemon says otherwise. Guessing
    // "running" here would leave the screen lying about a request the adapter
    // went on to refuse.
    state.notice = None;
    (state, Cmd::SendIpc(request))
}

fn daemon_event(mut state: AppState, event: Event) -> (AppState, Cmd) {
    match event {
        Event::SessionStarted { session_id, .. } => {
            state.session = Some(SessionSnapshot {
                id: session_id,
                state: SessionState::Running,
                thread_id: None,
                reason: None,
            });
            state.notice = None;
            (state, Cmd::None)
        }
        Event::Stopped {
            session_id,
            thread_id,
            reason,
            ..
        } => {
            state.session = Some(SessionSnapshot {
                id: session_id,
                state: SessionState::Paused,
                thread_id,
                reason: Some(reason),
            });
            // The event says which thread stopped, not where. One frame is
            // all the source pane needs; the stack pane asks for the rest
            // when it lands (M12).
            (state, top_frame_request(session_id, thread_id))
        }
        Event::Continued {
            session_id,
            thread_id,
            ..
        } => {
            state.session = Some(SessionSnapshot {
                id: session_id,
                state: SessionState::Running,
                thread_id,
                reason: None,
            });
            // Where it *was* stopped is no longer where it is. Leaving the
            // marker would point at a line the program has left.
            state.location = None;
            clear_marker(&mut state);
            (state, Cmd::None)
        }
        Event::SessionEnded { session_id, reason } => {
            state.session = Some(SessionSnapshot {
                id: session_id,
                state: ended_as(&reason),
                thread_id: None,
                reason: None,
            });
            state.location = None;
            clear_marker(&mut state);
            (state, Cmd::None)
        }
        // Subscribed to by nobody yet. Handled explicitly rather than with a
        // catch-all so that adding an event variant is a decision here.
        Event::Output { .. } | Event::BreakpointUpdated { .. } | Event::ThreadChanged { .. } => {
            (state, Cmd::None)
        }
    }
}

fn daemon_response(mut state: AppState, response: Response) -> (AppState, Cmd) {
    match response {
        // The snapshot a `Subscribe` is answered with (D038), and the reason
        // the TUI knows where a program that was already running is stopped.
        Response::Status(report) => {
            state.session = report.session.as_ref().map(SessionSnapshot::from_summary);
            match state.session.as_ref().filter(|s| s.is_paused()) {
                Some(session) => {
                    let cmd = top_frame_request(session.id, session.thread_id);
                    (state, cmd)
                }
                None => (state, Cmd::None),
            }
        }
        Response::StackTrace { frames, .. } => match frames.first().and_then(frame_location) {
            Some(location) => show(state, location),
            None => (state, Cmd::None),
        },
        // Acknowledgements. What actually happened arrives as an event.
        _ => (state, Cmd::None),
    }
}

/// Point the source pane at where the program is.
///
/// Loading the file is a command; marking the line is not. When the file is
/// already open both happen at once, and when it is not the marker is applied
/// by [`source_loaded`] — which is why the location is remembered rather than
/// passed along.
fn show(mut state: AppState, location: Location) -> (AppState, Cmd) {
    let already_open = state
        .source
        .as_ref()
        .is_some_and(|source| source.path() == location.path);

    if already_open {
        let line = location.line;
        with_source(&mut state, |source| source.set_marker(line));
        state.location = Some(location);
        return (state, Cmd::None);
    }

    let path = location.path.clone();
    state.location = Some(location);
    (state, Cmd::LoadSource(path))
}

fn source_loaded(
    mut state: AppState,
    path: std::path::PathBuf,
    contents: std::result::Result<String, String>,
) -> (AppState, Cmd) {
    match contents {
        Ok(contents) => {
            let mut source = SourceView::from_contents(&path, &contents);
            // The daemon may have said where the program is while the file was
            // still being read. Applying it here is what closes that gap.
            if let Some(location) = state.location.as_ref().filter(|l| l.path == path) {
                source.set_marker(location.line);
            }
            state.source = Some(source);
            state.notice = None;
        }
        Err(error) => {
            // Not fatal. The TUI without a file is still a TUI, and telling
            // the user why beats an empty pane they have to guess about.
            tracing::warn!(target: "tui.source", file = %path.display(), %error, "could not open the file");
            state.notice = Some(format!("{}: {error}", path.display()));
        }
    }
    (state, Cmd::None)
}

/// Ask for the top frame, and only the top frame.
fn top_frame_request(session_id: lazydap_core::SessionId, thread_id: Option<i64>) -> Cmd {
    Cmd::SendIpc(Request::StackTrace {
        session_id,
        thread_id,
        start_frame: Some(0),
        levels: Some(1),
    })
}

/// Where a frame is, when it is somewhere the TUI can open.
///
/// A frame with no path is a real thing — inlined code, disassembly, a source
/// the adapter holds in memory — and there is nothing to show for it until the
/// TUI can ask the adapter for the contents.
fn frame_location(frame: &StackFrame) -> Option<Location> {
    let path = frame.source.as_ref()?.path.clone()?;
    Some(Location {
        path,
        line: frame.line,
    })
}

fn ended_as(reason: &EndReason) -> SessionState {
    match reason {
        EndReason::Exited { .. } => SessionState::Exited,
        EndReason::AdapterDied { .. } => SessionState::AdapterDied,
        EndReason::Disconnected | EndReason::Terminated => SessionState::Terminated,
    }
}

fn with_notice(mut state: AppState, notice: String) -> AppState {
    state.notice = Some(notice);
    state
}

fn clear_marker(state: &mut AppState) {
    with_source(state, SourceView::clear_marker);
}

/// Do something to the open file, if there is one.
///
/// The `Option` is checked here rather than in every arm, so a key that moves
/// the cursor is one line whether or not a file happens to be loaded.
fn with_source(state: &mut AppState, action: impl FnOnce(&mut SourceView)) {
    if let Some(source) = state.source.as_mut() {
        action(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::{AdapterKind, PauseReason, SessionId, SourceRef};
    use lazydap_protocol::{ErrorCode, IpcError, SessionSummary, StatusReport};
    use std::path::PathBuf;

    const FILE: &str = "/tmp/numbers.txt";

    fn loaded(lines: u32) -> AppState {
        let body: Vec<String> = (1..=lines).map(|line| format!("line {line}")).collect();
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from(FILE),
                contents: Ok(body.join("\n")),
            },
        );
        state
    }

    /// A state with a session the daemon has told us about, paused.
    fn paused(lines: u32) -> (AppState, SessionId) {
        let session_id = SessionId::new();
        let (state, _) = update(loaded(lines), Msg::DaemonEvent(stopped(session_id)));
        (state, session_id)
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

    fn stack_trace(path: &str, line: u32) -> Response {
        Response::StackTrace {
            frames: vec![StackFrame {
                id: 1,
                name: "main".to_string(),
                source: Some(SourceRef {
                    name: None,
                    path: Some(PathBuf::from(path)),
                    source_reference: None,
                }),
                line,
                column: 1,
            }],
            total: Some(1),
        }
    }

    fn answer(state: AppState, response: Response) -> (AppState, Cmd) {
        update(
            state,
            Msg::DaemonResponse {
                id: 2,
                response: Box::new(response),
            },
        )
    }

    fn press(state: AppState, code: KeyCode) -> (AppState, Cmd) {
        update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn press_control(state: AppState, code: KeyCode) -> (AppState, Cmd) {
        update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::CONTROL)))
    }

    fn cursor(state: &AppState) -> u32 {
        state.source.as_ref().expect("a loaded file").cursor_line
    }

    fn marker(state: &AppState) -> Option<u32> {
        state.source.as_ref().expect("a loaded file").marker_line
    }

    // --- Keys that move the view -------------------------------------------

    #[test]
    fn q_and_escape_ask_the_loop_to_stop() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let (_, cmd) = press(loaded(3), code);
            assert_eq!(cmd, Cmd::Quit, "{code:?}");
        }
    }

    #[test]
    fn j_and_k_and_the_arrows_move_one_line() {
        let mut state = loaded(10);
        for code in [KeyCode::Char('j'), KeyCode::Down] {
            (state, _) = press(state, code);
        }
        assert_eq!(cursor(&state), 3);

        for code in [KeyCode::Char('k'), KeyCode::Up] {
            (state, _) = press(state, code);
        }
        assert_eq!(cursor(&state), 1);
    }

    #[test]
    fn shift_g_goes_to_the_end_and_gg_comes_back() {
        let (state, _) = press(loaded(10), KeyCode::Char('G'));
        assert_eq!(cursor(&state), 10);

        let (state, _) = press(state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 10, "one g on its own moves nothing");
        assert!(state.awaiting_g, "but it does arm the next one");

        let (state, _) = press(state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 1);
        assert!(!state.awaiting_g, "and disarms it again");
    }

    #[test]
    fn a_g_followed_by_anything_else_is_not_a_gg() {
        // Otherwise `gj` leaves the prefix armed and the *next* `g` — pressed
        // for some other reason entirely — jumps to the top.
        let (state, _) = press(loaded(10), KeyCode::Char('G'));
        let (state, _) = press(state, KeyCode::Char('g'));
        let (state, _) = press(state, KeyCode::Char('j'));
        assert!(!state.awaiting_g);

        let (state, _) = press(state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 10, "the j consumed the prefix");
    }

    #[test]
    fn control_d_and_control_u_move_by_half_a_page() {
        // Nothing has been drawn, so half a page is the one-line floor.
        let (state, _) = press_control(loaded(50), KeyCode::Char('d'));
        assert_eq!(cursor(&state), 2);

        let (state, _) = press_control(state, KeyCode::Char('u'));
        assert_eq!(cursor(&state), 1);
    }

    #[test]
    fn d_and_u_without_control_are_not_scrolls() {
        let (state, _) = press(loaded(50), KeyCode::Char('d'));
        let (state, _) = press(state, KeyCode::Char('u'));
        assert_eq!(cursor(&state), 1);
    }

    #[test]
    fn an_unbound_key_changes_nothing_and_asks_for_nothing() {
        let (state, cmd) = press(loaded(10), KeyCode::Char('z'));
        assert_eq!(cursor(&state), 1);
        assert_eq!(cmd, Cmd::None);
    }

    #[test]
    fn a_key_release_is_not_a_key_press() {
        let state = loaded(10);
        let mut release = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;

        let (state, _) = update(state, Msg::Key(release));
        assert_eq!(cursor(&state), 1, "one keystroke is one movement");
    }

    #[test]
    fn scrolling_with_no_file_open_is_a_no_op_rather_than_a_panic() {
        let mut state = AppState::default();
        for code in [KeyCode::Char('j'), KeyCode::Char('G'), KeyCode::Char('g')] {
            (state, _) = press(state, code);
        }
        assert!(state.source.is_none());
    }

    #[test]
    fn a_tick_and_a_resize_change_nothing() {
        let before = loaded(10);
        let (after, cmd) = update(before, Msg::Tick);
        assert_eq!(cmd, Cmd::None);

        let (after, cmd) = update(after, Msg::Resize);
        assert_eq!(cmd, Cmd::None);
        assert_eq!(cursor(&after), 1);
    }

    // --- Keys that move the program ----------------------------------------

    #[test]
    fn f5_and_c_both_send_the_continue_the_cli_sends() {
        for code in [KeyCode::F(5), KeyCode::Char('c')] {
            let (state, session_id) = paused(20);
            let (_, cmd) = press(state, code);

            assert_eq!(
                cmd,
                Cmd::SendIpc(Request::Continue {
                    session_id,
                    thread_id: Some(1),
                    // Never `--wait` from the TUI: the screen would freeze for
                    // as long as the program ran, and the event stream is
                    // already telling us what happened.
                    wait: WaitMode::NoWait,
                    all_threads: false,
                }),
                "{code:?}",
            );
        }
    }

    #[test]
    fn f10_and_n_both_step_over() {
        for code in [KeyCode::F(10), KeyCode::Char('n')] {
            let (state, session_id) = paused(20);
            let (_, cmd) = press(state, code);

            assert_eq!(
                cmd,
                Cmd::SendIpc(Request::Step {
                    session_id,
                    thread_id: Some(1),
                    kind: StepKind::Over,
                    wait: WaitMode::NoWait,
                }),
                "{code:?}",
            );
        }
    }

    #[test]
    fn f11_steps_in_and_shift_f11_steps_out() {
        let (state, session_id) = paused(20);
        let (state, cmd) = update(
            state,
            Msg::Key(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE)),
        );
        assert_eq!(
            cmd,
            Cmd::SendIpc(Request::Step {
                session_id,
                thread_id: Some(1),
                kind: StepKind::In,
                wait: WaitMode::NoWait,
            }),
        );

        let (_, cmd) = update(
            state,
            Msg::Key(KeyEvent::new(KeyCode::F(11), KeyModifiers::SHIFT)),
        );
        assert!(matches!(
            cmd,
            Cmd::SendIpc(Request::Step {
                kind: StepKind::Out,
                ..
            }),
        ));
    }

    #[test]
    fn stepping_with_no_session_says_so_rather_than_doing_nothing() {
        let (state, cmd) = press(loaded(10), KeyCode::F(5));

        assert_eq!(cmd, Cmd::None);
        assert_eq!(
            state.notice.as_deref(),
            Some("no live session — run `lazydap launch` first"),
        );
    }

    #[test]
    fn stepping_a_session_whose_program_has_finished_is_refused_locally() {
        // The daemon would refuse it too, but a round trip to be told "that
        // program is gone" is a round trip for nothing.
        let (state, session_id) = paused(10);
        let (state, _) = update(
            state,
            Msg::DaemonEvent(Event::SessionEnded {
                session_id,
                reason: EndReason::Exited { exit_code: Some(0) },
            }),
        );

        let (state, cmd) = press(state, KeyCode::F(5));
        assert_eq!(cmd, Cmd::None);
        assert!(state.notice.is_some());
    }

    // --- Events driving the marker -----------------------------------------

    #[test]
    fn a_stop_asks_where_the_program_is() {
        let session_id = SessionId::new();
        let (state, cmd) = update(loaded(20), Msg::DaemonEvent(stopped(session_id)));

        assert_eq!(
            cmd,
            Cmd::SendIpc(Request::StackTrace {
                session_id,
                thread_id: Some(1),
                start_frame: Some(0),
                // One frame. The source pane needs the top one; the stack
                // pane asks for the rest when it exists.
                levels: Some(1),
            }),
        );
        let session = state.session.expect("a session");
        assert!(session.is_paused());
        assert_eq!(session.thread_id, Some(1));
        assert_eq!(session.reason, Some(PauseReason::Breakpoint));
    }

    #[test]
    fn the_answer_puts_the_marker_on_the_line_the_program_is_on() {
        let (state, _) = paused(20);
        let (state, cmd) = answer(state, stack_trace(FILE, 19));

        assert_eq!(cmd, Cmd::None, "the file is already open");
        assert_eq!(marker(&state), Some(19));
        assert_eq!(cursor(&state), 19, "and the view follows it");
        assert_eq!(
            state.location,
            Some(Location {
                path: PathBuf::from(FILE),
                line: 19,
            }),
        );
    }

    #[test]
    fn stopping_in_a_file_that_is_not_open_opens_it_first() {
        let (state, _) = paused(20);
        let (state, cmd) = answer(state, stack_trace("/tmp/other.c", 7));

        assert_eq!(cmd, Cmd::LoadSource(PathBuf::from("/tmp/other.c")));
        assert_eq!(
            marker(&state),
            None,
            "the marker waits for the file it belongs to",
        );
    }

    #[test]
    fn the_marker_lands_as_soon_as_the_file_it_belongs_to_has_loaded() {
        // The gap this closes: the daemon says "line 7 of other.c" while
        // other.c is still being read off disk.
        let (state, _) = paused(20);
        let (state, _) = answer(state, stack_trace("/tmp/other.c", 7));
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/other.c"),
                contents: Ok("a\nb\nc\nd\ne\nf\ng\nh".to_string()),
            },
        );

        assert_eq!(marker(&state), Some(7));
    }

    #[test]
    fn a_file_that_arrives_after_the_program_has_moved_on_is_not_marked() {
        // Two stops in quick succession: the first file finishes loading after
        // the second stop has already been reported. Marking it would put an
        // arrow on a line the program left.
        let (state, _) = paused(20);
        let (state, _) = answer(state, stack_trace("/tmp/first.c", 3));
        let (state, _) = answer(state, stack_trace("/tmp/second.c", 9));
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/first.c"),
                contents: Ok("a\nb\nc\nd".to_string()),
            },
        );

        assert_eq!(marker(&state), None);
    }

    #[test]
    fn resuming_takes_the_marker_away() {
        // It says "the program is here". While it runs, it is not.
        let (state, session_id) = paused(20);
        let (state, _) = answer(state, stack_trace(FILE, 19));
        assert_eq!(marker(&state), Some(19));

        let (state, cmd) = update(
            state,
            Msg::DaemonEvent(Event::Continued {
                session_id,
                thread_id: Some(1),
                all_threads_continued: true,
            }),
        );

        assert_eq!(cmd, Cmd::None);
        assert_eq!(marker(&state), None);
        assert!(state.location.is_none());
        assert_eq!(
            state.session.expect("a session").state,
            SessionState::Running
        );
    }

    #[test]
    fn a_frame_with_no_file_behind_it_leaves_the_pane_alone() {
        // Inlined code and disassembly are real frames with nothing to open.
        let (state, _) = paused(20);
        let (state, cmd) = answer(
            state,
            Response::StackTrace {
                frames: vec![StackFrame {
                    id: 1,
                    name: "??".to_string(),
                    source: None,
                    line: 0,
                    column: 0,
                }],
                total: Some(1),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert_eq!(marker(&state), None);
    }

    #[test]
    fn an_empty_stack_is_not_a_reason_to_do_anything() {
        let (state, _) = paused(20);
        let (state, cmd) = answer(
            state,
            Response::StackTrace {
                frames: Vec::new(),
                total: Some(0),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert_eq!(marker(&state), None);
    }

    // --- The session, start to finish --------------------------------------

    #[test]
    fn a_session_starting_elsewhere_is_adopted_without_being_asked_about() {
        // Somebody ran `lazydap launch` in another terminal. The TUI finds out
        // because it subscribed, not because it polled.
        let session_id = SessionId::new();
        let (state, cmd) = update(
            loaded(10),
            Msg::DaemonEvent(Event::SessionStarted {
                session_id,
                adapter: AdapterKind::Codelldb,
            }),
        );

        assert_eq!(cmd, Cmd::None);
        let session = state.session.expect("a session");
        assert_eq!(session.id, session_id);
        assert_eq!(session.state, SessionState::Running);
    }

    #[test]
    fn the_subscription_snapshot_of_a_paused_session_asks_where_it_is() {
        // The TUI started after the program was already stopped. Without this
        // it would show no marker until the program next moved.
        let session_id = SessionId::new();
        let (state, cmd) = answer(loaded(20), Response::Status(status(Some(session_id))));

        assert!(state.session.expect("a session").is_paused());
        assert_eq!(
            cmd,
            Cmd::SendIpc(Request::StackTrace {
                session_id,
                thread_id: None,
                start_frame: Some(0),
                levels: Some(1),
            }),
            "and `None` for the thread means whichever one stopped",
        );
    }

    #[test]
    fn a_snapshot_with_no_session_asks_for_nothing() {
        let (state, cmd) = answer(loaded(10), Response::Status(status(None)));

        assert!(state.session.is_none());
        assert_eq!(cmd, Cmd::None);
    }

    #[test]
    fn a_program_that_finished_takes_the_marker_with_it() {
        let (state, session_id) = paused(20);
        let (state, _) = answer(state, stack_trace(FILE, 19));

        let (state, _) = update(
            state,
            Msg::DaemonEvent(Event::SessionEnded {
                session_id,
                reason: EndReason::Exited { exit_code: Some(0) },
            }),
        );

        assert_eq!(marker(&state), None);
        assert_eq!(
            state.session.expect("a session").state,
            SessionState::Exited
        );
    }

    #[test]
    fn a_refused_request_is_shown_rather_than_swallowed() {
        let (state, _) = update(
            loaded(10),
            Msg::DaemonFailed {
                id: 4,
                error: IpcError::new(ErrorCode::SessionNotPaused, "session is running"),
            },
        );
        assert_eq!(state.notice.as_deref(), Some("session is running"));
    }

    #[test]
    fn a_daemon_that_goes_away_says_so_and_forgets_the_session() {
        let (state, _) = paused(20);
        let (state, _) = answer(state, stack_trace(FILE, 19));

        let (state, cmd) = update(state, Msg::DaemonGone);

        assert_eq!(cmd, Cmd::None);
        assert!(state.session.is_none());
        assert_eq!(marker(&state), None);
        assert_eq!(state.notice.as_deref(), Some("the daemon went away"));
    }

    // --- Loading a file ----------------------------------------------------

    #[test]
    fn a_file_that_would_not_open_is_reported_rather_than_swallowed() {
        let (state, cmd) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("No such file or directory (os error 2)".to_string()),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert!(state.source.is_none());
        assert_eq!(
            state.notice.as_deref(),
            Some("/tmp/gone.c: No such file or directory (os error 2)"),
        );
    }

    #[test]
    fn loading_a_file_clears_the_complaint_about_the_last_one() {
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("no".to_string()),
            },
        );
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/there.c"),
                contents: Ok("int main(void) {}".to_string()),
            },
        );

        assert!(state.notice.is_none());
        assert!(state.source.is_some());
    }

    fn status(session_id: Option<SessionId>) -> StatusReport {
        StatusReport {
            instance: "lazydap-test".to_string(),
            daemon_pid: 1,
            uptime_ms: 0,
            protocol_version: lazydap_protocol::LAZYDAP_PROTOCOL_VERSION,
            lazydap_version: "0.1.0".to_string(),
            session: session_id.map(|session_id| SessionSummary {
                session_id,
                adapter: AdapterKind::Codelldb,
                program: PathBuf::from("/tmp/hello"),
                state: SessionState::Paused,
                exit_code: None,
                buffered_events: 0,
                captured_output_chunks: 0,
                dropped_events: 0,
                uptime_ms: 0,
            }),
        }
    }
}
