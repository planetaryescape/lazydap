//! The reducer: `(State, Msg) -> (State, Cmd)`.
//!
//! Pure. No I/O, no clock, no terminal, no socket — anything that touches the
//! world is returned as a [`Cmd`] for the loop to run. That is the whole point
//! of D012: every state transition in the TUI is reachable from a test,
//! including the ones a debug session would take minutes to reproduce.
//!
//! # Two disciplines worth knowing before changing anything here
//!
//! **Every request is numbered by the reducer** ([`AppState::next_request_id`],
//! D040), because several answers are indistinguishable from each other:
//! `Response::Variables` is a bare list with nothing saying which node asked
//! for it, and a stack trace for the stop before last looks exactly like one
//! for the current stop.
//!
//! **Answers that have been overtaken are dropped, never applied.** The ids
//! make that decidable. It matters more than it sounds: a stack trace names
//! *frame ids*, which the adapter only keeps valid until the program moves, so
//! a late one would fill the panes with handles that no longer address
//! anything.

use crate::msg::{Cmd, Msg};
use crate::panes::source::SourceView;
use crate::state::{AppState, Connection, Focus, Location, PendingExpansion, SessionSnapshot};
use lazydap_core::{
    AdapterBreakpoint, Breakpoint, BreakpointId, BreakpointSelector, BreakpointStatus, EndReason,
    NewBreakpoint, SessionId, SessionState, StackFrame, StepKind, Variable, VariableFilter,
};
use lazydap_protocol::{BreakpointAction, Event, EventKind, Request, Response, WaitMode};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use std::path::PathBuf;

/// What the TUI watches.
///
/// Deliberately not everything. `Output` is the chatty one and there is no
/// pane to put it in until M17; `ThreadChanged` lands with the pane that shows
/// it. A client that subscribed to everything would make the daemon do work
/// nobody reads.
const CHANNELS: [EventKind; 5] = [
    EventKind::SessionStarted,
    EventKind::SessionEnded,
    EventKind::Stopped,
    EventKind::Continued,
    EventKind::BreakpointUpdated,
];

/// How many frames to ask for on each stop.
///
/// Enough for any stack a person reads and few enough that a runaway recursion
/// is truncated rather than paged into the TUI a thousand frames at a time.
/// The adapter reports the real total, so a deeper stack is knowable even
/// though it is not all fetched.
const STACK_LEVELS: u32 = 64;

/// How long to wait before the first reconnection attempt, and the ceiling the
/// doubling stops at (M19).
const RECONNECT_BASE_MS: u64 = 250;
const RECONNECT_MAX_MS: u64 = 4_000;
/// How many doublings it takes to reach [`RECONNECT_MAX_MS`], past which the
/// delay stops changing. See [`backoff`].
const RECONNECT_CEILING_DOUBLINGS: u32 = 5;

pub fn update(state: AppState, msg: Msg) -> (AppState, Cmd) {
    match msg {
        // Nothing to store: every draw asks the frame for its own area. The
        // message exists so the loop wakes and repaints now rather than at the
        // next tick, which is the difference between a resize that looks
        // instant and one that looks stuck.
        Msg::Resize | Msg::Tick => (state, Cmd::None),
        Msg::Connected => connected(state),
        Msg::InputClosed => {
            tracing::warn!(target: "tui.input", "no more input can arrive; leaving");
            (state, Cmd::Quit)
        }
        Msg::SourceLoaded { id, path, contents } => source_loaded(state, id, path, contents),
        // Terminals with the kitty protocol on report releases and repeats as
        // well as presses. Acting on all three turns one keystroke into three.
        Msg::Key(key) if key.kind != KeyEventKind::Press => (state, Cmd::None),
        Msg::Key(key) => key_press(state, key),
        Msg::DaemonEvent(event) => daemon_event(state, event),
        Msg::DaemonResponse { id, response } => {
            tracing::debug!(target: "tui.ipc", id, "the daemon answered");
            daemon_response(state, id, *response)
        }
        Msg::DaemonFailed { id, error } => {
            tracing::warn!(target: "tui.ipc", id, %error, "the daemon refused a request");
            daemon_failed(state, id, error.message)
        }
        Msg::DaemonGone => daemon_gone(state),
        Msg::Reconnected { attempt, outcome } => reconnected(state, attempt, outcome),
    }
}

/// The opening moves on a connection — the first one and every one after it.
///
/// Subscribing is also how the TUI learns what is already going on: the reply
/// is a `Status` snapshot taken at the moment the stream starts (D038), which
/// is what reconciles the screen after a reconnection without a second code
/// path for it.
fn connected(mut state: AppState) -> (AppState, Cmd) {
    state.connection = Connection::Connected;
    let subscribe = send(
        &mut state,
        Request::Subscribe {
            channels: CHANNELS.to_vec(),
        },
    );
    // Breakpoints are project state, not session state: they are worth drawing
    // whether or not anything is running.
    let breakpoints = send(&mut state, Request::BreakpointList);
    (state, Cmd::Batch(vec![subscribe, breakpoints]))
}

/// Number a request and hand back the command that sends it.
///
/// Every request goes through here. That is what makes the ids monotonic, and
/// therefore what makes the staleness checks below mean anything.
fn send(state: &mut AppState, request: Request) -> Cmd {
    let id = state.next_request_id();
    Cmd::SendIpc { id, request }
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

        // Moving between panes.
        KeyCode::Tab => state.focus = state.focus.next(),
        KeyCode::BackTab => state.focus = state.focus.previous(),

        // Toggling a breakpoint on the line under the cursor, which only the
        // source pane has (M14).
        KeyCode::Char('b') if state.focus == Focus::Source => return toggle_breakpoint(state),

        KeyCode::Enter => return enter(state),

        // Moving within the focused pane.
        KeyCode::Char('j') | KeyCode::Down => return (moved(state, 1), Cmd::None),
        KeyCode::Char('k') | KeyCode::Up => return (moved(state, -1), Cmd::None),
        KeyCode::Char('g') if awaiting_g => with_source(&mut state, SourceView::go_to_top),
        KeyCode::Char('g') => state.awaiting_g = true,
        KeyCode::Char('G') => with_source(&mut state, SourceView::go_to_bottom),
        KeyCode::Char('d') if control => with_source(&mut state, |s| s.move_cursor(s.half_page())),
        KeyCode::Char('u') if control => with_source(&mut state, |s| s.move_cursor(-s.half_page())),
        _ => {}
    }

    (state, Cmd::None)
}

/// Move the selection in whichever pane has the keys.
fn moved(mut state: AppState, delta: i32) -> AppState {
    match state.focus {
        Focus::Source => with_source(&mut state, |source| source.move_cursor(delta)),
        Focus::Stack => state.stack.move_selection(delta),
        Focus::Scopes => state.scopes.move_selection(delta),
    }
    state
}

/// `<CR>`, which means something different in each pane.
fn enter(state: AppState) -> (AppState, Cmd) {
    match state.focus {
        // Nothing yet. `b` is the source pane's action.
        Focus::Source => (state, Cmd::None),
        Focus::Stack => select_frame(state),
        Focus::Scopes => expand(state),
    }
}

/// Jump the source pane to the selected frame and fetch its variables (M12).
///
/// Both, because they are the same intention: "show me this frame". Fetching
/// the scopes is what makes the stack pane more than a list of file names —
/// selecting a caller and seeing *its* locals is the thing a `printf` cannot
/// do.
fn select_frame(mut state: AppState) -> (AppState, Cmd) {
    if !state.stack.is_actionable() {
        // These frames are the previous stop's. Their ids address nothing the
        // adapter still recognises, so asking about one earns an error — and
        // the scopes request it carried would supersede the legitimate one the
        // new stop just sent.
        tracing::debug!(target: "tui.ipc", "ignoring a jump to a superseded frame");
        return (state, Cmd::None);
    }
    let Some(frame) = state.stack.selected() else {
        return (state, Cmd::None);
    };
    let frame_id = frame.id;
    let location = frame_location(frame);

    let Some(session) = state.session.as_ref().filter(|s| s.is_paused()) else {
        // The frames are from a program that has since moved on; its ids
        // address nothing. Jumping to the file is still useful and safe.
        return match location {
            Some(location) => show(state, location),
            None => (state, Cmd::None),
        };
    };
    let session_id = session.id;

    let scopes = scopes_request(&mut state, session_id, Some(frame_id));
    match location {
        Some(location) => {
            let (state, jump) = show(state, location);
            match jump {
                // The frame's file is already open, and a batch of one would
                // be a batch for nothing.
                Cmd::None => (state, scopes),
                jump => (state, Cmd::Batch(vec![jump, scopes])),
            }
        }
        // A frame with no file behind it — inlined code, disassembly — still
        // has variables worth showing.
        None => (state, scopes),
    }
}

/// `<CR>` in the scopes pane: open, close, or go and fetch (M13).
fn expand(mut state: AppState) -> (AppState, Cmd) {
    let Some(path) = state.scopes.selected_path() else {
        return (state, Cmd::None);
    };
    if !state.scopes.is_actionable() {
        // The tree belongs to a stop the program has left. Every handle in it
        // addresses nothing, so asking would earn an adapter error and a
        // notice about a row the user cannot do anything about.
        tracing::debug!(target: "tui.ipc", "ignoring an expansion of a superseded tree");
        return (state, Cmd::None);
    }
    let Some(node) = state.scopes.node_at(&path) else {
        return (state, Cmd::None);
    };

    if !node.expandable() {
        return (state, Cmd::None);
    }
    if node.is_pending() {
        // Already asked. Asking again would double the work and leave two
        // replies racing to populate one node.
        return (state, Cmd::None);
    }
    if node.is_loaded() {
        let expanded = node.is_expanded();
        state.scopes.set_expanded(&path, !expanded);
        return (state, Cmd::None);
    }

    // Mutually-referencing pointers are a real shape — a doubly linked list is
    // one — and following them costs a fetch per step until something runs
    // out. Refusing to reopen a handle already open above this row bounds it.
    let reference = node.reference();
    if state.scopes.ancestor_references(&path).contains(&reference) {
        state.notice = Some("already expanded further up — this points back at itself".to_string());
        return (state, Cmd::None);
    }

    if let Some(notice) = unreachable_notice(&state) {
        state.notice = Some(notice);
        return (state, Cmd::None);
    }
    let Some(session) = state.session.as_ref().filter(|s| s.is_paused()) else {
        state.notice = Some("the program is not paused".to_string());
        return (state, Cmd::None);
    };
    let session_id = session.id;

    let id = state.next_request_id();
    // The generation of the tree *on screen*, which between asking for a
    // frame's scopes and being given them is still the previous frame's. Using
    // the newest request's id instead would tag this expansion as belonging to
    // a tree it was never resolved against — which is the whole hazard.
    let scopes = state.scopes.generation();
    state.scopes.mark_pending(&path);
    state
        .pending_variables
        .insert(id, PendingExpansion { scopes, path });
    (
        state,
        Cmd::SendIpc {
            id,
            request: Request::Variables {
                session_id,
                variables_reference: reference,
                filter: VariableFilter::All,
                start: None,
                count: None,
            },
        },
    )
}

