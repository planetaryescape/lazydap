//! Reading a stopped program.
//!
//! Every handler here refuses a session that is running: the stack and the
//! variables of a moving program describe a moment that has already passed
//! (`docs/blueprint/10-async-to-sync.md`). The one exception is [`output`],
//! which reads the daemon's own buffer and never touches the adapter.

use super::{Result, find_session, live_session, paused_session};
use crate::adapter::AdapterError;
use crate::handles::HandleKind;
use crate::state::{DaemonState, Session};
use lazydap_core::{EvalContext, SessionId, StackFrame, VariableFilter};
use lazydap_protocol::{ErrorCode, IpcError, Response, VariableList};
use std::sync::Arc;

/// How many variables one `variables` answer carries when nobody said.
///
/// A `Vec` of two thousand expands to two thousand and one rows, and an agent
/// that asked what a container held used most of its context finding out —
/// with nothing in the response to say so. The number is a default rather than
/// a limit: `--max 0` lifts it entirely for a caller who means it (D080).
pub const DEFAULT_VARIABLE_CAP: u32 = 200;

pub async fn threads(state: &Arc<DaemonState>, session_id: SessionId) -> Result<Response> {
    // Deliberately not `paused_session`: which threads exist is a fair
    // question about a running program, and often the first one asked when
    // deciding which one to pause.
    let session = live_session(state, session_id)?;
    let threads = session
        .adapter()
        .threads()
        .await
        .map_err(AdapterError::into_ipc)?;
    Ok(Response::Threads(threads))
}

pub async fn stack_trace(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    thread_id: Option<i64>,
    start_frame: Option<u32>,
    levels: Option<u32>,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    let fence = session.stop_generation();
    let thread_id = match thread_id {
        Some(thread_id) => thread_id,
        None => stopped_thread(&session)?,
    };

    let (frames, total) = session
        .adapter()
        // `0` means "no limit" everywhere else a count is asked for — it is
        // what `--timeout 0` means — and DAP says the same thing about
        // `levels`. Answering it with an empty list under exit 0, which is what
        // passing it through did, is the one reading nobody could have wanted
        // (D079).
        .stack_trace(thread_id, start_frame, levels.filter(|levels| *levels > 0))
        .await
        .map_err(AdapterError::into_ipc)?;

    // Before the ids are handed out, not after: a handle stamped with a stop
    // its frame did not come from is precisely what the table exists to stop.
    still_paused(&session, fence)?;
    let frames = frames
        .into_iter()
        .map(|frame| ours(&session, fence, frame))
        .collect();
    Ok(Response::StackTrace { frames, total })
}

/// A frame with our handle in place of the adapter's id.
pub fn ours(session: &Arc<Session>, fence: u64, mut frame: StackFrame) -> StackFrame {
    frame.id = session.mint_handle(fence, HandleKind::Frame, frame.id);
    frame
}

pub async fn scopes(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    frame_id: Option<i64>,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    let fence = session.stop_generation();
    let frame_id = resolve_frame(&session, fence, frame_id).await?;
    still_paused(&session, fence)?;

    let mut scopes = session
        .adapter()
        .scopes(frame_id)
        .await
        .map_err(AdapterError::into_ipc)?;

    still_paused(&session, fence)?;
    for scope in &mut scopes {
        scope.variables_reference =
            session.mint_handle(fence, HandleKind::Variables, scope.variables_reference);
    }
    Ok(Response::Scopes(scopes))
}

pub async fn variables(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    variables_reference: i64,
    filter: VariableFilter,
    start: Option<u32>,
    count: Option<u32>,
    max: Option<u32>,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    let fence = session.stop_generation();
    // Before the adapter is asked anything. A reference from an earlier stop
    // either errored obscurely or — the reason this check exists — collided
    // with one the adapter had recycled and came back full of another frame's
    // variables under exit 0 (D075).
    let reference = session.resolve_handle(fence, HandleKind::Variables, variables_reference)?;

    let variables = session
        .adapter()
        .variables(reference, filter, start, count.filter(|count| *count > 0))
        .await
        .map_err(AdapterError::into_ipc)?;

    still_paused(&session, fence)?;
    Ok(Response::Variables(cap(&session, fence, variables, max)))
}

