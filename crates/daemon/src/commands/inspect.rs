//! Reading a paused program: stack, scopes, variables, expressions, output.

use super::{active_session_id, unexpected};
use crate::auto_spawn::ensure_daemon_running;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, Row, View, or_dash};
use lazydap_core::{EvalContext, VariableFilter};
use lazydap_protocol::{Request, Response};

pub async fn stack(
    instance: &Instance,
    thread: Option<i64>,
    start: Option<u32>,
    levels: Option<u32>,
    format: OutputFormat,
) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;

    let response = client
        .request(Request::StackTrace {
            session_id,
            thread_id: thread,
            start_frame: start,
            levels,
        })
        .await?;
    let Response::StackTrace { frames, total } = response else {
        return Err(unexpected(response));
    };

    let rows = frames
        .iter()
        .map(|frame| {
            let source = frame
                .source
                .as_ref()
                .map(|source| source.label())
                .unwrap_or_else(|| "-".to_string());
            Row::new(
                frame.id.to_string(),
                vec![
                    frame.id.to_string(),
                    frame.name.clone(),
                    source,
                    frame.line.to_string(),
                ],
                frame,
            )
        })
        .collect();

    View::list(
        serde_json::json!({ "frames": frames, "total": total }),
        &["frame", "name", "source", "line"],
        rows,
    )
    .print(format)
}

pub async fn scopes(instance: &Instance, frame: Option<i64>, format: OutputFormat) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;

    let response = client
        .request(Request::Scopes {
            session_id,
            frame_id: frame,
        })
        .await?;
    let Response::Scopes(scopes) = response else {
        return Err(unexpected(response));
    };

    let rows = scopes
        .iter()
        .map(|scope| {
            Row::new(
                // The reference, not the name: it is what the next command
                // needs.
                scope.variables_reference.to_string(),
                vec![
                    scope.name.clone(),
                    scope.variables_reference.to_string(),
                    scope.expensive.to_string(),
                ],
                scope,
            )
        })
        .collect();

    View::list(
        serde_json::json!({ "scopes": scopes }),
        &["name", "reference", "expensive"],
        rows,
    )
    .print(format)
}

pub async fn variables(
    instance: &Instance,
    reference: i64,
    filter: VariableFilter,
    start: Option<u32>,
    count: Option<u32>,
    max: Option<u32>,
    format: OutputFormat,
) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;

    let response = client
        .request(Request::Variables {
            session_id,
            variables_reference: reference,
            filter,
            start,
            count,
            max,
        })
        .await?;
    let Response::Variables(list) = response else {
        return Err(unexpected(response));
    };
    let truncated = list.truncated;
    let variables = list.variables;

    let rows = variables
        .iter()
        .map(|variable| {
            Row::new(
                // A variable's own reference, so `--format ids` feeds the next
                // `lazydap variables`. Scalars have `0`, which expands to
                // nothing.
                variable.variables_reference.to_string(),
                vec![
                    variable.name.clone(),
                    variable.value.clone(),
                    or_dash(variable.type_name.as_ref()),
                    variable.variables_reference.to_string(),
                ],
                variable,
            )
        })
        .collect();

    if truncated {
        // On stderr so it cannot corrupt a pipeline, but said out loud: a
        // partial list that looks complete is the thing the cap must not
        // create. The JSON carries `truncated` for anything parsing it.
        eprintln!(
            "warning: more variables than the cap; showing a prefix. \
             Use `--start {}` for the next page, or `--max 0` for all of them",
            start.unwrap_or(0) as usize + variables.len(),
        );
    }

    View::list(
        serde_json::json!({ "variables": variables, "truncated": truncated }),
        &["name", "value", "type", "reference"],
        rows,
    )
    .print(format)
}

pub async fn eval(
    instance: &Instance,
    expression: &str,
    frame: Option<i64>,
    context: EvalContext,
    format: OutputFormat,
) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;

    let response = client
        .request(Request::Eval {
            session_id,
            expression: expression.to_string(),
            frame_id: frame,
            context,
        })
        .await?;
    let Response::Evaluated(result) = response else {
        return Err(unexpected(response));
    };

    View::single(
        serde_json::to_value(&result).map_err(CliError::general)?,
        match &result.type_name {
            Some(type_name) => format!("{} ({type_name})", result.value),
            None => result.value.clone(),
        },
    )
    .print(format)
}

pub async fn threads(instance: &Instance, format: OutputFormat) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;

    let response = client.request(Request::Threads { session_id }).await?;
    let Response::Threads(threads) = response else {
        return Err(unexpected(response));
    };

    let rows = threads
        .iter()
        .map(|thread| {
            Row::new(
                thread.id.to_string(),
                // Blank when the adapter named nothing. A cell reading
                // "thread 0" would be lazydap's invention, not an answer
                // (D065).
                vec![
                    thread.id.to_string(),
                    thread.name.clone().unwrap_or_default(),
                ],
                thread,
            )
        })
        .collect();

    View::list(
        serde_json::json!({ "threads": threads }),
        &["id", "name"],
        rows,
    )
    .print(format)
}

pub async fn output(instance: &Instance, since: Option<u64>, format: OutputFormat) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let session_id = active_session_id(&mut client).await?;

    let response = client
        .request(Request::Output {
            session_id,
            since_ms: since,
        })
        .await?;
    let Response::Output { chunks, dropped } = response else {
        return Err(unexpected(response));
    };

    let rows = chunks
        .iter()
        .map(|chunk| {
            Row::new(
                chunk.timestamp_ms.to_string(),
                vec![
                    chunk.timestamp_ms.to_string(),
                    chunk.category.to_string(),
                    chunk.output.trim_end().to_string(),
                ],
                chunk,
            )
        })
        .collect();

    if dropped > 0 {
        // On stderr, so it cannot corrupt a pipeline reading stdout — but said
        // out loud, because a partial transcript that looks complete is worse
        // than no transcript.
        eprintln!("warning: {dropped} event(s) were dropped before anybody read them");
    }

    View::list(
        serde_json::json!({ "chunks": chunks, "dropped": dropped }),
        &["timestamp_ms", "category", "output"],
        rows,
    )
    .print(format)
}