/// `b`: add a breakpoint on the cursor line, or take away the one that is
/// already there (M14).
///
/// Add-or-remove rather than the daemon's `BreakpointToggle`, which flips a
/// breakpoint between enabled and disabled. Both are useful and they are
/// different things; `b` is the one an editor's gutter does, and it is built
/// from the same two requests `lazydap break` and `lazydap break --remove`
/// send (non-negotiable 2).
fn toggle_breakpoint(mut state: AppState) -> (AppState, Cmd) {
    let Some(source) = state.source.as_ref() else {
        return (state, Cmd::None);
    };
    let path = source.path().to_path_buf();
    let line = source.cursor_line;

    // Optimism is only safe while there is something to be optimistic about.
    // Flipping the gutter for a request the connection is about to drop on the
    // floor leaves the sign and `.lazydap/state.toml` disagreeing for the rest
    // of the run, with nothing to put them back.
    if let Some(notice) = unreachable_notice(&state) {
        state.notice = Some(notice);
        return (state, Cmd::None);
    }

    // Matched on the line the *gutter* draws, which is where the adapter put
    // the breakpoint rather than where it was asked for. Matching on the
    // persisted line instead made `b` act on something the user cannot see: on
    // a breakpoint moved from 10 to 12, pressing `b` on the visible sign at 12
    // added a second one, and pressing it on blank line 10 took away the sign
    // at 12.
    let held = state
        .breakpoints
        .iter()
        .find(|status| shown_at(status, &path, line));

    // Applied here rather than when the answer comes back, so that holding `b`
    // toggles rather than piling up adds: the second press has to see what the
    // first one asked for. The optimism is bounded — the answer overwrites
    // this, a refusal re-reads the whole list — and an added breakpoint shows
    // as unverified, which is exactly what it is until the adapter says
    // otherwise.
    let request = if let Some(held) = held {
        // The *persisted* line, because that is what the store selects on. The
        // two differ exactly when the adapter moved it.
        let selector = BreakpointSelector::Location {
            source: path.clone(),
            line: held.breakpoint.line,
        };
        state
            .breakpoints
            .retain(|status| !shown_at(status, &path, line));
        Request::BreakpointRemove {
            selector,
            dry_run: false,
        }
    } else {
        state
            .breakpoints
            .push(BreakpointStatus::unverified(Breakpoint {
                // Id `0` is the codebase's "nothing has been allocated yet", the
                // same one a dry-run add reports. The answer brings the real one.
                id: BreakpointId(0),
                source: path.clone(),
                line,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
                enabled: true,
            }));
        Request::BreakpointAdd {
            breakpoint: NewBreakpoint {
                source: path,
                line,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
                enabled: true,
            },
            dry_run: false,
        }
    };

    let id = state.next_request_id();
    state.pending_breakpoints.insert(id);
    (state, Cmd::SendIpc { id, request })
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
    // Before the session check, because a daemon that is not there is why there
    // is no session — and "run `lazydap launch` first" is unhelpful advice when
    // there is nothing to run it against.
    if let Some(notice) = unreachable_notice(&state) {
        state.notice = Some(notice);
        return (state, Cmd::None);
    }
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
    let cmd = send(&mut state, request);
    (state, cmd)
}

fn daemon_event(mut state: AppState, event: Event) -> (AppState, Cmd) {
    // Handled before the session filter below because the two scopes of this
    // event want opposite things from it: a project-scope one has no session to
    // be filtered against.
    if let Event::BreakpointUpdated {
        session_id,
        breakpoint,
    } = &event
    {
        return breakpoint_updated(state, *session_id, breakpoint);
    }

    if !applies(&state, &event) {
        tracing::debug!(
            target: "tui.ipc",
            session_id = ?event.session_id(),
            "ignoring an event that is not about the session being followed",
        );
        return (state, Cmd::None);
    }

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
            let cmd = inspect(&mut state, session_id, thread_id);
            (state, cmd)
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
            // Where it *was* stopped is no longer where it is. Leaving any of
            // this would show the user a position, a call stack and a set of
            // values that the program has left behind.
            forget_position(&mut state);
            (state, Cmd::None)
        }
        Event::SessionEnded { session_id, reason } => {
            state.session = Some(SessionSnapshot {
                id: session_id,
                state: ended_as(&reason),
                thread_id: None,
                reason: None,
            });
            forget_position(&mut state);
            forget_verification(&mut state);
            (state, Cmd::None)
        }
        // `Output` and `ThreadChanged` are subscribed to by nobody yet, and
        // handled explicitly rather than with a catch-all so that adding an
        // event variant is a decision here. `BreakpointUpdated` is listed only
        // to keep this exhaustive — it returned above, before the session
        // filter, because its two scopes want opposite things from one.
        Event::Output { .. } | Event::BreakpointUpdated { .. } | Event::ThreadChanged { .. } => {
            (state, Cmd::None)
        }
    }
}

/// Everything worth asking about a program that has just stopped.
///
/// Both at once rather than the stack first and the scopes after it arrives:
/// the daemon queues requests to one adapter anyway (non-negotiable 6), so
/// waiting for the first answer before asking the second only adds a round
/// trip to every step. `None` for the frame means the top one, which is where
/// the pane starts.
fn inspect(state: &mut AppState, session_id: SessionId, thread_id: Option<i64>) -> Cmd {
    // What is on screen describes the *previous* stop until these answers land.
    // Still worth drawing — clearing it would make both panes blink empty on
    // every step — but not worth acting on, because every frame id and every
    // `variables_reference` in it belongs to a frame the adapter has discarded.
    // Without this, a `<CR>` in the gap sent a dead frame id *and* superseded
    // the legitimate scopes request with one for a frame that no longer exists.
    state.stack.invalidate();
    state.scopes.invalidate();

    let stack = stack_request(state, session_id, thread_id);
    let scopes = scopes_request(state, session_id, None);
    Cmd::Batch(vec![stack, scopes])
}

fn stack_request(state: &mut AppState, session_id: SessionId, thread_id: Option<i64>) -> Cmd {
    let id = state.next_request_id();
    state.latest_stack = id;
    Cmd::SendIpc {
        id,
        request: Request::StackTrace {
            session_id,
            thread_id,
            start_frame: Some(0),
            levels: Some(STACK_LEVELS),
        },
    }
}

fn scopes_request(state: &mut AppState, session_id: SessionId, frame_id: Option<i64>) -> Cmd {
    let id = state.next_request_id();
    state.latest_scopes = id;
    // Deliberately *not* clearing the in-flight expansions here. Clearing at
    // request time was the original hole: an expansion of the tree still on
    // screen, pressed in the gap before the new scopes arrive, is inserted
    // after the clear and survives into a tree it was never about. The
    // generation each one carries is what settles it, on arrival.
    Cmd::SendIpc {
        id,
        request: Request::Scopes {
            session_id,
            frame_id,
        },
    }
}

fn daemon_response(mut state: AppState, id: u64, response: Response) -> (AppState, Cmd) {
    match response {
        // The snapshot a `Subscribe` is answered with (D038), and the reason
        // the TUI knows where a program that was already running is stopped —
        // on the first connection and on every reconnection alike.
        Response::Status(report) => {
            state.session = report.session.as_ref().map(SessionSnapshot::from_summary);
            match state.session.as_ref().filter(|s| s.is_paused()) {
                Some(session) => {
                    let (session_id, thread_id) = (session.id, session.thread_id);
                    let cmd = inspect(&mut state, session_id, thread_id);
                    (state, cmd)
                }
                None => {
                    forget_position(&mut state);
                    (state, Cmd::None)
                }
            }
        }
        Response::StackTrace { frames, .. } => {
            if stale(id, state.latest_stack, "stack") {
                return (state, Cmd::None);
            }
            let top = frames.first().and_then(frame_location);
            state.stack.replace(frames);
            match top {
                Some(location) => show(state, location),
                None => (state, Cmd::None),
            }
        }
        Response::Scopes(scopes) => {
            if stale(id, state.latest_scopes, "scopes") {
                return (state, Cmd::None);
            }
            state.scopes.replace(scopes, id);
            // Anything still in flight was asked about the tree this one just
            // replaced. Dropping the entries now rather than only on arrival
            // keeps the map from holding paths into a tree nobody can see.
            state
                .pending_variables
                .retain(|_, pending| pending.scopes == id);
            (state, Cmd::None)
        }
        Response::Variables(variables) => (variables_arrived(state, id, variables), Cmd::None),
        Response::Breakpoints(report) => {
            state.pending_breakpoints.remove(&id);
            // The TUI never asks for a preview, but applying one would write
            // a change that did not happen.
            if !report.dry_run {
                reconcile(&mut state, report.action, report.breakpoints);
            }
            (state, Cmd::None)
        }
        // Acknowledgements. What actually happened arrives as an event.
        _ => (state, Cmd::None),
    }
}

/// Whether an answer has been overtaken by a newer request of the same kind.
fn stale(id: u64, latest: u64, kind: &'static str) -> bool {
    if id == latest {
        return false;
    }
    tracing::debug!(
        target: "tui.ipc",
        id,
        waiting_for = latest,
        kind,
        "dropping an answer that has been overtaken",
    );
    true
}

/// Put a node's children in place, if the node is still the one that asked.
fn variables_arrived(mut state: AppState, id: u64, variables: Vec<Variable>) -> AppState {
    let Some(pending) = state.pending_variables.remove(&id) else {
        tracing::debug!(target: "tui.ipc", id, "dropping variables nothing is waiting for");
        return state;
    };

    // The generation, not just the path. A path is a *position* in a tree, and
    // the tree is rebuilt whenever the selected frame changes — so an expansion
    // asked about the caller and answered after the callee's scopes arrived
    // would fill the callee's node with the caller's values, at the same
    // position, with nothing on screen to say so.
    if pending.scopes != state.scopes.generation() {
        tracing::debug!(
            target: "tui.ipc",
            id,
            asked_under = pending.scopes,
            showing = state.scopes.generation(),
            "dropping variables belonging to a tree that has been replaced",
        );
        return state;
    }

    state.scopes.populate(&pending.path, variables);
    state
}

