//! The adapter seam: the only part of the daemon that knows what DAP is.
//!
//! Everything outside this module works in lazydap's own vocabulary —
//! [`lazydap_core`] types and [`lazydap_protocol`] events. DAP requests,
//! responses, `seq` numbers and camelCase field names stop here
//! (`ARCHITECTURE.md`, anti-pattern 4).
//!
//! There is deliberately no `DebugAdapter` trait yet. v0.1 ships one adapter
//! (D013), and a trait with a single implementor is ceremony that hides where
//! the real seam is. The seam is this module boundary, and it is checked the
//! only way that actually holds: `lazydap_dap` is imported nowhere else in the
//! daemon. When debugpy arrives at M18 this module becomes the trait and its
//! first implementation. See D029.

pub mod codelldb;
mod pump;
mod translate;

use lazydap_core::{
    AdapterBreakpoint, AdapterKind, Breakpoint, EvalContext, EvalResult, Scope, StackFrame,
    StepKind, ThreadInfo, Variable, VariableFilter,
};
use lazydap_dap::{
    ContinueArgs, ContinueResponse, DapResponse, DapWriter, DisconnectArgs, EvaluateArgs,
    EvaluateResponse, PauseArgs, ScopesArgs, ScopesResponse, SetBreakpointsArgs,
    SetBreakpointsResponse, Source, StackTraceArgs, StackTraceResponse, StepArgs, ThreadsResponse,
    TransportError, VariablesArgs, VariablesResponse,
};
use lazydap_protocol::{ErrorCode, IpcError};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};

pub use pump::spawn_pump;

/// How long to wait for the adapter to answer one request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Requests we have sent and not yet seen a response to, keyed by DAP `seq`.
///
/// The pump owns delivery; request callers own the waiting end.
pub(crate) type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<DapResponse<serde_json::Value>>>>>;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("no {adapter} binary found on PATH")]
    NotFound {
        adapter: AdapterKind,
        searched: Vec<PathBuf>,
    },

    #[error("the adapter is no longer running")]
    Gone,

    #[error("the adapter did not answer `{command}` within {}s", .timeout.as_secs())]
    Timeout { command: String, timeout: Duration },

    #[error("the adapter rejected `{command}`: {message}")]
    Rejected { command: String, message: String },

    #[error("the adapter's `{command}` answer was not the shape DAP describes: {source}")]
    Malformed {
        command: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("dap transport: {0}")]
    Transport(#[from] TransportError),
}

