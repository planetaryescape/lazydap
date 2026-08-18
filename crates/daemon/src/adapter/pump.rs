//! The session read pump.
//!
//! One task per session owns the adapter's read half and does nothing else
//! with it. That is not a style choice: `DapReader::read_incoming` is not
//! cancellation-safe, so a read that is ever raced against a timer or another
//! `select!` arm can be dropped mid-frame and desynchronise the stream for
//! good. Owning reads in a task that only reads makes that impossible by
//! construction, and everything else — request waiters, event fan-out — is
//! reached through channels.

use super::handshake::{PumpStart, applied_breakpoints, output_chunk, started_process};
use super::{BreakpointRequests, Pending, StopContext};
use crate::state::{Outstanding, OutstandingStep, Session};
use lazydap_core::{
    AdapterBreakpoint, Breakpoint, BreakpointId, EndReason, PauseReason, SessionState,
    ThreadUpdate, ThreadUpdateKind,
};
use lazydap_dap::{DapEvent, DapReader, DapResponse, Incoming};
use lazydap_protocol::Event;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

/// How long the adapter gets to answer the `disconnect` that ends a session it
/// has already terminated. It has nothing else left to do, so this is a
/// backstop rather than a negotiation.
const DISCONNECT_GRACE: Duration = Duration::from_secs(2);

/// How long to keep reading after that answer, for an `exited` still behind the
/// `terminated` on the wire. Same window, and the same reason, as the
/// handshake's.
const POST_TERMINATION_GRACE: Duration = Duration::from_millis(250);

/// The longest the pump itself will sit in a session that has already ended —
/// the wind-down above, plus slack, so a wind-down that somehow never finishes
/// cannot keep the adapter alive for the life of the daemon.
const WIND_DOWN_DEADLINE: Duration = Duration::from_secs(5);

/// Start pumping. The task ends when the adapter does.
pub fn spawn_pump(start: PumpStart, session: Arc<Session>) {
    tokio::spawn(pump(
        start.reader,
        start.pending,
        start.breakpoint_requests,
        session,
    ));
}

/// Read until the adapter has nothing left to say, then make sure it is gone.
///
/// The teardown is in this task rather than at the end of the read loop
/// because a panic in the loop would skip it, and what it skips is an adapter
/// process left running with nobody holding its socket. `tokio::spawn` turns a
/// panic into a `JoinError` here instead of into a silent leak — the pump's own
/// `JoinHandle` is dropped, so nothing else would ever notice.
async fn pump(
    reader: DapReader,
    pending: Pending,
    breakpoint_requests: BreakpointRequests,
    session: Arc<Session>,
) {
    let reading = tokio::spawn(run(
        reader,
        Arc::clone(&pending),
        Arc::clone(&breakpoint_requests),
        Arc::clone(&session),
    ));
    if let Err(error) = reading.await {
        finish(&session, format!("the session pump stopped: {error}")).await;
    }

    // Killed *before* the waiters are woken. A request that registers between
    // the two would otherwise wait out the full request timeout and come back
    // as `AdapterTimeout` for an adapter that is already gone; with the writer
    // taken first it gets `Gone` immediately.
    session.adapter().kill().await;
    // Nobody will ever answer the outstanding requests. Dropping the senders
    // wakes their waiters with `AdapterError::Gone` rather than leaving them
    // to time out one by one.
    pending.lock().await.clear();
    // Nothing will answer those requests either, so nothing will ever come to
    // claim what they asked for.
    lock(&breakpoint_requests).clear();

    tracing::debug!(target: "daemon.session", session_id = %session.id, "pump stopped");
}

