//! The adapter's lifecycle, and everything that makes the program move.

use super::{Result, find_session, live_session};
use crate::adapter;
use crate::state::{DaemonState, MarkerId, RunClaim, Session};
use crate::wait::{DEFAULT_TIMEOUT, Wait, WaitOptions};
use lazydap_core::{EndReason, SessionId, SessionState, StepKind};
use lazydap_protocol::{ErrorCode, Event, IpcError, Response, WaitMode};
use std::sync::Arc;
use std::time::Duration;

/// What kind of movement a stepping request asks for.
#[derive(Debug, Clone, Copy)]
pub enum Movement {
    Continue,
    Step(StepKind),
    /// Not a movement so much as a request to stop moving.
    Pause,
}

impl Movement {
    fn description(&self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Step(StepKind::Over) => "step over",
            Self::Step(StepKind::In) => "step in",
            Self::Step(StepKind::Out) => "step out",
            Self::Pause => "pause",
        }
    }
}

pub async fn launch(
    state: &Arc<DaemonState>,
    request: lazydap_protocol::LaunchRequest,
) -> Result<Response> {
    // A session whose program has finished is holding the slot for nothing.
    // Making the user run `lazydap disconnect` before they can launch again
    // is ceremony over a session that has no adapter left to protect.
    state.reap_finished();

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

    // The project's breakpoints, grouped the way `setBreakpoints` wants them:
    // one call per source, carrying that source's whole list. Disabled ones
    // are left out rather than sent and then un-set.
    let grouped: Vec<(std::path::PathBuf, Vec<lazydap_core::Breakpoint>)> = state
        .store
        .sources()
        .into_iter()
        .map(|source| {
            let enabled = state
                .store
                .breakpoints_in(&source)
                .into_iter()
                .filter(|breakpoint| breakpoint.enabled)
                .collect();
            (source, enabled)
        })
        .filter(|(_, breakpoints): &(_, Vec<_>)| !breakpoints.is_empty())
        .collect();

    // Which adapter this is, is `request.adapter`'s business — the handler's
    // is that a program is being launched. Non-negotiable #5, finally literal.
    let launched = adapter::launch(&request, &grouped)
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
    session.set_last_thread_id(launched.thread_id);
    session.record_breakpoints(&launched.breakpoints);
    // Recorded only on the launch path, deliberately. When `attach` lands it
    // must not do this: the process was somebody else's before we looked at
    // it, and killing it because our adapter crashed would destroy something
    // we never started (D045).
    if let Some(started) = launched.debuggee {
        session.set_debuggee(started);
    }

    session.seed_events(
        launched
            .output
            .into_iter()
            .map(|chunk| Event::Output { session_id, chunk })
            .collect(),
    );
    // Live and announced, in that order and in one call: a subscriber must
    // never be able to receive `SessionStarted` for a session it cannot then
    // find. This is also why promotion happens *before* the two steps below —
    // both can emit, and nothing may be emitted for this session before the
    // event that opens it.
    reservation.promote(Arc::clone(&session));

    // A debuggee quick enough to finish during its own launch has already
    // ended, and the pump will never see the events that say so. End it here,
    // before the pump starts: `end_once` then makes the socket closing behind
    // the dead adapter a no-op, rather than rewriting this ending as
    // `adapter_died`.
    if !launched.state.is_live() {
        session.set_exit_code(launched.exit_code);
        session.end_once(match launched.exit_code {
            Some(exit_code) => EndReason::Exited {
                exit_code: Some(exit_code),
            },
            None => EndReason::Terminated,
        });
    }

    // The pump takes over reads from here; nothing else may touch them.
    adapter::spawn_pump(launched.pump, Arc::clone(&session));

    // Read the state back rather than echoing what the handshake saw: ending
    // the session above may have refined `terminated` into `exited`.
    let session_state = session.state();
    tracing::info!(
        target: "daemon.session",
        session_id = %session_id,
        state = session_state.as_str(),
        breakpoints = launched.breakpoints.len(),
        "launched",
    );

    Ok(Response::Launched {
        session_id,
        state: session_state,
        reason: launched.reason,
        raw_reason: launched.raw_reason,
        thread_id: launched.thread_id,
        capabilities: launched.capabilities,
        breakpoints: session.decorate(state.store.breakpoints()),
    })
}

