//! The session read pump.
//!
//! One task per session owns the adapter's read half and does nothing else
//! with it. That is not a style choice: `DapReader::read_incoming` is not
//! cancellation-safe, so a read that is ever raced against a timer or another
//! `select!` arm can be dropped mid-frame and desynchronise the stream for
//! good. Owning reads in a task that only reads makes that impossible by
//! construction, and everything else — request waiters, event fan-out — is
//! reached through channels.

use super::Pending;
use super::codelldb::{PumpStart, output_chunk};
use crate::state::Session;
use lazydap_core::{EndReason, PauseReason, SessionState};
use lazydap_dap::{DapEvent, DapReader, Incoming, TransportError};
use lazydap_protocol::Event;
use std::sync::Arc;

/// Start pumping. The task ends when the adapter does.
pub fn spawn_pump(start: PumpStart, session: Arc<Session>) {
    tokio::spawn(run(start.reader, start.pending, session));
}

async fn run(mut reader: DapReader, pending: Pending, session: Arc<Session>) {
    loop {
        match reader.read_incoming().await {
            Ok(Incoming::Response(response)) => {
                let waiter = pending.lock().await.remove(&response.request_seq);
                match waiter {
                    Some(sender) => {
                        // A dropped receiver means the requester gave up
                        // first; that is its business, not ours.
                        let _ = sender.send(response);
                    }
                    None => tracing::warn!(
                        target: "daemon.session",
                        session_id = %session.id,
                        request_seq = response.request_seq,
                        command = response.command,
                        "response for a request nobody is waiting on",
                    ),
                }
            }
            Ok(Incoming::Event(event)) => handle_event(&session, event),
            Err(error) => {
                finish(&session, &error);
                break;
            }
        }
    }

    // Nobody will ever answer the outstanding requests. Dropping the senders
    // wakes their waiters with `AdapterError::Gone` rather than leaving them
    // to time out one by one.
    pending.lock().await.clear();
    session.adapter().kill().await;

    tracing::debug!(target: "daemon.session", session_id = %session.id, "pump stopped");
}

fn handle_event(session: &Arc<Session>, event: DapEvent) {
    let session_id = session.id;
    let body = event.body.clone().unwrap_or_default();

    match event.event.as_str() {
        "output" => {
            if let Some(chunk) = output_chunk(&event) {
                session.emit(Event::Output { session_id, chunk });
            }
        }
        "stopped" => {
            let reason = PauseReason::from(body["reason"].as_str().unwrap_or("unknown"));
            let thread_id = body["threadId"].as_i64();
            session.set_state(SessionState::Paused);
            tracing::debug!(
                target: "daemon.session",
                session_id = %session_id,
                reason = %reason,
                thread_id,
                "paused",
            );
            session.emit(Event::Stopped {
                session_id,
                thread_id,
                reason,
                all_threads_stopped: body["allThreadsStopped"].as_bool().unwrap_or(false),
            });
        }
        "continued" => {
            session.set_state(SessionState::Running);
            session.emit(Event::Continued {
                session_id,
                thread_id: body["threadId"].as_i64(),
                all_threads_continued: body["allThreadsContinued"].as_bool().unwrap_or(false),
            });
        }
        // `exited` carries the debuggee's status; `terminated` is the session
        // ending. They arrive in that order, so recording the code first means
        // the ending can report it.
        "exited" => session.set_exit_code(body["exitCode"].as_i64().map(|code| code as i32)),
        "terminated" => {
            let reason = match session.exit_code() {
                Some(exit_code) => EndReason::Exited {
                    exit_code: Some(exit_code),
                },
                None => EndReason::Terminated,
            };
            session.end_once(reason);
        }
        other => tracing::trace!(
            target: "daemon.session",
            session_id = %session_id,
            event = other,
            "unhandled adapter event",
        ),
    }
}

/// The read side died. Make sure clients hear about it (D022).
///
/// Adapters do not reliably say goodbye — a crashed one just closes the
/// socket, and a UI waiting for `terminated` waits forever. So an EOF that
/// arrives before the session ended properly becomes a synthetic ending.
fn finish(session: &Arc<Session>, error: &TransportError) {
    let detail = error.to_string();

    if session.end_once(EndReason::AdapterDied {
        detail: detail.clone(),
    }) {
        tracing::warn!(
            target: "daemon.session",
            session_id = %session.id,
            error = %detail,
            "adapter died without terminating the session; synthesised the ending",
        );
    } else {
        tracing::debug!(
            target: "daemon.session",
            session_id = %session.id,
            "adapter closed the socket after the session had already ended",
        );
    }
}