/// Fold a breakpoint answer into what the gutter draws.
fn reconcile(state: &mut AppState, action: BreakpointAction, breakpoints: Vec<BreakpointStatus>) {
    match action {
        // The whole truth, so it replaces rather than merges.
        BreakpointAction::Listed => state.breakpoints = breakpoints,
        BreakpointAction::Added | BreakpointAction::Toggled => {
            for status in breakpoints {
                upsert(&mut state.breakpoints, status);
            }
        }
        BreakpointAction::Removed => {
            for status in breakpoints {
                let breakpoint = status.breakpoint;
                state
                    .breakpoints
                    .retain(|held| !at(held, &breakpoint.source, breakpoint.line));
            }
        }
    }
}

/// Replace by *place* rather than by id.
///
/// The optimistic entry `b` leaves behind has no id yet, so matching on id
/// would leave the placeholder next to the real breakpoint and draw two signs
/// on one line.
fn upsert(breakpoints: &mut Vec<BreakpointStatus>, status: BreakpointStatus) {
    let place = breakpoints
        .iter()
        .position(|held| at(held, &status.breakpoint.source, status.breakpoint.line));
    match place {
        Some(index) => breakpoints[index] = status,
        None => breakpoints.push(status),
    }
}

/// Whether a breakpoint *is* the one recorded at this place.
fn at(status: &BreakpointStatus, source: &std::path::Path, line: u32) -> bool {
    status.breakpoint.source == source && status.breakpoint.line == line
}

/// Whether a breakpoint is the one the gutter *draws* at this line.
///
/// The two differ exactly when the adapter moved a breakpoint to the next line
/// with code. Anything the user aims at with the cursor has to go through this
/// one, or `b` acts on a sign that is not where they are looking.
fn shown_at(status: &BreakpointStatus, source: &std::path::Path, line: u32) -> bool {
    status.breakpoint.source == source && status.effective_line() == line
}

/// A breakpoint changed. What that means depends on who is saying so.
fn breakpoint_updated(
    mut state: AppState,
    session_id: Option<SessionId>,
    update: &AdapterBreakpoint,
) -> (AppState, Cmd) {
    let Some(session_id) = session_id else {
        // Project scope: somebody ran `lazydap break`. There is no adapter
        // opinion in one of these, so nothing here is applied — what it means
        // is that the list is not what was last read. Asking is the only honest
        // answer: an add, a removal and a toggle all arrive this way, and only
        // the list itself distinguishes them.
        let cmd = send(&mut state, Request::BreakpointList);
        return (state, cmd);
    };

    // Session scope: an adapter's opinion, true only while its session lives.
    // A queued update from a session that has since ended would otherwise
    // overwrite the live one's — and mark a breakpoint verified on the strength
    // of a program that is no longer running.
    if state.session.as_ref().map(|session| session.id) != Some(session_id) {
        tracing::debug!(
            target: "tui.ipc",
            %session_id,
            "ignoring a breakpoint update from a session that is not being followed",
        );
        return (state, Cmd::None);
    }

    let Some(id) = update.id else {
        // One the daemon could not map back to a breakpoint of ours. Legible in
        // a log, not actionable here.
        tracing::debug!(target: "tui.ipc", "an unmapped breakpoint update");
        return (state, Cmd::None);
    };
    for status in state
        .breakpoints
        .iter_mut()
        .filter(|status| status.breakpoint.id == id)
    {
        status.apply(update);
    }
    (state, Cmd::None)
}

fn daemon_failed(mut state: AppState, id: u64, message: String) -> (AppState, Cmd) {
    state.notice = Some(message);

    if let Some(pending) = state.pending_variables.remove(&id) {
        // Otherwise the row says `⋯` for the rest of the session and pressing
        // `<CR>` on it does nothing, which reads as a dead pane.
        state.scopes.abandon_pending(&pending.path);
        return (state, Cmd::None);
    }
    if state.pending_breakpoints.remove(&id) {
        // The gutter is showing an intention the daemon did not carry out, and
        // there is no way to tell from here which way it went. Ask.
        let cmd = send(&mut state, Request::BreakpointList);
        return (state, cmd);
    }
    (state, Cmd::None)
}

/// The daemon is not there any more (M19).
///
/// Everything about the session goes, because none of it can be checked any
/// more; the breakpoints stay, because they are the project's and outlive both
/// the session and the daemon.
fn daemon_gone(mut state: AppState) -> (AppState, Cmd) {
    // Single-flight. A daemon that dies just after a handshake can produce a
    // second `DaemonGone` while an attempt is already in the air, and starting
    // a fresh ladder for it would leave two climbing at once — each spawning
    // daemons, each handing back a connection, the loser of the race replacing
    // a working connection with an unsubscribed one.
    if let Connection::Reconnecting { attempt } = state.connection {
        tracing::debug!(target: "tui.ipc", attempt, "already reconnecting; not starting again");
        return (state, Cmd::None);
    }

    state.session = None;
    forget_position(&mut state);
    // The adapter that held these opinions is gone with the daemon.
    forget_verification(&mut state);
    state.connection = Connection::Reconnecting { attempt: 1 };
    state.notice = Some("the daemon went away — reconnecting".to_string());
    (
        state,
        Cmd::Reconnect {
            attempt: 1,
            delay_ms: backoff(1),
        },
    )
}

fn reconnected(
    mut state: AppState,
    attempt: u32,
    outcome: std::result::Result<(), String>,
) -> (AppState, Cmd) {
    if !state.is_awaiting(attempt) {
        // An answer from an attempt that has been superseded, or one that
        // arrived after the connection came back another way. Acting on it
        // would start a second ladder.
        tracing::debug!(target: "tui.ipc", attempt, "ignoring a superseded reconnection");
        return (state, Cmd::None);
    }

    match outcome {
        Ok(()) => {
            tracing::info!(target: "tui.ipc", attempt, "reconnected");
            state.notice = None;
            // Exactly the opening moves of the first connection, which is what
            // makes the screen true again: the `Subscribe` reply is a snapshot
            // of whatever has happened in the meantime.
            connected(state)
        }
        Err(error) => {
            let next = attempt + 1;
            tracing::warn!(target: "tui.ipc", attempt, %error, "reconnection failed; trying again");
            state.connection = Connection::Reconnecting { attempt: next };
            (
                state,
                Cmd::Reconnect {
                    attempt: next,
                    delay_ms: backoff(next),
                },
            )
        }
    }
}

/// How long to wait before attempt `n`: doubling to a ceiling, then staying
/// there for as long as the TUI is open.
///
/// It never gives up, and that is the point. Every attempt can *start* a daemon
/// rather than merely wait for one, so there is no failure here that stops
/// being recoverable — a machine that cannot run a daemon this second may well
/// run one the next. Stopping after a fixed count meant six failures inside ten
/// seconds left a live TUI permanently deaf to a daemon that came back a minute
/// later.
///
/// The ceiling is what makes that affordable: retrying at 4s costs nothing and
/// bounds how long the user waits once things recover.
fn backoff(attempt: u32) -> u64 {
    // 250ms << 5 is 8s, already past the ceiling, so five doublings is every
    // distinct delay there is. Capping the shift rather than the result is what
    // keeps a TUI left open overnight from overflowing its own backoff.
    let doublings = attempt.saturating_sub(1).min(RECONNECT_CEILING_DOUBLINGS);
    (RECONNECT_BASE_MS << doublings).min(RECONNECT_MAX_MS)
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
    let cmd = load_source(&mut state, path);
    (state, cmd)
}

/// Ask for a file, and record that this is the read now being waited on.
///
/// Every read goes through here, which is what makes the id monotonic and the
/// staleness check in [`source_loaded`] meaningful.
fn load_source(state: &mut AppState, path: PathBuf) -> Cmd {
    state.latest_load += 1;
    Cmd::LoadSource {
        id: state.latest_load,
        path,
    }
}

