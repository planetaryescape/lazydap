//! Turning a [`Request`] into a [`Response`].
//!
//! One function per request, all of them returning `Result<Response,
//! IpcError>` so a failure reaches the client as a code it can branch on
//! rather than as a closed connection.
//!
//! Split by topic: [`session`] owns the adapter's lifecycle and everything
//! that moves the program, [`inspect`] reads a stopped one, [`breakpoints`]
//! and [`watches`] own the project state that outlives any session.

mod breakpoints;
mod inspect;
mod session;
mod watches;

use crate::state::{DaemonState, Session};
use crate::wait::Abandoned;
use lazydap_core::{SessionId, SessionState};
use lazydap_protocol::{
    DoctorCheck, DoctorReport, ErrorCode, IpcError, LAZYDAP_PROTOCOL_VERSION, Request, Response,
};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, IpcError>;

/// `abandoned` resolves when the client that sent the request hangs up, and is
/// `None` for a caller with no connection behind it. Only the requests that
/// block — a `--wait` — take it, and only the wait loop itself honours it: a
/// half-sent DAP request or a half-applied mutation must never be what gets
/// abandoned (D092).
pub async fn dispatch(
    state: &Arc<DaemonState>,
    request: Request,
    abandoned: Option<Abandoned>,
) -> Result<Response> {
    match request {
        // --- Diagnostics ---
        Request::Ping => Ok(Response::Pong {
            version: LAZYDAP_PROTOCOL_VERSION,
            instance: state.instance.clone(),
            uptime_ms: state.uptime_ms(),
        }),
        Request::Status => Ok(Response::Status(state.status())),
        Request::Version => Ok(Response::Version {
            lazydap: env!("CARGO_PKG_VERSION").to_string(),
            protocol: LAZYDAP_PROTOCOL_VERSION,
        }),
        Request::Doctor {
            check_adapters,
            check_state,
        } => Ok(Response::Doctor(doctor(state, check_adapters, check_state))),
        Request::Shutdown => shutdown(state),

        // --- Session lifecycle ---
        Request::Launch(request) => session::launch(state, request).await,
        Request::Disconnect {
            session_id,
            terminate,
            dry_run,
        } => session::disconnect(state, session_id, terminate, dry_run).await,

        // --- Stepping ---
        Request::Continue {
            session_id,
            thread_id,
            wait,
            all_threads,
        } => {
            session::execute(
                state,
                session_id,
                session::Execution {
                    movement: session::Movement::Continue,
                    thread_id,
                    wait,
                    all_threads,
                },
                abandoned,
            )
            .await
        }
        Request::Step {
            session_id,
            thread_id,
            kind,
            wait,
        } => {
            session::execute(
                state,
                session_id,
                session::Execution {
                    movement: session::Movement::Step(kind),
                    thread_id,
                    wait,
                    all_threads: false,
                },
                abandoned,
            )
            .await
        }
        Request::Pause {
            session_id,
            thread_id,
            wait,
        } => {
            session::execute(
                state,
                session_id,
                session::Execution {
                    movement: session::Movement::Pause,
                    thread_id,
                    wait,
                    all_threads: false,
                },
                abandoned,
            )
            .await
        }

        // --- Inspection ---
        Request::Threads { session_id } => inspect::threads(state, session_id).await,
        Request::StackTrace {
            session_id,
            thread_id,
            start_frame,
            levels,
        } => inspect::stack_trace(state, session_id, thread_id, start_frame, levels).await,
        Request::Scopes {
            session_id,
            frame_id,
        } => inspect::scopes(state, session_id, frame_id).await,
        Request::Variables {
            session_id,
            variables_reference,
            filter,
            start,
            count,
            max,
        } => {
            inspect::variables(
                state,
                session_id,
                variables_reference,
                filter,
                start,
                count,
                max,
            )
            .await
        }
        Request::Eval {
            session_id,
            expression,
            frame_id,
            context,
        } => inspect::eval(state, session_id, &expression, frame_id, context).await,
        Request::Output {
            session_id,
            since_ms,
        } => inspect::output(state, session_id, since_ms),

        // --- Breakpoints ---
        Request::BreakpointList => breakpoints::list(state),
        Request::BreakpointAdd {
            breakpoint,
            dry_run,
        } => breakpoints::add(state, breakpoint, dry_run).await,
        Request::BreakpointRemove { selector, dry_run } => {
            breakpoints::remove(state, selector, dry_run).await
        }
        Request::BreakpointToggle { selector, dry_run } => {
            breakpoints::toggle(state, selector, dry_run).await
        }

        // --- Watches ---
        //
        // None of these are `async`: a watch is never handed to an adapter, so
        // there is nothing to await. What one evaluates to is an ordinary
        // `Request::Eval`, made by whoever wants to know, at a stop.
        Request::WatchList => watches::list(state),
        Request::WatchAdd { watch, dry_run } => watches::add(state, watch, dry_run),
        Request::WatchRemove { selector, dry_run } => watches::remove(state, selector, dry_run),

        // Answered by `server::serve_client`, which is the only thing that
        // has the connection to attach the event stream to. Reaching here
        // means somebody called `dispatch` directly.
        Request::Subscribe { .. } => Err(IpcError::new(
            ErrorCode::DaemonInternalError,
            "subscription is handled on the connection, not by the dispatcher",
        )),
    }
}