pub async fn disconnect(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    terminate: bool,
    dry_run: bool,
) -> Result<Response> {
    if dry_run {
        // The same lookup the real path makes, so a preview cannot claim it
        // would end a session that is not there (non-negotiable #4).
        let session = find_session(state, session_id)?;
        return Ok(Response::Disconnected {
            session_id: session.id,
            dry_run: true,
            terminated_debuggee: terminate && session.state().is_live(),
        });
    }

    // Claim the session without freeing its slot. Tearing down can take
    // seconds — a `disconnect` the adapter ignores waits out its timeout — and
    // a slot freed at the start of that window lets a concurrent `launch` past
    // the single-session check (D007) and spawn a second adapter while the
    // first is still being killed. The guard frees the slot when it drops, so
    // the slot outlives the adapter on every path out of here, including a
    // panic.
    let teardown = state.begin_teardown(session_id).ok_or_else(|| {
        IpcError::new(
            ErrorCode::SessionNotFound,
            format!("no session {session_id}"),
        )
        .with_details(serde_json::json!({ "session_id": session_id.to_string() }))
    })?;
    let session = teardown.session();
    let was_live = session.state().is_live();

    // Ask nicely first so the adapter can detach or kill the debuggee as
    // requested, then make sure the process is gone either way. A refused or
    // timed-out disconnect must not leave an adapter behind.
    if was_live && let Err(error) = session.adapter().disconnect(terminate).await {
        tracing::warn!(
            target: "daemon.session",
            session_id = %session_id,
            %error,
            "the adapter did not acknowledge disconnect; killing it",
        );
    }
    session.adapter().kill().await;
    // Belt-and-braces after killing the adapter: delve deletes its own compiled
    // binary when it handles the `disconnect` above, so this is normally a
    // no-op — but an adapter that ignored the disconnect and had to be killed
    // never got there, and this catches that (delve quirk 5).
    session.clean_compiled_artifact();
    // `end_once` sets the state, and only if this is the first ending: a
    // session whose debuggee had already exited keeps `exited` and its exit
    // code, rather than being relabelled `terminated` on the way out.
    session.end_once(EndReason::Disconnected);
    drop(teardown);

    tracing::info!(target: "daemon.session", session_id = %session_id, terminate, "disconnected");
    Ok(Response::Disconnected {
        session_id,
        dry_run: false,
        terminated_debuggee: terminate && was_live,
    })
}