/// Keep at most `max` of them, and say whether that lost anything.
///
/// `Some(0)` is the caller lifting the cap, the way `--timeout 0` lifts the
/// wait (D079). The values themselves are never shortened: a five-thousand
/// character string is one row, and a row reading `"abcd…"` would be a claim
/// about the *data* rather than about the list, which is the one thing
/// truncation must not become (D080).
fn cap(
    session: &Arc<Session>,
    fence: u64,
    mut variables: Vec<lazydap_core::Variable>,
    max: Option<u32>,
) -> VariableList {
    let limit = match max.unwrap_or(DEFAULT_VARIABLE_CAP) {
        0 => usize::MAX,
        limit => limit as usize,
    };
    let truncated = variables.len() > limit;
    variables.truncate(limit);

    for variable in &mut variables {
        // Zero is DAP's "this is a scalar, there is nothing inside it", not a
        // reference, and minting a handle for it would invite an expansion of
        // something that cannot be expanded.
        if variable.variables_reference != 0 {
            variable.variables_reference =
                session.mint_handle(fence, HandleKind::Variables, variable.variables_reference);
        }
    }

    VariableList {
        variables,
        truncated,
    }
}

pub async fn eval(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    expression: &str,
    frame_id: Option<i64>,
    context: EvalContext,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    // Taken beside the check, not after the await below it. See `still_paused`.
    let fence = session.stop_generation();
    // An expression without a frame is evaluated in the global scope, where
    // none of the local variables the caller means exist. Defaulting to the
    // frame they are looking at is what they meant.
    let frame_id = Some(resolve_frame(&session, fence, frame_id).await?);
    still_paused(&session, fence)?;

    let mut result = session
        .adapter()
        .evaluate(expression, frame_id, context)
        .await
        .map_err(AdapterError::into_ipc)?;

    still_paused(&session, fence)?;
    if result.variables_reference != 0 {
        result.variables_reference =
            session.mint_handle(fence, HandleKind::Variables, result.variables_reference);
    }
    Ok(Response::Evaluated(result))
}

/// Refuse unless the session is still sitting at the stop `fence` was taken at.
///
/// `paused_session` is a check, not a hold — nothing here owns the session's
/// state, and another client is free to `continue` while this handler is
/// awaiting the adapter. A handler that reads a paused program in two steps
/// therefore has a window between them, and what falls into it is not
/// harmless: the second request reaches a *running* program, which answers
/// with values from wherever it has got to, or more often does not answer at
/// all until the adapter's own timeout fires ten seconds later. Neither reads
/// as "you asked about a program that is no longer stopped".
///
/// Comparing the generation rather than only re-reading the state is what makes
/// resume-and-stop-again fail too: that is a *different* stop, every frame id
/// resolved a moment ago addresses nothing in it, and answering would be the
/// right shape of reply about the wrong moment.
fn still_paused(session: &Arc<Session>, fence: u64) -> Result<()> {
    if session.still_at(fence) {
        return Ok(());
    }
    tracing::debug!(
        target: "daemon.session",
        session_id = %session.id,
        fence,
        now = session.stop_generation(),
        state = session.state().as_str(),
        "the program moved while this request was being prepared",
    );
    Err(IpcError::new(
        ErrorCode::SessionNotPaused,
        "the program resumed while this request was being prepared; \
         it is no longer stopped where the request was about",
    ))
}

/// Debuggee output the daemon buffered, without asking the adapter anything.
///
/// Works on a finished session too, which is the point: the output of a
/// program that has already exited is often exactly what is wanted.
pub fn output(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    since_ms: Option<u64>,
) -> Result<Response> {
    let session = find_session(state, session_id)?;
    let (chunks, dropped) = session.buffered_output(since_ms);
    Ok(Response::Output { chunks, dropped })
}

/// The adapter's frame id for what a request named, or the top of the stack.
///
/// An explicit `--frame` is one of *our* handles and is checked against this
/// stop before anything is asked of the adapter. It used to be passed straight
/// through, which is how `eval --frame 0` came back claiming the process was
/// running while it was plainly stopped: codelldb reports an unresolvable frame
/// that way, and an agent reading it starts polling a program that is not going
/// anywhere. `scopes --frame 0` said `Invalid frame reference: 0`, which is at
/// least true; now neither reaches the adapter at all (D075).
///
/// A request that named nothing gets the top frame, fetched rather than
/// remembered: the adapter's ids are only valid until the program moves.
async fn resolve_frame(session: &Arc<Session>, fence: u64, explicit: Option<i64>) -> Result<i64> {
    if let Some(frame_id) = explicit {
        return session.resolve_handle(fence, HandleKind::Frame, frame_id);
    }

    let thread_id = stopped_thread(session)?;
    let (frames, _) = session
        .adapter()
        .stack_trace(thread_id, Some(0), Some(1))
        .await
        .map_err(AdapterError::into_ipc)?;

    frames.first().map(|frame| frame.id).ok_or_else(|| {
        IpcError::new(
            ErrorCode::DapProtocolError,
            "the adapter reported a paused thread with no frames",
        )
    })
}

fn stopped_thread(session: &Arc<Session>) -> Result<i64> {
    session.last_thread_id().ok_or_else(|| {
        IpcError::new(
            ErrorCode::SessionNotPaused,
            "no thread has stopped yet, so there is nothing to inspect",
        )
    })
}