/// Stop the daemon.
///
/// There is no dry-run here on purpose: `Request::Shutdown` is frozen as a
/// unit variant (see its doc comment), and a preview changes nothing, so it
/// does not need the daemon's cooperation. `lazydap shutdown --dry-run` is
/// built from a `Status` call on the client.
fn shutdown(state: &Arc<DaemonState>) -> Result<Response> {
    let sessions = state.summaries();
    tracing::info!(target: "daemon.ipc", "shutdown requested by a client");
    state.request_shutdown();
    Ok(Response::ShuttingDown { sessions })
}

/// What is set up and what is not. Writes nothing anywhere (D025).
fn doctor(state: &Arc<DaemonState>, check_adapters: bool, check_state: bool) -> DoctorReport {
    let mut checks = Vec::new();

    if check_adapters {
        // Every adapter lazydap ships, whether or not this machine has it.
        // Reporting only the ones that are installed would make a missing one
        // indistinguishable from one lazydap cannot drive at all, which is the
        // question `doctor` exists to answer.
        //
        // Read off `AdapterKind::ALL` rather than listed here: a literal is
        // the one place the compiler cannot notice a new adapter, and an
        // adapter missing from `doctor` is invisible rather than broken.
        for &kind in lazydap_core::AdapterKind::ALL {
            let name = format!("adapter.{kind}");
            checks.push(match crate::adapter::discover(kind) {
                Ok(path) => DoctorCheck {
                    name,
                    ok: true,
                    detail: path.display().to_string(),
                },
                Err(error) => DoctorCheck {
                    name,
                    ok: false,
                    detail: error.to_string(),
                },
            });
        }
    }

    if check_state {
        let path = state.store.path();
        checks.push(DoctorCheck {
            name: "state.file".to_string(),
            // A state file that does not exist yet is fine — most projects
            // never have one. What is worth reporting is where it would go.
            ok: true,
            detail: format!(
                "{} ({})",
                path.display(),
                if path.exists() {
                    format!("{} breakpoints", state.store.breakpoints().len())
                } else {
                    "not created yet".to_string()
                },
            ),
        });
    }

    checks.push(DoctorCheck {
        name: "daemon".to_string(),
        ok: true,
        detail: format!(
            "instance {}, pid {}, protocol v{LAZYDAP_PROTOCOL_VERSION}",
            state.instance,
            std::process::id(),
        ),
    });

    DoctorReport {
        ok: checks.iter().all(|check| check.ok),
        checks,
    }
}