/// Move the program, and — if asked — wait for it to settle.
///
/// The order below is the whole point of this function, and all three steps
/// have to happen in it:
///
/// 1. **Take the execution permit** (D021). Holding it for the whole run, not
///    just the message, is what stops a second `continue --wait` from resuming
///    the program past the stop this one is waiting for and returning that
///    stop as its own.
/// 2. **Claim the run** — [`Session::claim_run`]. One locked operation that
///    both decides whether the adapter needs asking and moves the session to
///    `Running`. It comes before the subscription rather than after because a
///    stop arriving in between is not lost: [`Wait::begin`] reads the
///    undelivered backlog as well as subscribing.
/// 3. **Subscribe**, inside the permit. A fast program can hit its breakpoint
///    before the adapter has acknowledged the `continue`, so a subscription
///    taken after the send would miss the stop it exists to catch.
/// 4. **Send**, unless step 2 said there was nothing to ask for.
///
/// `pause` skips steps 1 and 2 — see [`AdapterHandle::interrupt`].
pub async fn execute(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    movement: Movement,
    thread_id: Option<i64>,
    wait: WaitMode,
    all_threads: bool,
) -> Result<Response> {
    let session = live_session(state, session_id)?;
    refuse_pointless_pause(&session, movement)?;
    let thread_id = resolve_thread(&session, thread_id).await?;

    // Step 1. Dropped at the end of this function, so it covers the wait too.
    let permit = match movement {
        Movement::Pause => None,
        _ => Some(session.adapter().execution_permit().await),
    };

    // Step 2. `continue` on a program that is already running is a request to
    // do the thing it is already doing. codelldb answers it anyway and nothing
    // happens; debugpy does not answer it at all, and the acknowledgement
    // timeout that follows is treated as a wedged adapter and kills the
    // session (see `AdapterHandle::execute`). The sequence that reaches it is
    // ordinary rather than exotic: launch without `--stop-on-entry`, then
    // `continue --wait` to reach the first breakpoint (D055).
    //
    // Deciding that and writing the state are one locked operation, because
    // the pump can record a stop between any two of these lines — see
    // `claim_run` for what each interleaving used to corrupt.
    let claim = match movement {
        Movement::Pause => None,
        _ => Some(session.claim_run(matches!(movement, Movement::Continue))),
    };

    // Step 3. After the permit, so nothing observed here belongs to an earlier
    // run; before the send, so nothing this run causes is missed.
    let waiting = wait.is_waiting().then(|| Wait::begin(&session));

    // Step 4. The marker goes up *before* the send: an adapter can emit the
    // `stopped` event this request causes before it acknowledges the request
    // itself, and the pump reads the marker as the stop arrives.
    let marker = expect(&session, movement, thread_id);

    if !matches!(claim, Some(RunClaim::AlreadyRunning))
        && let Err(error) = send(&session, permit.as_ref(), movement, thread_id).await
    {
        // Take our own marker back down. A rejected `pause --thread 999` that
        // left `Pause` installed had every later SIGSTOP renamed to a pause
        // nobody was waiting for. By id, because a request that arrived while
        // this one was being refused may already have replaced it, and
        // clearing *that* would be the same bug one step along (D071).
        if let Some(marker) = marker {
            session.withdraw(marker);
        }
        // Put back only the state *we* wrote. By now the pump may have
        // recorded a real ending — an adapter can execute the request, emit
        // `terminated` and die before acknowledging — and stamping `paused`
        // over that would leave a dead session looking live, refusing every
        // later launch.
        if let Some(RunClaim::Ask { previous }) = claim {
            session.restore_state(SessionState::Running, previous);
        }
        return Err(error.into_ipc());
    }

    let Some(waiting) = waiting else {
        // Nothing was sent, so nothing was resumed, and the thread resolved
        // above is not a thread that moved — on a running program codelldb
        // answers `threads` with id `0`, which is not a thread at all. Saying
        // "running, thread 0" to that was two inventions in one line: a resume
        // that did not happen and a thread that does not exist (D076).
        let already_running = matches!(claim, Some(RunClaim::AlreadyRunning));
        return Ok(Response::Continued {
            session_id,
            thread_id: (!already_running).then_some(thread_id),
            already_running,
        });
    };

    let blob = waiting
        .collect(WaitOptions {
            timeout: resolve_timeout(wait),
            all_threads,
        })
        .await;

    tracing::debug!(
        target: "daemon.session",
        session_id = %session_id,
        movement = movement.description(),
        outcome = blob.state.as_str(),
        elapsed_ms = blob.elapsed_ms,
        "wait finished",
    );
    Ok(Response::Stepped(Box::new(blob)))
}

/// Refuse a `pause` on a program that is already stopped.
///
/// There is nothing to interrupt and no future stop to wait for, so what the
/// wait did instead was hand back the stop the program was *already* sitting
/// at, wearing a fresh `elapsed_ms` — a blob indistinguishable from one the
/// request had caused. An agent reading it concluded its pause had worked.
///
/// This is deliberately not what `continue` on a *running* program does (D076),
/// and the asymmetry is real rather than an oversight. "Continue" has a second
/// reading — *get me to the next stop* — and it is the documented ordinary
/// sequence for a launch without `--stop-on-entry` (D055), so it waits and
/// answers truthfully about what it did. "Pause" has no second reading: the
/// program is stopped, that is the state the caller wanted, and the only honest
/// answers are an error or a fabricated stop. `lazydap status` is the command
/// for where it is (D081).
fn refuse_pointless_pause(session: &Arc<Session>, movement: Movement) -> Result<()> {
    if !matches!(movement, Movement::Pause) || session.state() != SessionState::Paused {
        return Ok(());
    }
    Err(IpcError::new(
        ErrorCode::BadRequest,
        format!(
            "session {} is already stopped; there is nothing to pause \
             (`lazydap status` says where it is)",
            session.id,
        ),
    )
    .with_details(serde_json::json!({
        "session_id": session.id.to_string(),
        "state": SessionState::Paused,
    })))
}

