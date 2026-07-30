//! Turning a [`Request`] into a [`Response`].
//!
//! One function per request, all of them returning `Result<Response,
//! IpcError>` so a failure reaches the client as a code it can branch on
//! rather than as a closed connection.

use crate::adapter::{self, codelldb};
use crate::state::{DaemonState, Session};
use lazydap_core::{EndReason, SessionId, SessionState};
use lazydap_protocol::{ErrorCode, Event, IpcError, LAZYDAP_PROTOCOL_VERSION, Request, Response};
use std::sync::Arc;

pub type Result<T> = std::result::Result<T, IpcError>;

pub async fn dispatch(state: &Arc<DaemonState>, request: Request) -> Result<Response> {
    match request {
        Request::Ping => Ok(Response::Pong {
            version: LAZYDAP_PROTOCOL_VERSION,
            instance: state.instance.clone(),
            uptime_ms: state.uptime_ms(),
        }),
        Request::Status => Ok(Response::Status(state.status())),
        Request::Shutdown => {
            tracing::info!(target: "daemon.ipc", "shutdown requested by a client");
            state.request_shutdown();
            Ok(Response::ShuttingDown)
        }
        Request::Launch(request) => launch(state, request).await,
        Request::Disconnect {
            session_id,
            terminate,
        } => disconnect(state, session_id, terminate).await,
        Request::Subscribe { .. } => Err(IpcError::new(
            ErrorCode::Unsupported,
            "event subscription lands with the TUI at M11; use `lazydap status` for now",
        )),
    }
}

async fn launch(
    state: &Arc<DaemonState>,
    request: lazydap_protocol::LaunchRequest,
) -> Result<Response> {
    // Claim the slot before spawning anything (D007). Held across the whole
    // launch, and released automatically if any step below fails.
    let session_id = SessionId::new();
    let reservation = state.reserve(session_id)?;

    tracing::info!(
        target: "daemon.session",
        session_id = %session_id,
        program = %request.program.display(),
        stop_on_entry = request.stop_on_entry,
        "launching",
    );

    let launched = codelldb::launch(&request)
        .await
        .map_err(adapter::AdapterError::into_ipc)?;

    let session = Arc::new(Session::new(
        session_id,
        request.adapter,
        request.program,
        launched.state,
        launched.handle,
        state.events(),
    ));

    session.seed_events(
        launched
            .output
            .into_iter()
            .map(|chunk| Event::Output { session_id, chunk })
            .collect(),
    );
    session.emit(Event::SessionStarted {
        session_id,
        adapter: request.adapter,
    });

    // The pump takes over reads from here; nothing else may touch them.
    adapter::spawn_pump(launched.pump, Arc::clone(&session));
    reservation.promote(Arc::clone(&session));

    tracing::info!(
        target: "daemon.session",
        session_id = %session_id,
        state = ?launched.state,
        "launched",
    );

    Ok(Response::Launched {
        session_id,
        state: launched.state,
        reason: launched.reason,
        thread_id: launched.thread_id,
        capabilities: launched.capabilities,
    })
}

async fn disconnect(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    terminate: bool,
) -> Result<Response> {
    let session = state.remove_session(session_id).ok_or_else(|| {
        IpcError::new(
            ErrorCode::SessionNotFound,
            format!("no session {session_id}"),
        )
        .with_details(serde_json::json!({ "session_id": session_id.to_string() }))
    })?;

    // Ask nicely first so the adapter can detach or kill the debuggee as
    // requested, then make sure the process is gone either way. A refused or
    // timed-out disconnect must not leave an adapter behind.
    if session.state().is_live()
        && let Err(error) = session.adapter().disconnect(terminate).await
    {
        tracing::warn!(
            target: "daemon.session",
            session_id = %session_id,
            %error,
            "the adapter did not acknowledge disconnect; killing it",
        );
    }
    session.adapter().kill().await;
    session.end_once(EndReason::Disconnected);
    session.set_state(SessionState::Terminated);

    tracing::info!(target: "daemon.session", session_id = %session_id, terminate, "disconnected");
    Ok(Response::Disconnected { session_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<DaemonState> {
        DaemonState::new("lazydap-test".to_string())
    }

    #[tokio::test]
    async fn ping_answers_with_this_build_s_protocol_version() {
        let state = state();
        let response = dispatch(&state, Request::Ping).await.expect("pong");

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
        let response = dispatch(&state, Request::Status).await.expect("status");

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
    async fn a_request_this_build_does_not_implement_says_so() {
        let state = state();
        let error = dispatch(&state, Request::Subscribe { channels: vec![] })
            .await
            .expect_err("subscription is not implemented until M11");

        assert_eq!(error.code, ErrorCode::Unsupported, "got: {error}");
    }

    #[tokio::test]
    async fn disconnecting_a_session_that_does_not_exist_is_an_error_not_a_silence() {
        let state = state();
        let error = dispatch(
            &state,
            Request::Disconnect {
                session_id: SessionId::new(),
                terminate: true,
            },
        )
        .await
        .expect_err("there is no session to disconnect");

        assert_eq!(error.code, ErrorCode::SessionNotFound, "got: {error}");
    }

    #[tokio::test]
    async fn shutdown_is_acknowledged_and_signalled() {
        let state = state();
        assert!(!state.shutdown_requested());

        let response = dispatch(&state, Request::Shutdown).await.expect("ack");

        assert_eq!(response, Response::ShuttingDown);
        assert!(state.shutdown_requested(), "the accept loop must be told");
    }
}