/// The session a request names, if it is still there.
fn find_session(state: &Arc<DaemonState>, session_id: SessionId) -> Result<Arc<Session>> {
    state.session(session_id).ok_or_else(|| {
        IpcError::new(
            ErrorCode::SessionNotFound,
            format!("no session {session_id}"),
        )
        .with_details(serde_json::json!({ "session_id": session_id.to_string() }))
    })
}

/// The session a request names, refusing one whose program has finished.
///
/// A finished session is still worth having around — its output stays
/// buffered and `status` reports how it ended — but stepping it is asking a
/// process that no longer exists to move.
fn live_session(state: &Arc<DaemonState>, session_id: SessionId) -> Result<Arc<Session>> {
    let session = find_session(state, session_id)?;
    if !session.state().is_live() {
        return Err(session_finished(session_id, session.state()));
    }
    Ok(session)
}

/// The refusal a request gets for a session whose program has finished.
///
/// Shared with `session::execute`, which learns the same fact later: a program
/// can finish between this check and the moment the execution permit is
/// granted, and a caller must not be able to tell the two apart (D089).
pub(super) fn session_finished(session_id: SessionId, state: SessionState) -> IpcError {
    IpcError::new(
        ErrorCode::BadRequest,
        format!(
            "session {session_id} has {}; run `lazydap launch` to start another",
            past_tense(state),
        ),
    )
    .with_details(serde_json::json!({
        "session_id": session_id.to_string(),
        "state": state,
    }))
}

/// The session a request names, refusing one whose program is running.
///
/// The stack, scopes and variables of a running program are undefined — the
/// adapter's answer would describe a moment that has already passed
/// (`docs/blueprint/10-async-to-sync.md`).
fn paused_session(state: &Arc<DaemonState>, session_id: SessionId) -> Result<Arc<Session>> {
    let session = live_session(state, session_id)?;
    if session.state() != SessionState::Paused {
        return Err(IpcError::new(
            ErrorCode::SessionNotPaused,
            format!(
                "session {session_id} is running; pause it first \
                 (`lazydap pause --wait`) or wait for a breakpoint",
            ),
        )
        .with_details(serde_json::json!({
            "session_id": session_id.to_string(),
            "state": session.state(),
        })));
    }
    Ok(session)
}