/// Record what the program has just been asked to do, and name the marker so
/// the caller can take it back down if the adapter refuses.
///
/// A `continue` records nothing and clears both slots: its next stop is
/// whatever the program did next and needs no context to describe, and once it
/// resumes, a step still recorded is finished and a pause that never landed is
/// stale.
fn expect(session: &Arc<Session>, movement: Movement, thread_id: i64) -> Option<MarkerId> {
    match movement {
        Movement::Continue => {
            session.expect_nothing();
            None
        }
        Movement::Step(_) => Some(session.expect_step(thread_id)),
        Movement::Pause => Some(session.expect_pause()),
    }
}

async fn send(
    session: &Arc<Session>,
    permit: Option<&adapter::ExecutionPermit<'_>>,
    movement: Movement,
    thread_id: i64,
) -> adapter::Result<()> {
    match (movement, permit) {
        (Movement::Continue, Some(permit)) => {
            session.adapter().resume(permit, thread_id).await?;
        }
        (Movement::Step(kind), Some(permit)) => {
            session.adapter().step(permit, kind, thread_id).await?
        }
        (Movement::Pause, _) => session.adapter().interrupt(thread_id).await?,
        // `execute` takes a permit for everything that is not a pause, so this
        // cannot happen — and saying so is better than an `unwrap` that would
        // be wrong in a way nobody could see from the panic.
        (movement, None) => {
            tracing::error!(
                target: "daemon.session",
                movement = movement.description(),
                "an execution request reached the adapter without a permit",
            );
            return Err(adapter::AdapterError::Gone);
        }
    }
    Ok(())
}

/// `None` means the daemon's default; `Some(0)` means the caller has taken
/// responsibility for a program that may never stop.
fn resolve_timeout(wait: WaitMode) -> Option<Duration> {
    match wait {
        WaitMode::NoWait => None,
        WaitMode::Wait { timeout_ms: None } => Some(DEFAULT_TIMEOUT),
        WaitMode::Wait {
            timeout_ms: Some(0),
        } => None,
        WaitMode::Wait {
            timeout_ms: Some(ms),
        } => Some(Duration::from_millis(ms as u64)),
    }
}

/// Which thread a request that did not name one means.
///
/// The thread that stopped last, because that is the one the caller has been
/// looking at. Falling back to asking the adapter covers the case where
/// nothing has stopped yet — a `pause` on a program that has been running
/// since launch.
async fn resolve_thread(session: &Arc<Session>, explicit: Option<i64>) -> Result<i64> {
    if let Some(thread_id) = explicit {
        return Ok(thread_id);
    }
    if let Some(thread_id) = session.last_thread_id() {
        return Ok(thread_id);
    }

    let threads = session
        .adapter()
        .threads()
        .await
        .map_err(adapter::AdapterError::into_ipc)?;
    threads
        .first()
        .map(|thread| thread.id)
        .ok_or_else(|| IpcError::new(ErrorCode::BadRequest, "the program has no threads to move"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_timeout_at_all_is_what_a_caller_asks_for_with_zero() {
        assert_eq!(
            resolve_timeout(WaitMode::Wait {
                timeout_ms: Some(0)
            }),
            None,
            "0 is the documented spelling of `wait forever`",
        );
    }

    #[test]
    fn a_wait_that_names_no_timeout_gets_the_daemon_s_default() {
        assert_eq!(
            resolve_timeout(WaitMode::Wait { timeout_ms: None }),
            Some(DEFAULT_TIMEOUT),
        );
    }

    #[test]
    fn an_explicit_timeout_is_used_as_given() {
        assert_eq!(
            resolve_timeout(WaitMode::Wait {
                timeout_ms: Some(1_500)
            }),
            Some(Duration::from_millis(1_500)),
        );
    }
}