async fn run(
    mut reader: DapReader,
    pending: Pending,
    breakpoint_requests: BreakpointRequests,
    session: Arc<Session>,
) {
    // A debuggee quick enough to finish during its own launch has already
    // ended, and the `terminated` that says so was read by the handshake. There
    // is nothing left to wait for, so the wind-down starts here rather than
    // never (D094).
    let mut winding_down = ended_by_itself(&session).then(|| wind_down(&session));

    loop {
        let incoming = match winding_down {
            None => reader.read_incoming().await,
            // Cancelling a read leaves the stream mid-frame, which is normally
            // fatal — but the only thing left to do with this reader is stop
            // using it, and a misparse gets there as surely as a clean EOF.
            Some(deadline) => match tokio::time::timeout_at(deadline, reader.read_incoming()).await
            {
                Ok(incoming) => incoming,
                Err(_) => {
                    tracing::debug!(
                        target: "daemon.session",
                        session_id = %session.id,
                        "the adapter did not close its socket after the session ended; killing it",
                    );
                    return;
                }
            },
        };

        match incoming {
            Ok(Incoming::Response(response)) => {
                record_breakpoint_ids(&session, &breakpoint_requests, &response);
                let waiter = pending.lock().await.remove(&response.request_seq);
                match waiter {
                    Some(sender) => {
                        // A dropped receiver means the requester gave up
                        // first; that is its business, not ours.
                        let _ = sender.send(response);
                    }
                    // Ordinary once the session is over: the handshake stops
                    // reading the moment it sees `terminated`, so a program
                    // that ends during its own launch leaves the
                    // `configurationDone` answer for the pump to find. Only a
                    // *live* session losing an answer is worth a warning.
                    None if !session.state().is_live() => tracing::debug!(
                        target: "daemon.session",
                        session_id = %session.id,
                        command = response.command,
                        "an answer that arrived after the session had ended",
                    ),
                    None => tracing::warn!(
                        target: "daemon.session",
                        session_id = %session.id,
                        request_seq = response.request_seq,
                        command = response.command,
                        "response for a request nobody is waiting on",
                    ),
                }
            }
            Ok(Incoming::Event(event)) => {
                handle_event(&session, event);
                if winding_down.is_none() && ended_by_itself(&session) {
                    winding_down = Some(wind_down(&session));
                }
            }
            // Answered rather than ignored: an adapter waiting on a reply it
            // will never get is a session that stops making progress with
            // nothing said about why.
            Ok(Incoming::ReverseRequest(request)) => session.adapter().refuse(&request).await,
            Err(error) => {
                finish(&session, error.to_string()).await;
                return;
            }
        }
    }
}

/// The session has ended. Send the adapter the `disconnect` it is waiting for,
/// and say how long to keep reading for its answer.
///
/// This is the fix for the leak D094 names: codelldb, debugpy and delve all
/// hold their socket open after `terminated` until a client disconnects, so a
/// pump that simply kept reading kept the adapter — and its whole process tree
/// — alive for the life of the daemon. One adapter leaked per session that
/// ended on its own, which is every session an agent runs without a closing
/// `lazydap disconnect`.
///
/// Sent from a task rather than inline because this *is* the read loop: the
/// answer to the request can only arrive through it, so waiting here would
/// guarantee the timeout. `terminate: false` because there is nothing left to
/// terminate — the program has already ended.
///
/// The same task then kills the adapter, which is what ends the read loop:
/// codelldb answers the `disconnect` in under a millisecond and holds the
/// socket open anyway (quirk 25), so waiting for it to close would cost the
/// whole grace on every session.
fn wind_down(session: &Arc<Session>) -> Instant {
    let session = Arc::clone(session);
    tokio::spawn(async move {
        match tokio::time::timeout(DISCONNECT_GRACE, session.adapter().disconnect(false)).await {
            // Whatever is still in flight — an `exited` behind the
            // `terminated`, the last of the program's output — has this long to
            // arrive before the socket goes.
            Ok(Ok(())) => tokio::time::sleep(POST_TERMINATION_GRACE).await,
            Ok(Err(error)) => tracing::debug!(
                target: "daemon.session",
                session_id = %session.id,
                %error,
                "the adapter refused the disconnect that ends its session",
            ),
            Err(_) => tracing::debug!(
                target: "daemon.session",
                session_id = %session.id,
                "the adapter did not answer the disconnect that ends its session",
            ),
        }
        session.adapter().kill().await;
    });
    Instant::now() + WIND_DOWN_DEADLINE
}