fn source_loaded(
    mut state: AppState,
    id: u64,
    path: PathBuf,
    contents: std::result::Result<String, String>,
) -> (AppState, Cmd) {
    // Overtaken. Applying it would put a file the program has already left
    // back on screen — and, when it failed, would replace whatever the status
    // row is currently saying with a complaint about a read nobody is waiting
    // for any more.
    if id != state.latest_load {
        tracing::debug!(
            target: "tui.source",
            id,
            waiting_for = state.latest_load,
            file = %path.display(),
            "dropping a file read that has been overtaken",
        );
        return (state, Cmd::None);
    }

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

/// Whether an event is one this TUI should act on.
///
/// A new session always supersedes whatever was being followed. Everything
/// else has to be about the session being followed *and* about one that is
/// still live, and each half catches a real way the screen goes wrong:
///
/// - **A different id** is an event from a previous adapter that is still
///   dying while a new session has begun. Applied, it would hijack the live
///   session into showing another program's position.
/// - **A session that has ended** cannot stop or resume. A late `stopped` for
///   it would resurrect it: the status row would say "paused" for a program
///   that is gone, and the next F5 would be sent to an adapter that is not
///   there.
/// - **No session known at all** is neither of those. There is nothing to
///   hijack and nothing to resurrect, and an event is a perfectly good way to
///   find out a session exists — so it is adopted rather than dropped, which
///   is what stops a missed announcement from leaving the TUI blind.
fn applies(state: &AppState, event: &Event) -> bool {
    if matches!(event, Event::SessionStarted { .. }) {
        return true;
    }
    match state.session.as_ref() {
        None => true,
        Some(session) => event.session_id() == Some(session.id) && session.state.is_live(),
    }
}

fn ended_as(reason: &EndReason) -> SessionState {
    match reason {
        EndReason::Exited { .. } => SessionState::Exited,
        EndReason::AdapterDied { .. } => SessionState::AdapterDied,
        EndReason::Disconnected | EndReason::Terminated => SessionState::Terminated,
    }
}

/// Forget everything that was only true while the program was stopped.
///
/// The marker, the stack, the variables and any fetch still in flight for
/// them. All four are about one position, so they go together — leaving any
/// one would show the user something the program has moved past.
/// What to say instead of acting, when nothing can reach the daemon.
///
/// `None` when it can. Every key that sends a request goes through this, and
/// the reason is `b`: it flips the gutter before the answer comes back, so
/// pressing it while the connection is down left a sign on screen for a request
/// that was dropped on the floor, and `.lazydap/state.toml` disagreeing with it
/// for the rest of the run.
fn unreachable_notice(state: &AppState) -> Option<String> {
    match state.is_reachable() {
        true => None,
        false => Some("the daemon is not reachable — reconnecting".to_string()),
    }
}

/// Forget what an adapter thought of the breakpoints.
///
/// `verified` and `adapter_line` belong to a session, not to the project: they
/// are one adapter's opinion of a line, and they stop being true the moment
/// that adapter is gone. Keeping them would leave `●` on a program that has
/// exited, and leave the gutter drawing a moved line nothing moved.
fn forget_verification(state: &mut AppState) {
    for status in state.breakpoints.iter_mut() {
        status.verified = false;
        status.adapter_line = None;
        status.message = None;
    }
}

fn forget_position(state: &mut AppState) {
    state.location = None;
    with_source(state, SourceView::clear_marker);
    state.stack.clear();
    state.scopes.clear();
    state.pending_variables.clear();
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
    use crate::state::Connection;
    use lazydap_core::{AdapterKind, PauseReason, Scope, SourceRef};
    use lazydap_protocol::{
        BreakpointAction, BreakpointReport, ErrorCode, IpcError, SessionSummary, StatusReport,
    };
    use std::path::PathBuf;

    const FILE: &str = "/tmp/numbers.txt";

    // --- Building states ---------------------------------------------------

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

    fn frame(id: i64, name: &str, path: &str, line: u32) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: Some(SourceRef {
                name: None,
                path: Some(PathBuf::from(path)),
                source_reference: None,
            }),
            line,
            column: 1,
        }
    }

    fn stack_trace(path: &str, line: u32) -> Response {
        Response::StackTrace {
            frames: vec![frame(1, "main", path, line)],
            total: Some(1),
        }
    }

    fn scope(name: &str, reference: i64) -> Scope {
        Scope {
            name: name.to_string(),
            variables_reference: reference,
            expensive: false,
            named_variables: None,
            indexed_variables: None,
        }
    }

    fn variable(name: &str, value: &str, reference: i64) -> Variable {
        Variable {
            name: name.to_string(),
            value: value.to_string(),
            type_name: Some("int".to_string()),
            variables_reference: reference,
            named_variables: None,
            indexed_variables: None,
        }
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

    fn report(action: BreakpointAction, breakpoints: Vec<BreakpointStatus>) -> Response {
        Response::Breakpoints(BreakpointReport {
            action,
            dry_run: false,
            breakpoints,
            not_found: Vec::new(),
            applied_to_session: false,
        })
    }

    /// An adapter reporting where it actually put one of our breakpoints.
    ///
    /// `line` is where it landed, which is the same line it was asked for
    /// unless the adapter moved it to the next one with code.
    fn adapter_reports(session_id: SessionId, id: u32, line: u32) -> Event {
        Event::BreakpointUpdated {
            session_id: Some(session_id),
            breakpoint: AdapterBreakpoint {
                id: Some(BreakpointId(id)),
                adapter_id: Some(1),
                verified: true,
                line: Some(line),
                message: None,
            },
        }
    }

    /// A reconnection attempt that did not find a daemon.
    fn failed(attempt: u32) -> Msg {
        Msg::Reconnected {
            attempt,
            outcome: Err("no daemon".to_string()),
        }
    }

    /// A file on screen and no daemon behind it.
    fn loaded_disconnected() -> AppState {
        update(loaded(20), Msg::DaemonGone).0
    }

    fn a_breakpoint(id: u32, source: &str, line: u32) -> BreakpointStatus {
        BreakpointStatus::unverified(Breakpoint {
            id: BreakpointId(id),
            source: PathBuf::from(source),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        })
    }

    // --- Driving the reducer -----------------------------------------------

    /// Every request a command asks for, in order. Flattens a batch, so a test
    /// says what it wants to see rather than how deeply it happens to be
    /// nested.
    fn requests(cmd: &Cmd) -> Vec<Request> {
        match cmd {
            Cmd::SendIpc { request, .. } => vec![request.clone()],
            Cmd::Batch(cmds) => cmds.iter().flat_map(requests).collect(),
            _ => Vec::new(),
        }
    }

    fn one_request(cmd: &Cmd) -> Request {
        let mut asked = requests(cmd);
        assert_eq!(asked.len(), 1, "expected exactly one request, got: {cmd:?}");
        asked.remove(0)
    }

    /// The id of a command that sends exactly one request.
    fn request_id(cmd: &Cmd) -> u64 {
        match cmd {
            Cmd::SendIpc { id, .. } => *id,
            other => unreachable!("expected a single request, got: {other:?}"),
        }
    }

    /// The answer to whichever stack trace the state is waiting for.
    fn answer_stack(state: AppState, response: Response) -> (AppState, Cmd) {
        let id = state.latest_stack;
        update(
            state,
            Msg::DaemonResponse {
                id,
                response: Box::new(response),
            },
        )
    }

    fn answer_scopes(state: AppState, scopes: Vec<Scope>) -> (AppState, Cmd) {
        let id = state.latest_scopes;
        update(
            state,
            Msg::DaemonResponse {
                id,
                response: Box::new(Response::Scopes(scopes)),
            },
        )
    }

    /// An answer whose id the reducer does not gate on — a status report, a
    /// breakpoint report.
    fn answer(state: AppState, response: Response) -> (AppState, Cmd) {
        update(
            state,
            Msg::DaemonResponse {
                id: 999,
                response: Box::new(response),
            },
        )
    }

    fn answer_variables(state: AppState, id: u64, variables: Vec<Variable>) -> (AppState, Cmd) {
        update(
            state,
            Msg::DaemonResponse {
                id,
                response: Box::new(Response::Variables(variables)),
            },
        )
    }

    /// The answer to the read the state is currently waiting for.
    fn deliver(state: AppState, path: &str, contents: &str) -> (AppState, Cmd) {
        let id = state.latest_load;
        update(
            state,
            Msg::SourceLoaded {
                id,
                path: PathBuf::from(path),
                contents: Ok(contents.to_string()),
            },
        )
    }

    fn press(state: AppState, code: KeyCode) -> (AppState, Cmd) {
        update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn press_control(state: AppState, code: KeyCode) -> (AppState, Cmd) {
        update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::CONTROL)))
    }

    /// Move the focus to the scopes pane, which is two `Tab`s away.
    fn focus_scopes(state: AppState) -> AppState {
        let (state, _) = press(state, KeyCode::Tab);
        let (state, _) = press(state, KeyCode::Tab);
        state
    }

    fn cursor(state: &AppState) -> u32 {
        state.source.as_ref().expect("a loaded file").cursor_line
    }

    fn marker(state: &AppState) -> Option<u32> {
        state.source.as_ref().expect("a loaded file").marker_line
    }

    /// The scopes pane's visible rows as text, the way the pane draws them.
    fn tree(state: &AppState) -> Vec<String> {
        state
            .scopes
            .rows()
            .iter()
            .map(|row| row.text.clone())
            .collect()
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
                one_request(&cmd),
                Request::Continue {
                    session_id,
                    thread_id: Some(1),
                    // Never `--wait` from the TUI: the screen would freeze for
                    // as long as the program ran, and the event stream is
                    // already telling us what happened.
                    wait: WaitMode::NoWait,
                    all_threads: false,
                },
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
                one_request(&cmd),
                Request::Step {
                    session_id,
                    thread_id: Some(1),
                    kind: StepKind::Over,
                    wait: WaitMode::NoWait,
                },
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
            one_request(&cmd),
            Request::Step {
                session_id,
                thread_id: Some(1),
                kind: StepKind::In,
                wait: WaitMode::NoWait,
            },
        );

        let (_, cmd) = update(
            state,
            Msg::Key(KeyEvent::new(KeyCode::F(11), KeyModifiers::SHIFT)),
        );
        assert!(matches!(
            one_request(&cmd),
            Request::Step {
                kind: StepKind::Out,
                ..
            },
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

    // --- Opening a connection ----------------------------------------------

    #[test]
    fn connecting_subscribes_and_asks_for_the_breakpoints() {
        // Both, because the two are what make the first frame true: the
        // subscription's reply is a snapshot of the session, and the gutter is
        // drawn from project state that exists without one.
        let (state, cmd) = update(AppState::default(), Msg::Connected);

        assert_eq!(
            requests(&cmd),
            vec![
                Request::Subscribe {
                    channels: CHANNELS.to_vec(),
                },
                Request::BreakpointList,
            ],
        );
        assert_eq!(state.connection, Connection::Connected);
    }

    #[test]
    fn every_request_gets_an_id_of_its_own_clear_of_the_handshakes() {
        let (state, cmd) = update(AppState::default(), Msg::Connected);
        let ids: Vec<u64> = match &cmd {
            Cmd::Batch(cmds) => cmds.iter().map(request_id).collect(),
            other => unreachable!("expected a batch, got: {other:?}"),
        };

        assert_eq!(ids, vec![2, 3], "climbing, and clear of the handshake's 1");
        assert_eq!(state.next_request, 3);
    }

    // --- Events driving the panes ------------------------------------------

    #[test]
    fn a_stop_asks_for_both_the_stack_and_the_scopes() {
        // Both at once rather than the scopes after the stack comes back: the
        // daemon queues them anyway, so waiting would add a round trip to
        // every single step.
        let session_id = SessionId::new();
        let (state, cmd) = update(loaded(20), Msg::DaemonEvent(stopped(session_id)));

        assert_eq!(
            requests(&cmd),
            vec![
                Request::StackTrace {
                    session_id,
                    thread_id: Some(1),
                    start_frame: Some(0),
                    levels: Some(STACK_LEVELS),
                },
                Request::Scopes {
                    session_id,
                    // The top frame, which is where the pane starts.
                    frame_id: None,
                },
            ],
        );
        let session = state.session.expect("a session");
        assert!(session.is_paused());
        assert_eq!(session.thread_id, Some(1));
        assert_eq!(session.reason, Some(PauseReason::Breakpoint));
    }

    #[test]
    fn the_answer_puts_the_marker_on_the_line_the_program_is_on() {
        let (state, _) = paused(20);
        let (state, cmd) = answer_stack(state, stack_trace(FILE, 19));

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
        let (state, cmd) = answer_stack(state, stack_trace("/tmp/other.c", 7));

        assert_eq!(
            cmd,
            Cmd::LoadSource {
                id: 1,
                path: PathBuf::from("/tmp/other.c"),
            },
        );
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
        let (state, _) = answer_stack(state, stack_trace("/tmp/other.c", 7));
        let (state, _) = deliver(state, "/tmp/other.c", "a\nb\nc\nd\ne\nf\ng\nh");

        assert_eq!(marker(&state), Some(7));
    }

    #[test]
    fn a_file_that_arrives_after_the_program_has_moved_on_is_not_marked() {
        // Two stops in quick succession: the first file finishes loading after
        // the second stop has already been reported. Marking it would put an
        // arrow on a line the program left.
        let (state, session_id) = paused(20);
        let (state, _) = answer_stack(state, stack_trace("/tmp/first.c", 3));
        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));
        let (state, _) = answer_stack(state, stack_trace("/tmp/second.c", 9));
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                // The read the first stop asked for; the second has since
                // asked for another.
                id: 1,
                path: PathBuf::from("/tmp/first.c"),
                contents: Ok("a\nb\nc\nd".to_string()),
            },
        );

        assert_eq!(marker(&state), None);
    }

    #[test]
    fn resuming_takes_the_marker_and_the_panes_away() {
        // They say "the program is here, and these are its values". While it
        // runs, none of that is true.
        let (state, session_id) = paused(20);
        let (state, _) = answer_stack(state, stack_trace(FILE, 19));
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
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
        assert!(state.stack.selected().is_none());
        assert!(state.scopes.rows().is_empty());
        assert_eq!(
            state.session.expect("a session").state,
            SessionState::Running
        );
    }

    #[test]
    fn a_frame_with_no_file_behind_it_leaves_the_pane_alone() {
        // Inlined code and disassembly are real frames with nothing to open.
        let (state, _) = paused(20);
        let (state, cmd) = answer_stack(
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
        assert!(state.stack.selected().is_some(), "but it is still a frame");
    }

    #[test]
    fn an_empty_stack_is_not_a_reason_to_do_anything() {
        let (state, _) = paused(20);
        let (state, cmd) = answer_stack(
            state,
            Response::StackTrace {
                frames: Vec::new(),
                total: Some(0),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert_eq!(marker(&state), None);
    }

    // --- M12: the stack pane -----------------------------------------------

    #[test]
    fn tab_cycles_the_focus_and_backtab_goes_the_other_way() {
        let mut state = loaded(10);
        for expected in [Focus::Stack, Focus::Scopes, Focus::Source] {
            (state, _) = press(state, KeyCode::Tab);
            assert_eq!(state.focus, expected);
        }
        for expected in [Focus::Scopes, Focus::Stack, Focus::Source] {
            (state, _) = press(state, KeyCode::BackTab);
            assert_eq!(state.focus, expected);
        }
    }

    #[test]
    fn a_stack_trace_fills_the_pane_with_the_frame_the_program_is_in_selected() {
        let (state, _) = paused(20);
        let (state, _) = answer_stack(
            state,
            Response::StackTrace {
                frames: vec![frame(1, "inner", FILE, 5), frame(2, "main", FILE, 19)],
                total: Some(2),
            },
        );

        assert_eq!(state.stack.selected().expect("a frame").name, "inner");
        assert_eq!(marker(&state), Some(5), "and the marker is the top frame's");
    }

    #[test]
    fn j_and_k_move_the_stack_selection_when_the_stack_has_the_keys() {
        let (state, _) = paused(20);
        let (state, _) = answer_stack(
            state,
            Response::StackTrace {
                frames: vec![frame(1, "inner", FILE, 5), frame(2, "main", FILE, 19)],
                total: Some(2),
            },
        );
        let before = cursor(&state);

        let (state, _) = press(state, KeyCode::Tab);
        let (state, _) = press(state, KeyCode::Char('j'));

        assert_eq!(state.stack.selected().expect("a frame").name, "main");
        assert_eq!(
            cursor(&state),
            before,
            "and the source cursor stays where it was",
        );

        let (state, _) = press(state, KeyCode::Char('k'));
        assert_eq!(state.stack.selected().expect("a frame").name, "inner");
    }

    #[test]
    fn enter_on_a_frame_jumps_the_source_pane_and_asks_for_that_frames_variables() {
        let (state, session_id) = paused(20);
        let (state, _) = answer_stack(
            state,
            Response::StackTrace {
                frames: vec![
                    frame(1, "inner", FILE, 5),
                    frame(2, "main", "/tmp/other.c", 19),
                ],
                total: Some(2),
            },
        );

        let (state, _) = press(state, KeyCode::Tab);
        let (state, _) = press(state, KeyCode::Char('j'));
        let (state, cmd) = press(state, KeyCode::Enter);

        assert_eq!(
            requests(&cmd),
            vec![Request::Scopes {
                session_id,
                frame_id: Some(2),
            }],
        );
        assert!(
            matches!(&cmd, Cmd::Batch(cmds) if cmds.iter().any(|cmd| matches!(
                cmd,
                Cmd::LoadSource { path, .. } if path == &PathBuf::from("/tmp/other.c"),
            ))),
            "and it opens the frame's file: {cmd:?}",
        );
        assert_eq!(
            state.location,
            Some(Location {
                path: PathBuf::from("/tmp/other.c"),
                line: 19,
            }),
        );
    }

    #[test]
    fn enter_on_a_frame_already_on_screen_only_asks_for_the_variables() {
        let (state, _) = paused(20);
        let (state, _) = answer_stack(state, stack_trace(FILE, 19));
        let (state, _) = press(state, KeyCode::Tab);
        let (_, cmd) = press(state, KeyCode::Enter);

        assert!(
            matches!(
                cmd,
                Cmd::SendIpc {
                    request: Request::Scopes { .. },
                    ..
                },
            ),
            "a batch of one would be a batch for nothing: {cmd:?}",
        );
    }

    #[test]
    fn enter_on_an_empty_stack_asks_for_nothing() {
        let (state, _) = press(loaded(10), KeyCode::Tab);
        let (_, cmd) = press(state, KeyCode::Enter);
        assert_eq!(cmd, Cmd::None);
    }

    #[test]
    fn a_frame_from_the_previous_stop_cannot_be_jumped_to() {
        // Two hazards in one gap. Between a `stopped` and the trace that
        // answers it, the pane still lists the *previous* stop's frames — whose
        // ids the adapter has discarded. Acting on one sent a dead `frame_id`,
        // and the scopes request that went with it superseded the legitimate
        // one the new stop had just sent, so the pane ended up empty.
        let (state, session_id) = paused(20);
        let (state, _) = answer_stack(
            state,
            Response::StackTrace {
                frames: vec![frame(1, "inner", FILE, 5), frame(2, "main", FILE, 19)],
                total: Some(2),
            },
        );
        let (state, _) = press(state, KeyCode::Tab);

        // The program moves. The new trace has not arrived yet.
        let (state, cmd) = update(state, Msg::DaemonEvent(stopped(session_id)));
        let legitimate_scopes = state.latest_scopes;
        assert!(matches!(cmd, Cmd::Batch(_)));

        let (state, cmd) = press(state, KeyCode::Enter);

        assert_eq!(cmd, Cmd::None, "no request for a frame that is gone");
        assert_eq!(
            state.latest_scopes, legitimate_scopes,
            "and the new stop's scopes request is still the one being waited on",
        );

        // The replacement trace makes the pane live again.
        let (state, _) = answer_stack(state, stack_trace(FILE, 7));
        let (_, cmd) = press(state, KeyCode::Enter);
        assert!(matches!(
            requests(&cmd).first(),
            Some(Request::Scopes { .. }),
        ));
    }

    #[test]
    fn a_scope_from_the_previous_stop_cannot_be_expanded() {
        // The same gap, in the other pane: every `variables_reference` on
        // screen belongs to a frame the adapter has discarded.
        let (state, session_id) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Local", 1000)]);
        let state = focus_scopes(state);

        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));
        let (state, cmd) = press(state, KeyCode::Enter);

        assert_eq!(cmd, Cmd::None);

        let (state, _) = answer_scopes(state, vec![scope("Local", 2000)]);
        let (_, cmd) = press(state, KeyCode::Enter);
        assert!(
            matches!(
                one_request(&cmd),
                Request::Variables {
                    variables_reference: 2000,
                    ..
                },
            ),
            "and the fresh tree is expandable again: {cmd:?}",
        );
    }

    #[test]
    fn a_stack_trace_for_a_stop_the_program_has_left_is_dropped() {
        // Frame ids are only valid until the program moves. Applying a late
        // trace would fill the pane with handles that address nothing, so the
        // next expansion would fail rather than merely show the wrong thing.
        let (state, session_id) = paused(20);
        let first = state.latest_stack;
        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));

        let (state, cmd) = update(
            state,
            Msg::DaemonResponse {
                id: first,
                response: Box::new(Response::StackTrace {
                    frames: vec![frame(1, "gone", "/tmp/stale.c", 3)],
                    total: Some(1),
                }),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert!(state.stack.selected().is_none());
    }

    // --- M13: the scopes pane ----------------------------------------------

    #[test]
    fn the_scopes_answer_fills_the_pane_collapsed() {
        let (state, _) = paused(20);
        let (state, cmd) =
            answer_scopes(state, vec![scope("Locals", 1000), scope("Globals", 1001)]);

        assert_eq!(cmd, Cmd::None, "nothing is fetched until it is opened");
        assert_eq!(tree(&state), ["Locals", "Globals"]);
    }

    #[test]
    fn enter_on_a_scope_asks_for_its_variables_and_the_answer_fills_it_in() {
        let (state, session_id) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);

        let (state, cmd) = press(state, KeyCode::Enter);
        assert_eq!(
            one_request(&cmd),
            Request::Variables {
                session_id,
                variables_reference: 1000,
                filter: VariableFilter::All,
                start: None,
                count: None,
            },
        );

        let (state, _) = answer_variables(
            state,
            request_id(&cmd),
            vec![variable("x", "5", 0), variable("y", "10", 0)],
        );

        assert_eq!(tree(&state), ["Locals", "x = 5 : int", "y = 10 : int"]);
    }

    #[test]
    fn enter_on_an_already_loaded_scope_closes_it_without_asking_again() {
        let (state, _) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);
        let (state, cmd) = press(state, KeyCode::Enter);
        let (state, _) = answer_variables(state, request_id(&cmd), vec![variable("x", "5", 0)]);

        let (state, cmd) = press(state, KeyCode::Enter);
        assert_eq!(cmd, Cmd::None, "the children are already here");
        assert_eq!(tree(&state), ["Locals"], "and it is shut");

        let (state, cmd) = press(state, KeyCode::Enter);
        assert_eq!(cmd, Cmd::None);
        assert_eq!(tree(&state), ["Locals", "x = 5 : int"], "and open again");
    }

    #[test]
    fn enter_on_a_scope_already_being_fetched_does_not_ask_twice() {
        // Two replies racing to populate one node, and the pane flickering
        // between them.
        let (state, _) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);

        let (state, first) = press(state, KeyCode::Enter);
        let (_, second) = press(state, KeyCode::Enter);

        assert!(matches!(first, Cmd::SendIpc { .. }));
        assert_eq!(second, Cmd::None);
    }

    #[test]
    fn enter_on_a_variable_with_no_children_asks_for_nothing() {
        let (state, _) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);
        let (state, cmd) = press(state, KeyCode::Enter);
        let (state, _) = answer_variables(state, request_id(&cmd), vec![variable("x", "5", 0)]);

        let (state, _) = press(state, KeyCode::Char('j'));
        let (_, cmd) = press(state, KeyCode::Enter);

        assert_eq!(cmd, Cmd::None, "a plain int has nothing to open");
    }

    #[test]
    fn variables_for_a_frame_that_has_been_replaced_are_dropped() {
        // The handles they name belong to a frame the program has left.
        let (state, session_id) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);
        let (state, cmd) = press(state, KeyCode::Enter);
        let id = request_id(&cmd);

        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));
        let (state, _) = answer_scopes(state, vec![scope("Locals", 2000)]);
        let (state, _) = answer_variables(state, id, vec![variable("stale", "0", 0)]);

        assert_eq!(tree(&state), ["Locals"], "nothing from the old frame");
        assert!(state.pending_variables.is_empty());
    }

    #[test]
    fn variables_asked_about_one_frame_never_land_in_another_frames_tree() {
        // The interleaving: select the caller (which asks for its scopes),
        // press `<CR>` on the tree still on screen before that answer arrives,
        // then let the two come back in order. Both requests are legitimate and
        // both are answered — but the variables belong to the *callee's* tree,
        // and the path they carry addresses the same position in the caller's.
        // Without the generation they filled the caller's `Local` with the
        // callee's values, at the right position, with nothing to say so.
        let (state, _) = paused(20);
        let (state, _) = answer_stack(
            state,
            Response::StackTrace {
                frames: vec![frame(1, "inner", FILE, 5), frame(2, "main", FILE, 19)],
                total: Some(2),
            },
        );
        let (state, _) = answer_scopes(state, vec![scope("Local", 1000)]);

        // Select the caller, which asks for *its* scopes. Nothing has answered
        // yet, so the tree on screen is still the callee's.
        let (state, _) = press(state, KeyCode::Tab);
        assert_eq!(state.focus, Focus::Stack);
        let (state, _) = press(state, KeyCode::Char('j'));
        let (state, selection) = press(state, KeyCode::Enter);
        assert!(
            requests(&selection)
                .iter()
                .any(|request| matches!(request, Request::Scopes { .. })),
            "the caller's scopes were asked for: {selection:?}",
        );

        // Now `<CR>` on the callee's `Local`, still the tree being drawn. This
        // is the press that used to poison the next tree: it is issued *after*
        // the new scopes were requested, so a clear done at request time had
        // already happened and could not catch it.
        let (state, _) = press(state, KeyCode::Tab);
        assert_eq!(state.focus, Focus::Scopes);
        let (state, expansion) = press(state, KeyCode::Enter);
        let stale_variables = request_id(&expansion);

        // The caller's scopes arrive and replace the tree...
        let (state, _) = answer_scopes(state, vec![scope("Local", 2000)]);
        // ...and only then does the callee's expansion come back.
        let (state, _) = answer_variables(
            state,
            stale_variables,
            vec![variable("callee_only", "99", 0)],
        );

        assert_eq!(
            tree(&state),
            ["Local"],
            "the caller's tree stays shut rather than filling with the callee's values",
        );
        assert!(state.pending_variables.is_empty());
    }

    #[test]
    fn a_scopes_answer_that_has_been_overtaken_is_dropped() {
        let (state, session_id) = paused(20);
        let first = state.latest_scopes;
        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));

        let (state, cmd) = update(
            state,
            Msg::DaemonResponse {
                id: first,
                response: Box::new(Response::Scopes(vec![scope("Stale", 1)])),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert!(state.scopes.rows().is_empty());
    }

    #[test]
    fn a_pointer_that_points_back_at_itself_is_refused_rather_than_followed() {
        // A doubly linked list. Without this, holding <CR> walks the cycle
        // until the fetches or the memory run out.
        let (state, _) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let mut state = focus_scopes(state);

        // Locals -> node -> next, where next's handle is Locals' own.
        for children in [
            vec![variable("node", "0x1", 1002)],
            vec![variable("next", "0x2", 1000)],
        ] {
            let (next, cmd) = press(state, KeyCode::Enter);
            let (next, _) = answer_variables(next, request_id(&cmd), children);
            (state, _) = press(next, KeyCode::Char('j'));
        }

        let (state, cmd) = press(state, KeyCode::Enter);

        assert_eq!(cmd, Cmd::None, "it must not go and fetch it again");
        assert!(
            state
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("points back")),
            "got: {:?}",
            state.notice,
        );
    }

    #[test]
    fn a_variables_request_the_daemon_refused_leaves_the_row_openable_again() {
        let (state, _) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);
        let (state, cmd) = press(state, KeyCode::Enter);

        let (state, cmd) = update(
            state,
            Msg::DaemonFailed {
                id: request_id(&cmd),
                error: IpcError::new(ErrorCode::SessionNotPaused, "session is running"),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert_eq!(state.notice.as_deref(), Some("session is running"));
        let (_, cmd) = press(state, KeyCode::Enter);
        assert!(matches!(cmd, Cmd::SendIpc { .. }), "<CR> retries: {cmd:?}");
    }

    #[test]
    fn expanding_with_no_live_session_says_so_rather_than_asking() {
        let (state, _) = paused(20);
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);
        let state = focus_scopes(state);
        let (state, _) = update(state, Msg::DaemonGone);

        let (state, cmd) = press(state, KeyCode::Enter);

        assert_eq!(cmd, Cmd::None);
        assert!(state.scopes.rows().is_empty(), "and the tree went with it");
    }

    // --- M14: toggling a breakpoint ----------------------------------------

    #[test]
    fn b_on_a_bare_line_adds_the_breakpoint_the_cli_would_add() {
        let (state, _) = press(loaded(20), KeyCode::Char('j'));
        let (state, cmd) = press(state, KeyCode::Char('b'));

        assert_eq!(
            one_request(&cmd),
            Request::BreakpointAdd {
                breakpoint: NewBreakpoint {
                    source: PathBuf::from(FILE),
                    line: 2,
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                    enabled: true,
                },
                dry_run: false,
            },
        );
        assert_eq!(
            state.breakpoints.len(),
            1,
            "and the gutter says so straight away",
        );
        assert!(!state.breakpoints[0].verified, "as unverified, which it is");
    }

    #[test]
    fn b_on_a_line_that_already_has_one_takes_it_away() {
        let (state, _) = answer(
            loaded(20),
            report(BreakpointAction::Listed, vec![a_breakpoint(3, FILE, 1)]),
        );
        let (state, cmd) = press(state, KeyCode::Char('b'));

        assert_eq!(
            one_request(&cmd),
            Request::BreakpointRemove {
                selector: BreakpointSelector::Location {
                    source: PathBuf::from(FILE),
                    line: 1,
                },
                dry_run: false,
            },
        );
        assert!(state.breakpoints.is_empty());
    }

    #[test]
    fn b_pressed_twice_before_either_answer_arrives_toggles_rather_than_adding_twice() {
        // Holding `b` is the obvious way to find out what it does. The second
        // press has to see what the first one asked for, or the gutter piles
        // up adds for one line.
        let (state, first) = press(loaded(20), KeyCode::Char('b'));
        let (state, second) = press(state, KeyCode::Char('b'));

        assert!(matches!(one_request(&first), Request::BreakpointAdd { .. }));
        assert!(matches!(
            one_request(&second),
            Request::BreakpointRemove { .. },
        ));
        assert!(state.breakpoints.is_empty());

        // And the answers, arriving in the order they were asked for, leave it
        // where the daemon is.
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(1, FILE, 1)]),
        );
        let (state, _) = answer(
            state,
            report(BreakpointAction::Removed, vec![a_breakpoint(1, FILE, 1)]),
        );
        assert!(state.breakpoints.is_empty());
    }

    #[test]
    fn the_answer_replaces_the_placeholder_rather_than_sitting_beside_it() {
        // The optimistic entry has no id yet. Matching on id would draw two
        // signs on one line.
        let (state, _) = press(loaded(20), KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(7, FILE, 1)]),
        );

        assert_eq!(state.breakpoints.len(), 1);
        assert_eq!(state.breakpoints[0].breakpoint.id, BreakpointId(7));
    }

    #[test]
    fn an_adapter_verifying_a_breakpoint_mid_toggle_updates_the_sign() {
        // The real sequence: `b`, an `Added` answer that is still unverified,
        // and then the adapter of the session being followed confirming it.
        let (state, session_id) = paused(20);
        let (state, _) = press(state, KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(7, FILE, 1)]),
        );
        assert!(!state.breakpoints[0].verified);

        let (state, cmd) = update(state, Msg::DaemonEvent(adapter_reports(session_id, 7, 1)));

        assert_eq!(cmd, Cmd::None);
        assert!(state.breakpoints[0].verified);
    }

    #[test]
    fn a_breakpoint_update_from_another_session_is_not_applied_to_this_one() {
        // `verified` and the moved line are one adapter's opinion, true only
        // while its session lives. A queued update from a session that has
        // ended would otherwise mark a breakpoint verified on the strength of a
        // program that is no longer running.
        let (state, _) = paused(20);
        let (state, _) = press(state, KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(7, FILE, 1)]),
        );

        let (state, cmd) = update(
            state,
            Msg::DaemonEvent(adapter_reports(SessionId::new(), 7, 1)),
        );

        assert_eq!(cmd, Cmd::None);
        assert!(!state.breakpoints[0].verified);
    }

    #[test]
    fn a_session_ending_takes_its_opinion_of_the_breakpoints_with_it() {
        // Otherwise `●` stays on a program that has exited, and the gutter goes
        // on drawing a moved line that nothing is moving any more.
        let (state, session_id) = paused(20);
        let (state, _) = press(state, KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(7, FILE, 1)]),
        );
        let (state, _) = update(state, Msg::DaemonEvent(adapter_reports(session_id, 7, 4)));
        assert!(state.breakpoints[0].verified);
        assert_eq!(state.breakpoints[0].adapter_line, Some(4));

        let (state, _) = update(
            state,
            Msg::DaemonEvent(Event::SessionEnded {
                session_id,
                reason: EndReason::Exited { exit_code: Some(0) },
            }),
        );

        assert!(!state.breakpoints[0].verified);
        assert_eq!(state.breakpoints[0].adapter_line, None);
        assert_eq!(
            state.breakpoints[0].breakpoint.line, 1,
            "the line the user asked for is still the project's",
        );
    }

    #[test]
    fn a_daemon_going_away_takes_its_opinion_of_the_breakpoints_with_it() {
        let (state, session_id) = paused(20);
        let (state, _) = press(state, KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(7, FILE, 1)]),
        );
        let (state, _) = update(state, Msg::DaemonEvent(adapter_reports(session_id, 7, 1)));

        let (state, _) = update(state, Msg::DaemonGone);

        assert_eq!(state.breakpoints.len(), 1, "the breakpoint itself survives");
        assert!(!state.breakpoints[0].verified, "the opinion of it does not");
    }

    #[test]
    fn a_project_scope_update_asks_for_the_list_rather_than_guessing() {
        // Somebody ran `lazydap break` in another terminal with nothing
        // running. There is no adapter opinion in that event and it could be an
        // add, a removal or a toggle — only the list itself says which.
        let (state, _) = answer(
            loaded(20),
            report(BreakpointAction::Listed, vec![a_breakpoint(1, FILE, 4)]),
        );

        let (state, cmd) = update(
            state,
            Msg::DaemonEvent(Event::BreakpointUpdated {
                session_id: None,
                breakpoint: AdapterBreakpoint {
                    id: Some(BreakpointId(9)),
                    adapter_id: None,
                    verified: false,
                    line: Some(12),
                    message: None,
                },
            }),
        );

        assert_eq!(one_request(&cmd), Request::BreakpointList);
        assert!(
            !state.breakpoints[0].verified,
            "and it applies none of the event's verification fields",
        );
    }

    #[test]
    fn an_update_for_a_breakpoint_that_is_not_ours_is_ignored_rather_than_guessed_at() {
        let (state, session_id) = paused(20);
        let (state, _) = press(state, KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(BreakpointAction::Added, vec![a_breakpoint(7, FILE, 1)]),
        );

        let (state, _) = update(
            state,
            Msg::DaemonEvent(Event::BreakpointUpdated {
                session_id: Some(session_id),
                breakpoint: AdapterBreakpoint {
                    id: None,
                    adapter_id: Some(4),
                    verified: true,
                    line: Some(1),
                    message: None,
                },
            }),
        );

        assert!(!state.breakpoints[0].verified);
    }

    #[test]
    fn b_acts_on_the_sign_the_user_can_see_not_the_line_it_was_typed_on() {
        // codelldb moves a breakpoint on a blank line to the next line with
        // code. The gutter draws it where it landed, so that is where `b` has
        // to find it — matching the persisted line instead meant `b` on the
        // visible sign added a *second* breakpoint.
        let (state, session_id) = paused(20);
        let (state, _) = answer(
            state,
            report(BreakpointAction::Listed, vec![a_breakpoint(3, FILE, 10)]),
        );
        let (state, _) = update(state, Msg::DaemonEvent(adapter_reports(session_id, 3, 12)));
        assert_eq!(state.breakpoints[0].effective_line(), 12);

        // Cursor to the line the sign is drawn on.
        let (state, _) = press(state, KeyCode::Char('g'));
        let (mut state, _) = press(state, KeyCode::Char('g'));
        for _ in 0..11 {
            (state, _) = press(state, KeyCode::Char('j'));
        }
        assert_eq!(cursor(&state), 12);

        let (state, cmd) = press(state, KeyCode::Char('b'));

        assert_eq!(
            one_request(&cmd),
            Request::BreakpointRemove {
                selector: BreakpointSelector::Location {
                    source: PathBuf::from(FILE),
                    // The line the *store* knows it by, which is the one the
                    // user originally typed.
                    line: 10,
                },
                dry_run: false,
            },
        );
        assert!(state.breakpoints.is_empty());
    }

    #[test]
    fn b_on_the_line_a_moved_breakpoint_came_from_does_not_take_it_away() {
        // The other half. Line 10 shows no sign, so `b` there must not remove
        // the one the user can see at 12.
        let (state, session_id) = paused(20);
        let (state, _) = answer(
            state,
            report(BreakpointAction::Listed, vec![a_breakpoint(3, FILE, 10)]),
        );
        let (state, _) = update(state, Msg::DaemonEvent(adapter_reports(session_id, 3, 12)));

        let (state, _) = press(state, KeyCode::Char('g'));
        let (mut state, _) = press(state, KeyCode::Char('g'));
        for _ in 0..9 {
            (state, _) = press(state, KeyCode::Char('j'));
        }
        assert_eq!(cursor(&state), 10);

        let (_, cmd) = press(state, KeyCode::Char('b'));

        assert!(
            matches!(one_request(&cmd), Request::BreakpointAdd { .. }),
            "got: {cmd:?}",
        );
    }

    #[test]
    fn a_breakpoint_request_the_daemon_refused_re_reads_the_whole_list() {
        // The gutter is showing an intention that did not happen, and there is
        // no way to tell from here which way it went.
        let (state, cmd) = press(loaded(20), KeyCode::Char('b'));

        let (state, cmd) = update(
            state,
            Msg::DaemonFailed {
                id: request_id(&cmd),
                error: IpcError::new(
                    ErrorCode::DaemonInternalError,
                    "the state file is read-only",
                ),
            },
        );

        assert_eq!(one_request(&cmd), Request::BreakpointList);
        assert_eq!(state.notice.as_deref(), Some("the state file is read-only"));
    }

    #[test]
    fn a_listing_replaces_what_the_gutter_had_rather_than_merging_into_it() {
        let (state, _) = press(loaded(20), KeyCode::Char('b'));
        let (state, _) = answer(
            state,
            report(
                BreakpointAction::Listed,
                vec![a_breakpoint(4, "/tmp/other.c", 9)],
            ),
        );

        assert_eq!(state.breakpoints.len(), 1);
        assert_eq!(state.breakpoints[0].breakpoint.line, 9);
    }

    #[test]
    fn a_preview_is_never_applied() {
        // The TUI does not ask for one, but writing a change that did not
        // happen is the one thing a dry run must never do.
        let (state, _) = update(
            loaded(20),
            Msg::DaemonResponse {
                id: 9,
                response: Box::new(Response::Breakpoints(BreakpointReport {
                    action: BreakpointAction::Added,
                    dry_run: true,
                    breakpoints: vec![a_breakpoint(1, FILE, 4)],
                    not_found: Vec::new(),
                    applied_to_session: false,
                })),
            },
        );

        assert!(state.breakpoints.is_empty());
    }

    #[test]
    fn b_with_no_file_open_asks_for_nothing() {
        let (state, cmd) = press(AppState::default(), KeyCode::Char('b'));
        assert_eq!(cmd, Cmd::None);
        assert!(state.breakpoints.is_empty());
    }

    #[test]
    fn b_only_means_a_breakpoint_when_the_source_pane_has_the_keys() {
        // In the stack pane there is no cursor line for it to be about.
        let (state, _) = press(loaded(20), KeyCode::Tab);
        let (state, cmd) = press(state, KeyCode::Char('b'));

        assert_eq!(cmd, Cmd::None);
        assert!(state.breakpoints.is_empty());
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
            requests(&cmd),
            vec![
                Request::StackTrace {
                    session_id,
                    thread_id: None,
                    start_frame: Some(0),
                    levels: Some(STACK_LEVELS),
                },
                Request::Scopes {
                    session_id,
                    frame_id: None,
                },
            ],
            "and `None` for the thread means whichever one stopped",
        );
    }

    #[test]
    fn a_snapshot_with_no_session_asks_for_nothing_and_empties_the_panes() {
        let (state, _) = paused(20);
        let (state, _) = answer_stack(state, stack_trace(FILE, 19));

        let (state, cmd) = answer(state, Response::Status(status(None)));

        assert!(state.session.is_none());
        assert_eq!(cmd, Cmd::None);
        assert!(state.stack.selected().is_none());
        assert_eq!(marker(&state), None);
    }

    #[test]
    fn a_program_that_finished_takes_the_marker_with_it() {
        let (state, session_id) = paused(20);
        let (state, _) = answer_stack(state, stack_trace(FILE, 19));

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
    fn a_terminal_that_can_no_longer_be_read_ends_the_tui() {
        // Without this the render loop keeps drawing in raw mode with no key
        // able to reach it — including the one that quits — and the only way
        // out is killing the process from another terminal.
        let (_, cmd) = update(loaded(10), Msg::InputClosed);
        assert_eq!(cmd, Cmd::Quit);
    }

    #[test]
    fn a_stop_arriving_after_its_session_ended_does_not_resurrect_it() {
        // A dying adapter can emit one last `stopped`. Applied, the status row
        // would say "paused" for a program that is gone, and the next F5 would
        // be sent to an adapter that is not there.
        let (state, session_id) = paused(20);
        let (state, _) = update(
            state,
            Msg::DaemonEvent(Event::SessionEnded {
                session_id,
                reason: EndReason::Exited { exit_code: Some(0) },
            }),
        );

        let (state, cmd) = update(state, Msg::DaemonEvent(stopped(session_id)));

        assert_eq!(cmd, Cmd::None, "and it must not go asking for a stack");
        assert_eq!(
            state.session.expect("a session").state,
            SessionState::Exited
        );
    }

    #[test]
    fn an_event_from_a_previous_session_does_not_hijack_the_current_one() {
        let (state, old_session) = paused(20);
        let new_session = SessionId::new();
        let (state, _) = update(
            state,
            Msg::DaemonEvent(Event::SessionStarted {
                session_id: new_session,
                adapter: AdapterKind::Codelldb,
            }),
        );

        // The previous adapter, still shutting down, reports a stop.
        let (state, cmd) = update(state, Msg::DaemonEvent(stopped(old_session)));

        assert_eq!(cmd, Cmd::None);
        let session = state.session.expect("a session");
        assert_eq!(session.id, new_session, "the live session is untouched");
        assert_eq!(
            session.state,
            SessionState::Running,
            "and is not dragged into looking paused by another program's stop",
        );
    }

    #[test]
    fn an_event_for_a_session_nobody_has_heard_of_is_adopted_rather_than_dropped() {
        // The other side of the identity check. With nothing being followed
        // there is nothing to hijack, and refusing the event would leave the
        // TUI blind until the program next moved.
        let session_id = SessionId::new();
        let (state, cmd) = update(loaded(20), Msg::DaemonEvent(stopped(session_id)));

        assert!(matches!(
            requests(&cmd).first(),
            Some(Request::StackTrace { .. }),
        ));
        assert_eq!(state.session.expect("a session").id, session_id);
    }

    #[test]
    fn the_newest_file_read_wins_however_the_answers_come_back() {
        // Reads finish in whatever order the filesystem manages. The first
        // stop's file arriving *after* the second stop has been reported must
        // not put a file the program has left back on screen.
        let (state, session_id) = paused(20);
        let (state, first) = answer_stack(state, stack_trace("/tmp/first.c", 3));
        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));
        let (state, second) = answer_stack(state, stack_trace("/tmp/second.c", 9));

        let (first, second) = match (first, second) {
            (Cmd::LoadSource { id: first, .. }, Cmd::LoadSource { id: second, .. }) => {
                (first, second)
            }
            other => unreachable!("both stops should ask for a file, got: {other:?}"),
        };
        assert!(second > first, "ids climb, so the newer one is knowable");

        // Newest first, oldest second — the order that used to lose.
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                id: second,
                path: PathBuf::from("/tmp/second.c"),
                contents: Ok("a\nb\nc\nd\ne\nf\ng\nh\ni\nj".to_string()),
            },
        );
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                id: first,
                path: PathBuf::from("/tmp/first.c"),
                contents: Ok("a\nb\nc\nd".to_string()),
            },
        );

        assert_eq!(
            state.source.as_ref().expect("a file").path(),
            PathBuf::from("/tmp/second.c"),
        );
        assert_eq!(marker(&state), Some(9));
    }

    #[test]
    fn an_overtaken_read_that_failed_does_not_overwrite_what_the_status_row_says() {
        // The failure branch of the same race: a stale read reporting "no such
        // file" would replace a notice the user has not read yet.
        let (state, session_id) = paused(20);
        let (state, _) = answer_stack(state, stack_trace("/tmp/first.c", 3));
        let (state, _) = update(state, Msg::DaemonEvent(stopped(session_id)));
        let (state, _) = answer_stack(state, stack_trace("/tmp/second.c", 9));
        let (state, _) = update(
            state,
            Msg::DaemonFailed {
                id: 4,
                error: IpcError::new(ErrorCode::SessionNotPaused, "session is running"),
            },
        );

        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                id: 1,
                path: PathBuf::from("/tmp/first.c"),
                contents: Err("No such file or directory (os error 2)".to_string()),
            },
        );

        assert_eq!(state.notice.as_deref(), Some("session is running"));
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

    // --- M19: losing the daemon and getting it back ------------------------

    #[test]
    fn a_daemon_that_goes_away_forgets_the_session_and_starts_reconnecting() {
        let (state, _) = paused(20);
        let (state, _) = answer_stack(state, stack_trace(FILE, 19));
        let (state, _) = answer_scopes(state, vec![scope("Locals", 1000)]);

        let (state, cmd) = update(state, Msg::DaemonGone);

        assert_eq!(
            cmd,
            Cmd::Reconnect {
                attempt: 1,
                delay_ms: 250,
            },
        );
        assert_eq!(state.connection, Connection::Reconnecting { attempt: 1 });
        assert!(state.session.is_none());
        assert_eq!(marker(&state), None);
        assert!(state.stack.selected().is_none());
        assert!(state.scopes.rows().is_empty());
    }

    #[test]
    fn a_second_hang_up_while_an_attempt_is_in_flight_does_not_start_a_second_ladder() {
        // A daemon that dies just after a handshake produces one of these.
        // Two ladders would each spawn daemons and each hand back a
        // connection, and whichever lost would replace a working connection
        // with one nobody is subscribed on.
        let (state, _) = update(loaded(20), Msg::DaemonGone);
        let (state, cmd) = update(state, Msg::DaemonGone);

        assert_eq!(cmd, Cmd::None);
        assert_eq!(state.connection, Connection::Reconnecting { attempt: 1 });
    }

    #[test]
    fn the_breakpoints_survive_the_daemon_that_was_holding_them() {
        // They are the project's, recorded in `.lazydap/state.toml`. Clearing
        // the gutter would suggest they had been lost, which they have not.
        let (state, _) = answer(
            loaded(20),
            report(BreakpointAction::Listed, vec![a_breakpoint(1, FILE, 4)]),
        );
        let (state, _) = update(state, Msg::DaemonGone);

        assert_eq!(state.breakpoints.len(), 1);
    }

    #[test]
    fn a_failed_attempt_backs_off_and_keeps_trying_for_as_long_as_the_tui_is_open() {
        // It must never give up. Every attempt can *start* a daemon rather
        // than only wait for one, so a machine that could not run one a second
        // ago may well run one now — and a fixed count meant six failures
        // inside ten seconds left a live TUI deaf to a daemon that came back a
        // minute later.
        let (mut state, _) = update(loaded(20), Msg::DaemonGone);

        for (index, delay_ms) in [500, 1_000, 2_000, 4_000, 4_000, 4_000]
            .into_iter()
            .enumerate()
        {
            let attempt = index as u32 + 1;
            let (next, cmd) = update(state, failed(attempt));
            state = next;
            assert_eq!(
                cmd,
                Cmd::Reconnect {
                    attempt: attempt + 1,
                    delay_ms,
                },
                "attempt {}",
                attempt + 1,
            );
        }

        // And a hundred failures later it is still going, still at the ceiling.
        for attempt in 7..107 {
            let (next, cmd) = update(state, failed(attempt));
            state = next;
            assert_eq!(
                cmd,
                Cmd::Reconnect {
                    attempt: attempt + 1,
                    delay_ms: 4_000,
                },
            );
        }
        assert_eq!(
            state.connection,
            Connection::Reconnecting { attempt: 107 },
            "still trying, which is the whole point",
        );
    }

    #[test]
    fn the_backoff_never_overflows_however_long_the_tui_is_left_open() {
        assert_eq!(backoff(1), 250);
        assert_eq!(backoff(2), 500);
        assert_eq!(backoff(6), 4_000);
        assert_eq!(backoff(u32::MAX), 4_000);
    }

    #[test]
    fn an_answer_from_a_superseded_attempt_is_ignored() {
        // Otherwise a late reply from an attempt that has already failed and
        // been replaced starts a second ladder alongside the current one.
        let (state, _) = update(loaded(20), Msg::DaemonGone);
        let (state, _) = update(state, failed(1));
        assert_eq!(state.connection, Connection::Reconnecting { attempt: 2 });

        let (state, cmd) = update(state, failed(1));

        assert_eq!(cmd, Cmd::None);
        assert_eq!(
            state.connection,
            Connection::Reconnecting { attempt: 2 },
            "the ladder is where it was",
        );
    }

    #[test]
    fn a_success_from_a_superseded_attempt_does_not_declare_the_tui_connected() {
        // The screen would say connected while the loop had thrown that
        // connection away for being stale, leaving every key silently dropped.
        let (state, _) = update(loaded(20), Msg::DaemonGone);
        let (state, _) = update(state, failed(1));

        let (state, cmd) = update(
            state,
            Msg::Reconnected {
                attempt: 1,
                outcome: Ok(()),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert_eq!(state.connection, Connection::Reconnecting { attempt: 2 });
    }

    #[test]
    fn getting_the_daemon_back_replays_the_opening_moves() {
        // Which is what makes the screen true again: the subscription's reply
        // is a snapshot of everything that happened while it was away.
        let (state, _) = update(loaded(20), Msg::DaemonGone);
        let (state, cmd) = update(
            state,
            Msg::Reconnected {
                attempt: 1,
                outcome: Ok(()),
            },
        );

        assert_eq!(state.connection, Connection::Connected);
        assert!(state.notice.is_none());
        assert_eq!(
            requests(&cmd),
            vec![
                Request::Subscribe {
                    channels: CHANNELS.to_vec(),
                },
                Request::BreakpointList,
            ],
        );
    }

    #[test]
    fn the_snapshot_after_a_reconnection_picks_the_session_back_up() {
        // A program launched from another terminal while the daemon was being
        // restarted. The TUI has to find it, not wait for it to move.
        let session_id = SessionId::new();
        let (state, _) = update(loaded(20), Msg::DaemonGone);
        let (state, _) = update(
            state,
            Msg::Reconnected {
                attempt: 1,
                outcome: Ok(()),
            },
        );
        let (state, cmd) = answer(state, Response::Status(status(Some(session_id))));

        assert_eq!(state.session.expect("a session").id, session_id);
        assert!(matches!(
            requests(&cmd).first(),
            Some(Request::StackTrace { .. }),
        ));
    }

    #[test]
    fn keys_that_need_the_daemon_say_so_rather_than_pretending_while_it_is_away() {
        // `b` is the one that mattered: it flips the gutter before the answer
        // comes back, so pressing it while the connection was down left a sign
        // on screen for a request that went nowhere — and `.lazydap/state.toml`
        // disagreeing with it for the rest of the run, with nothing to put them
        // back.
        let (state, _) = update(loaded(20), Msg::DaemonGone);

        let (state, cmd) = press(state, KeyCode::Char('b'));
        assert_eq!(cmd, Cmd::None);
        assert!(state.breakpoints.is_empty(), "and the gutter did not move");
        assert_eq!(
            state.notice.as_deref(),
            Some("the daemon is not reachable — reconnecting"),
        );

        // The same for everything else that has to reach the daemon.
        for code in [
            KeyCode::F(5),
            KeyCode::Char('c'),
            KeyCode::F(10),
            KeyCode::Char('n'),
            KeyCode::F(11),
        ] {
            let (state, cmd) = press(loaded_disconnected(), code);
            assert_eq!(cmd, Cmd::None, "{code:?}");
            assert_eq!(
                state.notice.as_deref(),
                Some("the daemon is not reachable — reconnecting"),
                "{code:?}",
            );
        }
    }

    #[test]
    fn quitting_still_works_while_the_daemon_is_away() {
        // The one key that must never depend on the daemon.
        let (state, _) = update(loaded(20), Msg::DaemonGone);
        let (_, cmd) = press(state, KeyCode::Char('q'));
        assert_eq!(cmd, Cmd::Quit);
    }

    // --- Loading a file ----------------------------------------------------

    #[test]
    fn a_file_that_would_not_open_is_reported_rather_than_swallowed() {
        let (state, cmd) = update(
            AppState::default(),
            Msg::SourceLoaded {
                id: 0,
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
                id: 0,
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("no".to_string()),
            },
        );
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                id: 0,
                path: PathBuf::from("/tmp/there.c"),
                contents: Ok("int main(void) {}".to_string()),
            },
        );

        assert!(state.notice.is_none());
        assert!(state.source.is_some());
    }
}
