//! The session read pump.
//!
//! One task per session owns the adapter's read half and does nothing else
//! with it. That is not a style choice: `DapReader::read_incoming` is not
//! cancellation-safe, so a read that is ever raced against a timer or another
//! `select!` arm can be dropped mid-frame and desynchronise the stream for
//! good. Owning reads in a task that only reads makes that impossible by
//! construction, and everything else — request waiters, event fan-out — is
//! reached through channels.

use super::handshake::{PumpStart, output_chunk, started_process};
use super::{Pending, StopContext};
use crate::state::{Outstanding, OutstandingStep, Session};
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
            // Answered rather than ignored: an adapter waiting on a reply it
            // will never get is a session that stops making progress with
            // nothing said about why.
            Ok(Incoming::ReverseRequest(request)) => session.adapter().refuse(&request).await,
            Err(error) => {
                finish(&session, &error).await;
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
        // Which process the adapter started. The handshake watches for this
        // too, and usually sees it first — but nothing orders it against the
        // `launch` response, so a launch that settles first would otherwise
        // leave the session with no pid to reap (D045). `set_debuggee` keeps
        // the first answer, so both paths recording it is not two records.
        "process" => {
            if let Some(started) = started_process(&event) {
                session.set_debuggee(started);
            }
        }
        // An adapter's reasons are not its own once lazydap asked for
        // something: codelldb reports a `pause` as an exception with a
        // `SIGSTOP` description, exactly as it reports an entry stop, and
        // answers a `next` aimed at one thread by naming another. Both are
        // read against the requests still outstanding — read, not taken,
        // because which of them this stop answers is not known until the stop
        // has been read (D071).
        "stopped" => {
            let outstanding = session.outstanding();
            let (reason, raw_reason) = super::for_kind(session.adapter_kind).normalise_stop(
                body["reason"].as_str().unwrap_or("unknown"),
                body["description"].as_str().unwrap_or_default(),
                StopContext {
                    // The handshake owns the entry stop; by the time the pump
                    // is running that one has already been reported.
                    stop_on_entry: false,
                    pause_requested: outstanding.pause.is_some(),
                },
            );

            let said = body["threadId"].as_i64();
            let (thread_id, adapter_thread_id) =
                stopped_thread_ids(&reason, outstanding.step, said);
            answered(session, &reason, outstanding);
            session.set_state(SessionState::Paused);
            session.set_last_thread_id(thread_id);
            tracing::debug!(
                target: "daemon.session",
                session_id = %session_id,
                reason = %reason,
                thread_id,
                adapter_thread_id,
                "paused",
            );
            session.emit(Event::Stopped {
                session_id,
                thread_id,
                adapter_thread_id,
                reason,
                raw_reason,
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
                // This *is* an adapter's opinion, so it is scoped to the
                // session whose adapter holds it.
                session_id: Some(session_id),
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

/// Take down the marker this stop answered, and only that one.
///
/// A `pause` does not hold the execution permit, so it can be outstanding at
/// the same time as the step it interrupts, and the two are answered by two
/// different stops. Consuming both on whichever arrives first is what made a
/// concurrent `step --wait` and `pause --wait` produce two wrong answers at
/// once (D071).
///
/// - A stop reported as `pause` is the pause's. It says nothing about the step,
///   which is still in flight: the program was stepping when it was stopped.
/// - Any other stop ends the run a step started, whether or not it is the
///   step's own — `stopped_thread_ids` is what checks that separately.
///
/// A pause that never produces its own stop is left installed until the next
/// `continue` clears it, which is where a caller has plainly moved on.
fn answered(session: &Arc<Session>, reason: &PauseReason, outstanding: Outstanding) {
    if matches!(reason, PauseReason::Pause) {
        if let Some(id) = outstanding.pause {
            session.withdraw(id);
        }
    } else if let Some(step) = outstanding.step {
        session.withdraw(step.id);
    }
}

/// Which thread a stop is about, and which one the adapter said.
///
/// codelldb answers `{"threadId": A, "command": "next"}` with
/// `{"event": "stopped", "reason": "step", "threadId": B}`, where B is whatever
/// thread it had selected before — measured ten times out of ten, with A the
/// thread that actually moved and B one that did not. Relaying B told the agent
/// a thread stepped that had not, and left B as `last_thread_id`, so the next
/// bare `lazydap stack` answered about the wrong thread as well.
///
/// So a *step* is reported against the thread it was aimed at, and the
/// adapter's own answer is kept beside it rather than dropped — `raw_reason`'s
/// discipline, applied to the thread (D066). The guard is narrow on purpose:
/// only a step, only when a step is outstanding, only when the two disagree. A
/// stop that is not a step — a breakpoint hit on another thread while stepping
/// this one — is the adapter telling us something we did not ask about, and
/// passes through as it always did.
///
/// No frame is invented for A: the blob's frame is fetched for whichever thread
/// this returns, so reporting A means fetching A's frame.
fn stopped_thread_ids(
    reason: &PauseReason,
    step: Option<OutstandingStep>,
    said: Option<i64>,
) -> (Option<i64>, Option<i64>) {
    let Some(step) = step else {
        return (said, None);
    };
    if !matches!(reason, PauseReason::Step) || said == Some(step.thread_id) {
        return (said, None);
    }

    tracing::debug!(
        target: "daemon.session",
        requested = step.thread_id,
        said,
        "the adapter named a different thread than the one asked to step (D066)",
    );
    (Some(step.thread_id), said)
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
    // Same rule as the `setBreakpoints` response it corrects: a verified
    // breakpoint's message is commentary, and `Resolved locations: 1` reads
    // like a change of state when nothing changed (D077).
    .without_settled_message()
}

/// The read side died. Make sure clients hear about it (D022).
///
/// Adapters do not reliably say goodbye — a crashed one just closes the
/// socket, and a UI waiting for `terminated` waits forever. So an EOF that
/// arrives before the session ended properly becomes a synthetic ending.
async fn finish(session: &Arc<Session>, error: &TransportError) {
    let mut detail = error.to_string();

    // Before the ending is announced, so the ending can say what became of the
    // program. An adapter that was killed never got to stop its debuggee, and a
    // daemon that killed only the adapter left the user's program running with
    // nothing left that knew it was being debugged (D045).
    if let Some(reaped) = session.reap_debuggee().await {
        detail.push_str("; ");
        detail.push_str(&reaped);
    }
    // The adapter never ran its own cleanup, so delve's compiled binary is
    // still on disk (delve quirk 5). Remove it here — the process it belonged
    // to has just been reaped above.
    session.clean_compiled_artifact();

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterHandle;
    use lazydap_core::{AdapterKind, SessionId};
    use std::path::PathBuf;

    fn step(thread_id: i64) -> Option<OutstandingStep> {
        Some(OutstandingStep {
            id: session().expect_step(thread_id),
            thread_id,
        })
    }

    /// A session with no adapter behind it. Everything here exercises the
    /// bookkeeping around a stop, which is where the subtle bugs live.
    fn session() -> Arc<Session> {
        let (event_tx, _keep_open) = tokio::sync::broadcast::channel(64);
        Arc::new(Session::new(
            SessionId::new(),
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            AdapterHandle::detached(),
            event_tx,
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        ))
    }

    #[test]
    fn a_step_is_reported_against_the_thread_it_was_aimed_at() {
        // Verbatim off the wire: `next(threadId=34353118)` answered with
        // `{"reason":"step","threadId":34353117}`, where 34353117 was the
        // previous step's target and did not move. Relaying it told the agent
        // a thread stepped that had not (D066).
        let (thread_id, adapter_thread_id) =
            stopped_thread_ids(&PauseReason::Step, step(34353118), Some(34353117));

        assert_eq!(
            thread_id,
            Some(34353118),
            "the thread that was asked to step"
        );
        assert_eq!(
            adapter_thread_id,
            Some(34353117),
            "and the adapter's own answer, kept rather than dropped",
        );
    }

    #[test]
    fn an_adapter_that_names_the_thread_we_asked_for_discloses_nothing() {
        assert_eq!(
            stopped_thread_ids(&PauseReason::Step, step(7), Some(7)),
            (Some(7), None),
            "there is no discrepancy to report",
        );
    }

    #[test]
    fn a_stop_that_is_not_the_step_passes_through_untouched() {
        // A breakpoint hit on another thread while this one was stepping is
        // the adapter telling us something we did not ask about. Rewriting it
        // to the stepped thread would be the invention this fix exists to
        // stop.
        assert_eq!(
            stopped_thread_ids(&PauseReason::Breakpoint, step(7), Some(9)),
            (Some(9), None),
        );
        assert_eq!(
            stopped_thread_ids(&PauseReason::Step, None, Some(9)),
            (Some(9), None),
            "nothing was asked to step",
        );
    }

    #[test]
    fn a_pause_racing_a_step_does_not_cost_the_step_its_thread_correction() {
        // `pause` takes no execution permit (D021), so it is in flight beside
        // the step it interrupts. A single marker slot meant the pause
        // overwrote the step and *both* answers went wrong at once: the step's
        // stop lost its thread correction, and the pause's own SIGSTOP — with
        // the marker already consumed — came back as a genuine exception.
        let session = session();
        let step_marker = session.expect_step(42);
        let pause_marker = session.expect_pause();
        assert_ne!(
            step_marker, pause_marker,
            "two requests, two markers, or one of them is lost",
        );

        // The step's stop lands first. It still sees its own step.
        let outstanding = session.outstanding();
        assert_eq!(
            stopped_thread_ids(&PauseReason::Step, outstanding.step, Some(99)),
            (Some(42), Some(99)),
            "the step is still tracked despite the pause behind it",
        );
        answered(&session, &PauseReason::Step, outstanding);

        // ...and the pause is still outstanding, so its own stop is still
        // readable as a pause rather than as a crash.
        let after = session.outstanding();
        assert_eq!(after.step, None, "the step has been answered");
        assert_eq!(
            after.pause,
            Some(pause_marker),
            "the pause has not, and its SIGSTOP is still to come",
        );
    }

    #[test]
    fn a_pause_that_lands_first_leaves_the_step_it_interrupted_outstanding() {
        // The other order. The pause's stop must not consume the step's marker,
        // or the step that follows loses its thread correction.
        let session = session();
        let step_marker = session.expect_step(42);
        session.expect_pause();

        let outstanding = session.outstanding();
        answered(&session, &PauseReason::Pause, outstanding);

        let after = session.outstanding();
        assert_eq!(after.pause, None, "the pause has been answered");
        assert_eq!(
            after.step.map(|step| step.id),
            Some(step_marker),
            "the step was interrupted, not answered",
        );
    }

    #[test]
    fn withdrawing_a_rejected_request_cannot_take_a_newer_one_with_it() {
        // `pause --thread 999` is refused, and its marker has to come back
        // down or every later SIGSTOP is renamed to a pause nobody asked for.
        // But a step may have arrived while it was being refused, and clearing
        // "the marker" would erase that instead (D071).
        let session = session();
        let stale = session.expect_pause();
        let fresh = session.expect_step(7);

        session.withdraw(stale);

        let after = session.outstanding();
        assert_eq!(after.pause, None, "the refused pause is gone");
        assert_eq!(
            after.step.map(|step| step.id),
            Some(fresh),
            "and the step that arrived meanwhile is untouched",
        );
    }

    #[test]
    fn resuming_forgets_both_slots() {
        // A `continue` makes a step still recorded finished and a pause that
        // never landed stale, which is what bounds a marker's lifetime.
        let session = session();
        session.expect_step(7);
        session.expect_pause();

        session.expect_nothing();

        assert_eq!(session.outstanding(), Outstanding::default());
    }
}