fn past_tense(state: SessionState) -> &'static str {
    match state {
        SessionState::Exited => "already exited",
        SessionState::AdapterDied => "lost its adapter",
        _ => "already ended",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_store::ProjectStore;

    /// A daemon whose project root is its own temporary directory, so tests
    /// never write a `.lazydap/` into the repository they run from.
    pub(super) fn state() -> Arc<DaemonState> {
        let root = std::env::temp_dir().join(format!(
            "lazydap-handlers-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create the project root");
        DaemonState::new(
            "lazydap-test".to_string(),
            ProjectStore::load(&root).expect("load the store"),
        )
    }

    #[tokio::test]
    async fn ping_answers_with_this_build_s_protocol_version() {
        let state = state();
        let response = dispatch(&state, Request::Ping, None).await.expect("pong");

        match response {
            Response::Pong {
                version, instance, ..
            } => {
                assert_eq!(version, LAZYDAP_PROTOCOL_VERSION);
                assert_eq!(instance, "lazydap-test");
            }
            other => unreachable!("expected a pong, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn status_reports_no_session_before_anything_is_launched() {
        let state = state();
        let response = dispatch(&state, Request::Status, None)
            .await
            .expect("status");

        match response {
            Response::Status(report) => {
                assert!(report.session.is_none());
                assert_eq!(report.protocol_version, LAZYDAP_PROTOCOL_VERSION);
                assert_eq!(report.daemon_pid, std::process::id());
            }
            other => unreachable!("expected a status report, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn subscription_is_not_something_the_dispatcher_can_answer() {
        // It needs the connection to attach the event stream to, which only
        // `server::serve_client` has. Refusing here rather than answering
        // something plausible is what stops a future caller from wiring it up
        // in the one place that cannot deliver on it.
        let state = state();
        let error = dispatch(&state, Request::Subscribe { channels: vec![] }, None)
            .await
            .expect_err("the dispatcher has no connection to subscribe");

        assert_eq!(error.code, ErrorCode::DaemonInternalError, "got: {error}");
    }

    #[tokio::test]
    async fn disconnecting_a_session_that_does_not_exist_is_an_error_not_a_silence() {
        let state = state();
        let error = dispatch(
            &state,
            Request::Disconnect {
                session_id: SessionId::new(),
                terminate: true,
                dry_run: false,
            },
            None,
        )
        .await
        .expect_err("there is no session to disconnect");

        assert_eq!(error.code, ErrorCode::SessionNotFound, "got: {error}");
    }

    #[tokio::test]
    async fn shutdown_is_acknowledged_and_signalled() {
        let state = state();
        assert!(!state.shutdown_requested());

        let response = dispatch(&state, Request::Shutdown, None)
            .await
            .expect("ack");

        assert!(matches!(response, Response::ShuttingDown { .. }));
        assert!(state.shutdown_requested(), "the accept loop must be told");
    }

    #[tokio::test]
    async fn there_is_no_dry_run_shutdown_to_get_wrong() {
        // `Request::Shutdown` is a frozen unit variant — it is the escape
        // hatch for talking to a daemon whose version we do not speak, so it
        // cannot carry flags. `lazydap shutdown --dry-run` is answered from a
        // `Status` call on the client instead, and mutates nothing.
        let state = state();
        let report = match dispatch(&state, Request::Status, None)
            .await
            .expect("status")
        {
            Response::Status(report) => report,
            other => unreachable!("expected a status report, got: {other:?}"),
        };

        assert!(report.session.is_none());
        assert!(
            !state.shutdown_requested(),
            "asking what would happen must not make it happen",
        );
    }

    #[tokio::test]
    async fn version_answers_without_needing_a_session_or_an_adapter() {
        let state = state();
        match dispatch(&state, Request::Version, None)
            .await
            .expect("version")
        {
            Response::Version { lazydap, protocol } => {
                assert_eq!(protocol, LAZYDAP_PROTOCOL_VERSION);
                assert!(!lazydap.is_empty());
            }
            other => unreachable!("expected a version, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn doctor_reports_where_the_state_file_would_go_even_before_there_is_one() {
        let state = state();
        let report = match dispatch(
            &state,
            Request::Doctor {
                check_adapters: false,
                check_state: true,
            },
            None,
        )
        .await
        .expect("doctor")
        {
            Response::Doctor(report) => report,
            other => unreachable!("expected a doctor report, got: {other:?}"),
        };

        let check = report
            .checks
            .iter()
            .find(|check| check.name == "state.file")
            .expect("a state check");
        assert!(check.ok);
        assert!(check.detail.contains("not created yet"), "got: {check:?}");
    }

    #[tokio::test]
    async fn stepping_a_session_that_does_not_exist_names_the_session_not_the_adapter() {
        let state = state();
        let error = dispatch(
            &state,
            Request::Continue {
                session_id: SessionId::new(),
                thread_id: None,
                wait: lazydap_protocol::WaitMode::NoWait,
                all_threads: false,
            },
            None,
        )
        .await
        .expect_err("nothing to continue");

        assert_eq!(error.code, ErrorCode::SessionNotFound, "got: {error}");
    }

    #[tokio::test]
    async fn inspecting_a_session_that_does_not_exist_is_not_a_missing_pause() {
        let state = state();
        let error = dispatch(
            &state,
            Request::Scopes {
                session_id: SessionId::new(),
                frame_id: None,
            },
            None,
        )
        .await
        .expect_err("nothing to inspect");

        assert_eq!(error.code, ErrorCode::SessionNotFound, "got: {error}");
    }
}