/// Whether this session ended without a client taking it down.
///
/// Only that case is the pump's to wind down. A `lazydap disconnect` is already
/// sending its own `disconnect` and killing the adapter after it; adding a
/// second one behind it puts two of them on the wire, carrying opposite
/// `terminateDebuggee` values (D095).
fn ended_by_itself(session: &Arc<Session>) -> bool {
    !session.state().is_live() && !session.client_teardown_started()
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
/// `continue` that actually resumes the program clears it, which is where a
/// caller has plainly moved on. A `continue` on a program that is already
/// running sends nothing and so clears nothing — clearing there would strand
/// exactly the SIGSTOP this marker exists to name (D090).
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

/// Map the adapter's breakpoint ids to ours, as its answer goes past.
///
/// Here rather than in the caller that sent the request, and that is the fix
/// rather than an implementation detail. This task owns the socket, so it
/// dispatches everything that followed the answer — including the `breakpoint`
/// event codelldb sends microseconds later — before the caller awaiting the
/// answer is scheduled again. Recorded from the caller, the mapping arrived
/// after the event that needed it, and the update went out with a `null` id no
/// caller could match against `break --list` (D099).
///
/// Called for every response, not only successful ones: an entry left behind
/// by a request the adapter rejected is an entry nothing else will ever
/// remove.
fn record_breakpoint_ids(
    session: &Session,
    requests: &BreakpointRequests,
    response: &DapResponse<serde_json::Value>,
) {
    let Some(requested) = lock(requests).remove(&response.request_seq) else {
        return;
    };
    if !response.success {
        return;
    }
    session.record_breakpoints(&applied_breakpoints(&requested, response.body.clone()));
}

/// The in-flight `setBreakpoints` map, with a poisoned lock treated as usable.
fn lock(requests: &BreakpointRequests) -> std::sync::MutexGuard<'_, HashMap<i64, Vec<Breakpoint>>> {
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The read side died. Make sure clients hear about it (D022).
///
/// Adapters do not reliably say goodbye — a crashed one just closes the
/// socket, and a UI waiting for `terminated` waits forever. So an EOF that
/// arrives before the session ended properly becomes a synthetic ending.
async fn finish(session: &Arc<Session>, mut detail: String) {
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

    /// A session whose events can be read back, for the two tests below.
    fn watched_session() -> (
        Arc<Session>,
        tokio::sync::broadcast::Receiver<crate::state::SeqEvent>,
    ) {
        let (event_tx, events) = tokio::sync::broadcast::channel(64);
        let session = Arc::new(Session::new(
            SessionId::new(),
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            AdapterHandle::detached(),
            event_tx,
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        ));
        (session, events)
    }

    /// Exactly what codelldb sends, in the order it sends it, taken off the
    /// wire in `docs/reference/codelldb-quirks.md` quirk 10.
    fn set_breakpoints_response(seq: i64) -> DapResponse<serde_json::Value> {
        DapResponse {
            seq: 15,
            request_seq: seq,
            message_type: "response".to_string(),
            command: "setBreakpoints".to_string(),
            success: true,
            message: None,
            body: Some(serde_json::json!({
                "breakpoints": [
                    {"id": 1, "line": 6, "message": "Resolved locations: 1", "verified": true}
                ]
            })),
        }
    }

    fn breakpoint_event() -> DapEvent {
        DapEvent {
            seq: 16,
            message_type: "event".to_string(),
            event: "breakpoint".to_string(),
            body: Some(serde_json::json!({
                "breakpoint": {
                    "id": 1, "line": 6, "message": "Resolved locations: 1", "verified": true
                },
                "reason": "changed"
            })),
        }
    }

    fn ours(line: u32) -> Breakpoint {
        Breakpoint {
            id: BreakpointId(1),
            source: PathBuf::from("/tmp/exits.c"),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }
    }

    fn updated_id(
        events: &mut tokio::sync::broadcast::Receiver<crate::state::SeqEvent>,
    ) -> Option<BreakpointId> {
        loop {
            match events.try_recv().expect("an event was emitted").event {
                Event::BreakpointUpdated { breakpoint, .. } => return breakpoint.id,
                _ => continue,
            }
        }
    }

    #[test]
    fn a_breakpoint_event_that_follows_its_own_answer_carries_our_id() {
        // The failure this exists for, reproduced in the order the wire had
        // it: codelldb answers `setBreakpoints` and sends the `breakpoint`
        // event about the same breakpoint microseconds later. Both are read
        // here, in this task, before the caller that sent the request runs
        // again — so if the mapping were recorded there, this update would go
        // out with `id: null` and no caller could match it against
        // `break --list`. On macOS a second event 20ms later coalesced over
        // the bad one and hid this; on Linux there is no second event.
        let (session, mut events) = watched_session();
        let requests: BreakpointRequests = Arc::new(std::sync::Mutex::new(HashMap::new()));
        lock(&requests).insert(4, vec![ours(6)]);

        record_breakpoint_ids(&session, &requests, &set_breakpoints_response(4));
        handle_event(&session, breakpoint_event());

        assert_eq!(
            updated_id(&mut events),
            Some(BreakpointId(1)),
            "the update has to name the breakpoint `break --list` gave the caller",
        );
        assert!(
            lock(&requests).is_empty(),
            "and the request is no longer in flight",
        );
    }

    #[test]
    fn an_answer_nobody_registered_leaves_the_id_unmapped() {
        // The same two messages with the registration missing, which is what
        // the old code amounted to. Here so the test above is known to be
        // testing the ordering rather than passing for its own reasons.
        let (session, mut events) = watched_session();
        let requests: BreakpointRequests = Arc::new(std::sync::Mutex::new(HashMap::new()));

        record_breakpoint_ids(&session, &requests, &set_breakpoints_response(4));
        handle_event(&session, breakpoint_event());

        assert_eq!(updated_id(&mut events), None);
    }

    #[test]
    fn a_rejected_answer_takes_its_request_out_of_flight_without_recording_anything() {
        let (session, mut events) = watched_session();
        let requests: BreakpointRequests = Arc::new(std::sync::Mutex::new(HashMap::new()));
        lock(&requests).insert(4, vec![ours(6)]);

        let mut rejected = set_breakpoints_response(4);
        rejected.success = false;
        rejected.body = None;
        record_breakpoint_ids(&session, &requests, &rejected);
        handle_event(&session, breakpoint_event());

        assert!(lock(&requests).is_empty(), "nothing is still in flight");
        assert_eq!(
            updated_id(&mut events),
            None,
            "and a refused request maps nothing",
        );
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
