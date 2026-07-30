//! Reading a stopped program.
//!
//! Every handler here refuses a session that is running: the stack and the
//! variables of a moving program describe a moment that has already passed
//! (`docs/blueprint/10-async-to-sync.md`). The one exception is [`output`],
//! which reads the daemon's own buffer and never touches the adapter.

use super::{Result, find_session, live_session, paused_session};
use crate::adapter::AdapterError;
use crate::state::{DaemonState, Session};
use lazydap_core::{EvalContext, SessionId, VariableFilter};
use lazydap_protocol::{ErrorCode, IpcError, Response};
use std::sync::Arc;

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
    let thread_id = match thread_id {
        Some(thread_id) => thread_id,
        None => stopped_thread(&session)?,
    };

    let (frames, total) = session
        .adapter()
        .stack_trace(thread_id, start_frame, levels)
        .await
        .map_err(AdapterError::into_ipc)?;
    Ok(Response::StackTrace { frames, total })
}

pub async fn scopes(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    frame_id: Option<i64>,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    let frame_id = resolve_frame(&session, frame_id).await?;

    let scopes = session
        .adapter()
        .scopes(frame_id)
        .await
        .map_err(AdapterError::into_ipc)?;
    Ok(Response::Scopes(scopes))
}

pub async fn variables(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    variables_reference: i64,
    filter: VariableFilter,
    start: Option<u32>,
    count: Option<u32>,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    let variables = session
        .adapter()
        .variables(variables_reference, filter, start, count)
        .await
        .map_err(AdapterError::into_ipc)?;
    Ok(Response::Variables(variables))
}

pub async fn eval(
    state: &Arc<DaemonState>,
    session_id: SessionId,
    expression: &str,
    frame_id: Option<i64>,
    context: EvalContext,
) -> Result<Response> {
    let session = paused_session(state, session_id)?;
    // An expression without a frame is evaluated in the global scope, where
    // none of the local variables the caller means exist. Defaulting to the
    // frame they are looking at is what they meant.
    let frame_id = Some(resolve_frame(&session, frame_id).await?);

    let result = session
        .adapter()
        .evaluate(expression, frame_id, context)
        .await
        .map_err(AdapterError::into_ipc)?;
    Ok(Response::Evaluated(result))
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

/// The frame a request means when it did not name one: the top of the stack.
///
/// Fetched rather than remembered, because frame ids are only valid until the
/// program moves, and a remembered one would be a stale handle the adapter
/// would either reject or — worse — answer about the wrong frame.
async fn resolve_frame(session: &Arc<Session>, explicit: Option<i64>) -> Result<i64> {
    if let Some(frame_id) = explicit {
        return Ok(frame_id);
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
