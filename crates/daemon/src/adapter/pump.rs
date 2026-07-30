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
use lazydap_core::{
    AdapterBreakpoint, BreakpointId, EndReason, PauseReason, SessionState, ThreadUpdate,
    ThreadUpdateKind,
};
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
            session.set_last_thread_id(thread_id);
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
                // Only the first stop of a `stop_on_entry` launch is
                // normalised, and the handshake owns that one — by the time
                // the pump is running, the adapter's reasons are its own.
                raw_reason: None,
                all_threads_stopped: body["allThreadsStopped"].as_bool().unwrap_or(false),
                hit_breakpoint_ids: hit_breakpoints(session, &body),
            });
        }
        // The adapter changed its mind about a breakpoint: verified one it had
        // not, or moved it to the nearest line that has code. Recorded as well
        // as broadcast, so a `break --list` between sessions-worth of events
        // reports what the debugger will actually do.
        "breakpoint" => {
            let mut breakpoint = adapter_breakpoint(&body["breakpoint"]);
            session.update_breakpoint(&breakpoint);
            // Fill in our own id before the event goes out. The adapter only
            // knows its own, and a `breakpoint_updates` entry a caller cannot
            // match against the ids `break --list` gave them is an update
            // about nothing they can name.
            breakpoint.id = breakpoint
                .adapter_id
                .and_then(|adapter_id| session.breakpoint_id_for(adapter_id));
            session.emit(Event::BreakpointUpdated {
                session_id,
                breakpoint,
            });
        }
        "thread" => {
            let Some(thread_id) = body["threadId"].as_i64() else {
                return;
            };
            let kind = match body["reason"].as_str() {
                Some("exited") => ThreadUpdateKind::Exited,
                _ => ThreadUpdateKind::Started,
            };
            session.emit(Event::ThreadChanged {
                session_id,
                update: ThreadUpdate {
                    thread_id,
                    kind,
                    name: None,
                },
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
        // ending. DAP does *not* guarantee that order — adapters are free to
        // send `terminated` first, or `exited` late — so this arm records the
        // code unconditionally, including after the session has already ended.
        // `status` therefore reports the right exit code either way; what a
        // late `exited` cannot do is correct an already-emitted
        // `SessionEnded`, which M6's `--wait` has to handle with a short grace
        // window before it emits its final blob.
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

/// Which of *our* breakpoints a stop is attributed to.
///
/// The event lists the adapter's ids, which mean nothing to a client. An id we
/// cannot map — a breakpoint set by something other than us, or a stale one —
/// is dropped rather than passed through as a number from a different
/// namespace.
fn hit_breakpoints(session: &Arc<Session>, body: &serde_json::Value) -> Vec<BreakpointId> {
    body["hitBreakpointIds"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_i64())
                .filter_map(|id| session.breakpoint_id_for(id))
                .collect()
        })
        .unwrap_or_default()
}

fn adapter_breakpoint(body: &serde_json::Value) -> AdapterBreakpoint {
    AdapterBreakpoint {
        // Filled in from the session's map by whoever records it; the event
        // itself only knows the adapter's id.
        id: None,
        adapter_id: body["id"].as_i64(),
        verified: body["verified"].as_bool().unwrap_or(false),
        line: body["line"].as_u64().map(|line| line as u32),
        message: body["message"].as_str().map(str::to_string),
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