impl AdapterError {
    /// Translate into the error clients see. The mapping is the contract:
    /// agents branch on the code, so a missing binary must never arrive as a
    /// generic internal error.
    pub fn into_ipc(self) -> IpcError {
        match self {
            Self::NotFound { adapter, searched } => IpcError::new(
                ErrorCode::AdapterNotFound,
                format!("no {adapter} binary found on PATH"),
            )
            .with_details(serde_json::json!({
                "adapter": adapter.as_str(),
                "searched": searched,
            })),
            Self::Gone => IpcError::new(ErrorCode::AdapterCrashed, "the adapter exited"),
            Self::Timeout { ref command, .. } => {
                let message = self.to_string();
                IpcError::new(ErrorCode::AdapterTimeout, message)
                    .with_details(serde_json::json!({ "command": command }))
            }
            Self::Rejected {
                ref command,
                ref message,
            } => IpcError::new(ErrorCode::DapProtocolError, self.to_string()).with_details(
                serde_json::json!({
                    "command": command,
                    "adapter_message": message,
                }),
            ),
            Self::Malformed { ref command, .. } => {
                let message = self.to_string();
                IpcError::new(ErrorCode::DapProtocolError, message)
                    .with_details(serde_json::json!({ "command": command }))
            }
            Self::Transport(_) => IpcError::new(ErrorCode::AdapterCrashed, self.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, AdapterError>;

/// The live end of an adapter: send it requests, or pull the plug.
pub struct AdapterHandle {
    /// `None` once the adapter has been killed. Held only for the write
    /// itself, so writes are serialised but waiting for a response is not.
    writer: Mutex<Option<DapWriter>>,
    /// The execution queue (D021, non-negotiable #6).
    ///
    /// Some adapters serialise execution requests internally and can deadlock
    /// if a second arrives while the first is outstanding (ptvsd #1502), so an
    /// execution request holds this permit across the write *and* the wait for
    /// its response — serialising the writer alone would still let two be in
    /// flight at once.
    ///
    /// The permit covers the DAP request and its acknowledgement, and stops
    /// there. A `continue --wait` goes on waiting for events without it, which
    /// it has to: `pause` is itself an execution request, and holding the
    /// permit for the whole wait would mean the one command that interrupts a
    /// runaway program queues behind the program.
    execution: Mutex<()>,
    pending: Pending,
}

/// Whether a request moves the program or merely asks it about itself.
///
/// Only the first kind queues. Inspection is safe to run concurrently: it is
/// synchronous within a stable state, and serialising it would make a stack
/// query wait behind whatever else was going on for no reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    Execution,
    Query,
}

impl AdapterHandle {
    pub(crate) fn new(writer: DapWriter, pending: Pending) -> Self {
        Self {
            writer: Mutex::new(Some(writer)),
            execution: Mutex::new(()),
            pending,
        }
    }

    /// A handle whose adapter has already gone.
    ///
    /// Not a fake adapter — it is the real state an `AdapterHandle` reaches
    /// once [`kill`](Self::kill) has run, which is what lets session-lifecycle
    /// tests exercise a `Session` without a live codelldb behind it.
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        Self {
            writer: Mutex::new(None),
            execution: Mutex::new(()),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Send one request and wait for its response, typed both ways.
    async fn call<A: Serialize, R: DeserializeOwned>(
        &self,
        command: &str,
        args: &A,
        class: Class,
    ) -> Result<R> {
        let body = self.request(command, args, class).await?;
        serde_json::from_value(body).map_err(|source| AdapterError::Malformed {
            command: command.to_string(),
            source,
        })
    }

    /// Send one request and wait for its response.
    ///
    /// An execution request takes the permit first and holds it for the whole
    /// cycle; see the field comment on `execution` for why the writer lock
    /// alone is not enough.
    async fn request<A: Serialize>(
        &self,
        command: &str,
        args: &A,
        class: Class,
    ) -> Result<serde_json::Value> {
        let _permit = match class {
            Class::Execution => Some(self.execution.lock().await),
            Class::Query => None,
        };

        let receiver = {
            let mut writer = self.writer.lock().await;
            let writer = writer.as_mut().ok_or(AdapterError::Gone)?;

            // Take the pending map *before* writing. The pump needs it to
            // deliver a response, so holding it across the write is what makes
            // "response arrives before we registered for it" impossible. The
            // pump cannot deadlock on this: it is waiting on the socket, and
            // the write is what unblocks it.
            let mut pending = self.pending.lock().await;
            let (sender, receiver) = oneshot::channel();
            let seq = writer.send_request(command, args).await?;
            pending.insert(seq, sender);
            receiver
        };

        let response = match tokio::time::timeout(REQUEST_TIMEOUT, receiver).await {
            Ok(Ok(response)) => response,
            // The pump dropped the sender, which it only does when the adapter
            // is gone.
            Ok(Err(_)) => return Err(AdapterError::Gone),
            Err(_) => {
                return Err(AdapterError::Timeout {
                    command: command.to_string(),
                    timeout: REQUEST_TIMEOUT,
                });
            }
        };

        if !response.success {
            return Err(AdapterError::Rejected {
                command: command.to_string(),
                message: response.message.unwrap_or_default(),
            });
        }
        Ok(response.body.unwrap_or(serde_json::Value::Null))
    }

    // --- Execution. One at a time, per D021. ---

    /// Resume. Answers as soon as the adapter acknowledges — the program is
    /// running by then, and what it does next arrives as events.
    pub async fn resume(&self, thread_id: i64) -> Result<bool> {
        let response: ContinueResponse = self
            .call("continue", &ContinueArgs { thread_id }, Class::Execution)
            .await?;
        Ok(response.all_threads_continued)
    }

    pub async fn step(&self, kind: StepKind, thread_id: i64) -> Result<()> {
        self.request(
            translate::step_command(kind),
            &StepArgs {
                thread_id,
                granularity: None,
            },
            Class::Execution,
        )
        .await?;
        Ok(())
    }

    /// Ask the program to stop. The stop itself arrives as an event later, or
    /// not at all if the program ended in the meantime.
    pub async fn interrupt(&self, thread_id: i64) -> Result<()> {
        self.request("pause", &PauseArgs { thread_id }, Class::Execution)
            .await?;
        Ok(())
    }

    /// End the debug session, optionally killing the debuggee with it.
    pub async fn disconnect(&self, terminate: bool) -> Result<()> {
        self.request(
            "disconnect",
            &DisconnectArgs {
                terminate_debuggee: terminate,
            },
            Class::Execution,
        )
        .await?;
        Ok(())
    }

    // --- Inspection. Safe concurrently, within a stable state. ---

    pub async fn threads(&self) -> Result<Vec<ThreadInfo>> {
        let response: ThreadsResponse = self
            .call("threads", &serde_json::json!({}), Class::Query)
            .await?;
        Ok(response
            .threads
            .into_iter()
            .map(translate::thread_info)
            .collect())
    }

    /// Frames, newest first, plus the total when the adapter knows it.
    pub async fn stack_trace(
        &self,
        thread_id: i64,
        start_frame: Option<u32>,
        levels: Option<u32>,
    ) -> Result<(Vec<StackFrame>, Option<u32>)> {
        let response: StackTraceResponse = self
            .call(
                "stackTrace",
                &StackTraceArgs {
                    thread_id,
                    start_frame,
                    levels,
                },
                Class::Query,
            )
            .await?;

        let frames = response
            .stack_frames
            .into_iter()
            .map(translate::stack_frame)
            .collect();
        Ok((frames, response.total_frames))
    }

    pub async fn scopes(&self, frame_id: i64) -> Result<Vec<Scope>> {
        let response: ScopesResponse = self
            .call("scopes", &ScopesArgs { frame_id }, Class::Query)
            .await?;
        Ok(response.scopes.into_iter().map(translate::scope).collect())
    }

    pub async fn variables(
        &self,
        variables_reference: i64,
        filter: VariableFilter,
        start: Option<u32>,
        count: Option<u32>,
    ) -> Result<Vec<Variable>> {
        let response: VariablesResponse = self
            .call(
                "variables",
                &VariablesArgs {
                    variables_reference,
                    filter: filter.as_dap().map(str::to_string),
                    start,
                    count,
                },
                Class::Query,
            )
            .await?;
        Ok(response
            .variables
            .into_iter()
            .map(translate::variable)
            .collect())
    }

    pub async fn evaluate(
        &self,
        expression: &str,
        frame_id: Option<i64>,
        context: EvalContext,
    ) -> Result<EvalResult> {
        let response: EvaluateResponse = self
            .call(
                "evaluate",
                &EvaluateArgs {
                    expression: expression.to_string(),
                    frame_id,
                    context: Some(context.as_str().to_string()),
                },
                Class::Query,
            )
            .await?;
        Ok(translate::eval_result(response))
    }

    /// Tell the adapter about every breakpoint in one source file.
    ///
    /// The whole file, every time: `setBreakpoints` *replaces* a source's list
    /// rather than adding to it, so sending only the new one silently removes
    /// the rest. See `docs/reference/dap-protocol-cheatsheet.md`.
    pub async fn set_breakpoints(
        &self,
        source: &Path,
        breakpoints: &[Breakpoint],
    ) -> Result<Vec<AdapterBreakpoint>> {
        let response: SetBreakpointsResponse = self
            .call(
                "setBreakpoints",
                &SetBreakpointsArgs {
                    source: Source {
                        path: source.to_string_lossy().into_owned(),
                        name: source
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned()),
                    },
                    breakpoints: breakpoints
                        .iter()
                        .map(translate::source_breakpoint)
                        .collect(),
                    source_modified: None,
                },
                Class::Query,
            )
            .await?;

        Ok(translate::reconcile_breakpoints(
            breakpoints,
            response.breakpoints,
        ))
    }

    /// Kill the adapter process and reap it.
    ///
    /// Idempotent: the pump calls this when the socket dies, and `disconnect`
    /// calls it afterwards to be sure.
    pub async fn kill(&self) {
        let writer = self.writer.lock().await.take();
        if let Some(writer) = writer
            && let Err(error) = writer.shutdown().await
        {
            tracing::warn!(target: "daemon.session", %error, "could not kill the adapter");
        }
    }
}

/// Find the binary for `kind`.
///
/// M5 looks on `PATH` only. The full order decided in D026 is config file >
/// lazydap-managed directory > `PATH`; the first two need the config loader
/// that lands with M15/M18, and adding empty lookups now would just be dead
/// code pretending to be a policy.
pub fn discover(kind: AdapterKind) -> Result<PathBuf> {
    discover_in(kind, &std::env::var_os("PATH").unwrap_or_default())
}

/// The lookup itself, with `PATH` passed in so tests do not have to mutate the
/// process environment (which edition 2024 makes `unsafe`, and this workspace
/// forbids).
fn discover_in(kind: AdapterKind, path: &std::ffi::OsStr) -> Result<PathBuf> {
    let binary = match kind {
        AdapterKind::Codelldb => "codelldb",
    };

    let mut searched = Vec::new();
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(binary);
        if is_executable(&candidate) {
            tracing::debug!(target: "daemon.session", adapter = %kind, path = %candidate.display(), "found adapter");
            return Ok(candidate);
        }
        searched.push(dir);
    }

    Err(AdapterError::NotFound {
        adapter: kind,
        searched,
    })
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_adapter_reports_where_it_looked() {
        let error = discover_in(
            AdapterKind::Codelldb,
            std::ffi::OsStr::new("/nonexistent-a:/nonexistent-b"),
        )
        .expect_err("codelldb cannot be on a PATH of missing directories");

        let ipc = error.into_ipc();
        assert_eq!(ipc.code, ErrorCode::AdapterNotFound);
        assert_eq!(ipc.details["adapter"], "codelldb");
        assert_eq!(
            ipc.details["searched"].as_array().map(Vec::len),
            Some(2),
            "got: {}",
            ipc.details,
        );
    }

    #[test]
    fn a_rejected_request_keeps_the_adapter_s_own_words() {
        let ipc = AdapterError::Rejected {
            command: "launch".to_string(),
            message: "could not find the program".to_string(),
        }
        .into_ipc();

        assert_eq!(ipc.code, ErrorCode::DapProtocolError);
        assert_eq!(ipc.details["adapter_message"], "could not find the program");
    }
}
