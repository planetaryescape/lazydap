//! The adapter seam: the only part of the daemon that knows what DAP is.
//!
//! Everything outside this module works in lazydap's own vocabulary —
//! [`lazydap_core`] types and [`lazydap_protocol`] events. DAP requests,
//! responses, `seq` numbers and camelCase field names stop here
//! (`ARCHITECTURE.md`, anti-pattern 4).
//!
//! D029 said this module would become a trait when the second adapter landed,
//! and M18 is that moment: [`DebugAdapter`] is the trait, [`codelldb`] and
//! [`debugpy`] are its implementations, and [`handshake`] is everything the
//! two do identically. The daemon depends on the trait rather than on either
//! adapter (non-negotiable #5), and the module boundary still does the
//! enforcing it always did: `lazydap_dap` is imported nowhere else in the
//! daemon, checked by `scripts/check_architecture_boundaries.sh`.
//!
//! The trait lives here rather than in `lazydap-core` (D052). Its methods
//! speak DAP — `adapterID`, launch arguments, a stop's `reason` string — and
//! moving it to a crate every other crate depends on would move the DAP
//! vocabulary with it, undoing the one thing this boundary exists to do.

// All private. Nothing outside this module names an adapter — it calls
// [`launch`] and gets a [`Launched`] back — and making that a visibility rule
// rather than a convention is what stops non-negotiable #5 from eroding one
// convenient `use` at a time.
mod codelldb;
mod debugpy;
mod delve;
mod handshake;
mod pump;
mod translate;

use lazydap_core::{
    AdapterBreakpoint, AdapterKind, Breakpoint, EvalContext, EvalResult, PauseReason, Scope,
    StackFrame, StepKind, ThreadInfo, Variable, VariableFilter,
};
use lazydap_dap::{
    ContinueArgs, ContinueResponse, DapRequest, DapResponse, DapWriter, DisconnectArgs,
    EvaluateArgs, EvaluateResponse, PauseArgs, ScopesArgs, ScopesResponse, SetBreakpointsArgs,
    SetBreakpointsResponse, Source, StackTraceArgs, StackTraceResponse, StepArgs, TcpSpawn,
    ThreadsResponse, TransportError, VariablesArgs, VariablesResponse,
};
use lazydap_protocol::{ErrorCode, IpcError, LaunchRequest};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};

pub use handshake::{Launched, launch};
pub use pump::spawn_pump;

/// What one debug adapter does differently from another.
///
/// Deliberately small. Everything a session does *after* the launch — stepping,
/// stacks, scopes, evaluation, breakpoints — is specified precisely enough
/// that both adapters answer the same requests the same way, and all of it
/// lives once in [`AdapterHandle`]. What is left is the launch: how to start
/// the adapter, what to call ourselves, what arguments its `launch` takes, and
/// the two places each adapter reports something in its own words rather than
/// the specification's.
///
/// Object-safe and synchronous on purpose. Starting the adapter is described
/// as a [`Spawn`] value rather than done by the trait, so no method here is
/// `async` — which keeps the trait usable behind `dyn` without pulling in a
/// procedural macro to box the futures. It also makes the difference between
/// the two adapters something a test can assert on rather than something it
/// has to run a process to observe.
pub trait DebugAdapter: Send + Sync {
    fn kind(&self) -> AdapterKind;

    /// How to start this adapter, given the command discovery resolved for it.
    fn spawn(&self, command: &Path) -> Spawn;

    /// What to put in `initialize`'s `adapterID`. Adapters branch on it.
    fn adapter_id(&self) -> &'static str;

    /// This adapter's `launch` arguments, as JSON.
    ///
    /// JSON rather than a typed struct because the two adapters share only
    /// half their fields and disagree about the rest; each builds its own
    /// typed arguments and serialises them here, so the shape is still checked
    /// where it is written.
    fn launch_args(&self, request: &LaunchRequest) -> serde_json::Value;

    /// What lazydap calls a stop, and what the adapter called it when those
    /// differ.
    ///
    /// The default is to take the adapter at its word, which is right for any
    /// adapter that follows the specification — debugpy does. codelldb
    /// overrides it (D033, quirk 6).
    fn normalise_stop(
        &self,
        raw: &str,
        _description: &str,
        _stop_on_entry: bool,
    ) -> (PauseReason, Option<String>) {
        (PauseReason::from(raw), None)
    }

    /// The debuggee's pid, if this adapter only says it in console output.
    ///
    /// The default is `None`: an adapter that sends the DAP `process` event is
    /// already understood by the handshake, and needs nothing here. codelldb
    /// does not send it (quirk 9), so codelldb scrapes.
    fn debuggee_pid_in(&self, _output: &str) -> Option<u32> {
        None
    }
}

/// How to start an adapter and reach it once it is running.
///
/// The two shapes DAP adapters come in. Which one an adapter wants is a fact
/// about that adapter, so it is answered by [`DebugAdapter::spawn`] rather
/// than guessed at from the command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spawn {
    /// Start it and connect to the TCP port it announces as it comes up.
    /// Which stream, and under what words, is part of the recipe: codelldb and
    /// delve share nothing about their own startup.
    Tcp(TcpSpawn),
    /// Start it and speak DAP over its own stdin and stdout.
    Stdio { program: PathBuf, args: Vec<String> },
}

/// The implementation for one adapter kind.
///
/// `&'static` rather than boxed: no adapter carries any state — what they are
/// is which methods they implement — so there is nothing to allocate.
pub fn for_kind(kind: AdapterKind) -> &'static dyn DebugAdapter {
    match kind {
        AdapterKind::Codelldb => &codelldb::CodeLldb,
        AdapterKind::Debugpy => &debugpy::Debugpy,
        AdapterKind::Delve => &delve::Delve,
    }
}

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

    #[error("{adapter} is pinned to {path} by {pinned_by}, and that is not an executable")]
    ConfiguredAdapterMissing {
        adapter: AdapterKind,
        path: PathBuf,
        /// Which file said so: [`PIN_USER_CONFIG`] or [`PIN_LAUNCH_CONFIG`].
        /// Not named `source`, which `thiserror` reads as the underlying error
        /// rather than as a field to interpolate.
        pinned_by: &'static str,
    },

    /// Found, and cannot do the job. Only debugpy can reach this: it is a
    /// Python module, so an interpreter being present says nothing about the
    /// module being installed in it.
    #[error("{path} cannot act as {adapter}: {problem} — {hint}")]
    Incomplete {
        adapter: AdapterKind,
        path: PathBuf,
        problem: String,
        hint: String,
    },

    #[error("cannot read lazydap's config: {source}")]
    Config {
        #[source]
        source: lazydap_config::ConfigError,
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
            Self::ConfiguredAdapterMissing {
                adapter,
                ref path,
                pinned_by,
            } => IpcError::new(
                ErrorCode::AdapterNotFound,
                format!(
                    "{adapter} is pinned to {} by {pinned_by}, and that is not an executable",
                    path.display(),
                ),
            )
            .with_details(serde_json::json!({
                "adapter": adapter.as_str(),
                "configured": path,
            })),
            // Also `AdapterNotFound`: from a caller's point of view there is
            // no usable adapter of this kind, which is the same situation and
            // the same exit code (4). The hint is what makes it actionable.
            Self::Incomplete {
                adapter,
                ref path,
                ref hint,
                ..
            } => {
                let message = self.to_string();
                IpcError::new(ErrorCode::AdapterNotFound, message).with_details(serde_json::json!({
                    "adapter": adapter.as_str(),
                    "found": path,
                    "hint": hint,
                }))
            }
            // Not `AdapterNotFound`: the adapter was never looked for. A
            // caller that retries with a different adapter would keep hitting
            // the same broken file.
            Self::Config { ref source } => {
                IpcError::new(ErrorCode::InvalidLaunchConfig, source.to_string())
            }
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
    /// if a second arrives while the first is outstanding (ptvsd #1502).
    ///
    /// The permit is taken by the *caller*, not by the methods below, and held
    /// for the whole run — send, acknowledgement, and the wait for the program
    /// to settle. Releasing it at the acknowledgement is not enough: a second
    /// `continue --wait` would then take it while the first is still waiting,
    /// resume the program past the stop the first was about to report, and
    /// hand that stop back as its own. "One in flight" has to mean one *run*,
    /// not one message.
    ///
    /// [`interrupt`](AdapterHandle::interrupt) and
    /// [`disconnect`](AdapterHandle::disconnect) deliberately do not take it.
    /// Both exist to end a run that is already under way, and a queue is
    /// exactly the wrong place for the thing that breaks the queue.
    execution: Mutex<()>,
    pending: Pending,
}

/// Proof that the holder may move the program.
///
/// Taking it is how a caller joins the execution queue; holding it is what
/// the type system checks. It is a parameter on the execution methods rather
/// than something they acquire themselves so that the *caller* controls the
/// ordering — specifically, so a `--wait` can subscribe to events after taking
/// the permit but before sending, which is the only ordering that is free of
/// both a lost stop and a stolen one.
pub struct ExecutionPermit<'a> {
    _guard: tokio::sync::MutexGuard<'a, ()>,
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

    /// Join the execution queue. Held until the returned permit is dropped.
    pub async fn execution_permit(&self) -> ExecutionPermit<'_> {
        ExecutionPermit {
            _guard: self.execution.lock().await,
        }
    }

    /// Send one request and wait for its response, typed both ways.
    async fn call<A: Serialize, R: DeserializeOwned>(&self, command: &str, args: &A) -> Result<R> {
        let body = self.request(command, args).await?;
        decode(command, body)
    }

    /// Send one execution request, and give up on the adapter if it does not
    /// answer.
    ///
    /// An acknowledgement timeout is not retryable. The request is still
    /// outstanding at the adapter, so sending another would put two execution
    /// requests in flight — the exact thing D021 and non-negotiable #6 exist
    /// to prevent, and the one that deadlocks adapters which serialise
    /// internally. There is also no way to withdraw the first.
    ///
    /// So the adapter is treated as gone: killed here, which makes the pump's
    /// next read fail and turns the session into `adapter_died` (D022). A
    /// debug adapter that cannot acknowledge a `continue` within the deadline
    /// is wedged, and an honest failure beats a poisoned session that looks
    /// usable.
    async fn execute<A: Serialize>(&self, command: &str, args: &A) -> Result<serde_json::Value> {
        match self.request(command, args).await {
            Err(AdapterError::Timeout { command, timeout }) => {
                tracing::warn!(
                    target: "daemon.session",
                    command,
                    timeout_s = timeout.as_secs(),
                    "the adapter did not acknowledge an execution request; killing it",
                );
                self.kill().await;
                Err(AdapterError::Timeout { command, timeout })
            }
            other => other,
        }
    }

    /// Send one request and wait for its response.
    ///
    /// Deliberately does *not* touch the execution permit: whether this
    /// request needs one, and for how long, is the caller's decision. See the
    /// field comment on `execution`.
    async fn request<A: Serialize>(&self, command: &str, args: &A) -> Result<serde_json::Value> {
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

    // --- Execution. One run at a time, per D021. ---

    /// Resume. Answers as soon as the adapter acknowledges — the program is
    /// running by then, and what it does next arrives as events.
    ///
    /// The permit is a parameter rather than something taken here: the caller
    /// holds it across the whole run, not just this message.
    pub async fn resume(&self, _permit: &ExecutionPermit<'_>, thread_id: i64) -> Result<bool> {
        let body = self
            .execute("continue", &ContinueArgs { thread_id })
            .await?;
        let response: ContinueResponse = decode("continue", body)?;
        Ok(response.all_threads_continued)
    }

    pub async fn step(
        &self,
        _permit: &ExecutionPermit<'_>,
        kind: StepKind,
        thread_id: i64,
    ) -> Result<()> {
        self.execute(
            translate::step_command(kind),
            &StepArgs {
                thread_id,
                granularity: None,
            },
        )
        .await?;
        Ok(())
    }

    /// Ask the program to stop. The stop itself arrives as an event later, or
    /// not at all if the program ended in the meantime.
    ///
    /// Takes no permit. `pause` exists to interrupt a run that is already
    /// under way, so queueing it behind that run would mean the only way to
    /// stop a runaway program is to wait for it to stop.
    pub async fn interrupt(&self, thread_id: i64) -> Result<()> {
        self.request("pause", &PauseArgs { thread_id }).await?;
        Ok(())
    }

    /// End the debug session, optionally killing the debuggee with it.
    ///
    /// Takes no permit, for the same reason `interrupt` does not: giving up on
    /// a session must not queue behind the session.
    pub async fn disconnect(&self, terminate: bool) -> Result<()> {
        self.request(
            "disconnect",
            &DisconnectArgs {
                terminate_debuggee: terminate,
            },
        )
        .await?;
        Ok(())
    }

    // --- Inspection. Safe concurrently, within a stable state. ---

    pub async fn threads(&self) -> Result<Vec<ThreadInfo>> {
        let response: ThreadsResponse = self.call("threads", &serde_json::json!({})).await?;
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
        let response: ScopesResponse = self.call("scopes", &ScopesArgs { frame_id }).await?;
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
            )
            .await?;
        Ok(translate::eval_result(response))
    }

    /// Tell the adapter about every breakpoint in one source file.
    ///
    /// The whole file, every time: `setBreakpoints` *replaces* a source's list
    /// rather than adding to it, so sending only the new one silently removes
    /// the rest. See `docs/reference/dap-protocol-cheatsheet.md`.
    ///
    /// Sent twice at most: an adapter that declines the path but names one it
    /// could have used gets a second chance under that name (quirk 8).
    pub async fn set_breakpoints(
        &self,
        source: &Path,
        breakpoints: &[Breakpoint],
    ) -> Result<Vec<AdapterBreakpoint>> {
        let applied = self.send_breakpoints(source, breakpoints).await?;

        let Some(rebound) = rebind_source(source, &applied) else {
            return Ok(applied);
        };
        tracing::debug!(
            target: "daemon.session",
            requested = %source.display(),
            rebound = %rebound.display(),
            "the adapter would not bind this path but named another spelling of it; re-sending (quirk 8)",
        );
        self.send_breakpoints(&rebound, breakpoints).await
    }

    async fn send_breakpoints(
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
            )
            .await?;

        Ok(translate::reconcile_breakpoints(
            breakpoints,
            response.breakpoints,
        ))
    }

    /// Answer a reverse request with "no".
    ///
    /// Takes no permit and no reply from the caller: refusing is not a thing
    /// anybody chose to do, it is the only honest answer to a question lazydap
    /// never said it could answer. A failure to write the refusal is logged
    /// rather than returned — the pump is the only caller, it has nowhere to
    /// return an error to, and a write that fails means the adapter is going
    /// away anyway.
    pub async fn refuse(&self, request: &DapRequest) {
        let mut writer = self.writer.lock().await;
        let Some(writer) = writer.as_mut() else {
            return;
        };
        if let Err(error) = writer.refuse(request).await {
            tracing::warn!(
                target: "daemon.session",
                %error,
                command = request.command,
                "could not answer the adapter's reverse request",
            );
        }
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

/// The spelling of `requested` that this adapter would bind, when it refused
/// the one it was sent (quirk 8).
///
/// The failure this exists for: lazydap canonicalises source paths, the
/// compiler records whatever was typed on its command line, and codelldb
/// compares the two as strings. Under a symlinked directory — `/tmp` on macOS,
/// which is `/private/tmp` — those are different strings for one file, and
/// every breakpoint in the program silently fails to bind.
///
/// Two conditions, both necessary:
///
/// - **Nothing bound.** If some breakpoints in the file did bind, re-sending
///   the whole list under a second path would leave the adapter holding both
///   sets: two adapter breakpoints for one of ours, on one line.
/// - **The suggestion is the same file.** Resolved through the filesystem, not
///   compared as text. An adapter naming a path that resolves elsewhere is
///   offering to break in code the caller never asked about, and taking it up
///   on that would be worse than the unbound breakpoint.
///
/// Returning `None` leaves the breakpoints exactly as unverified as they
/// already were, which is what `break --list` has always shown.
fn rebind_source(requested: &Path, applied: &[AdapterBreakpoint]) -> Option<PathBuf> {
    if applied.is_empty() || applied.iter().any(|breakpoint| breakpoint.verified) {
        return None;
    }

    let candidate = applied
        .iter()
        .filter_map(|breakpoint| breakpoint.message.as_deref())
        .find_map(translate::suggested_location)?;
    if candidate == requested {
        return None;
    }

    let same_file =
        std::fs::canonicalize(&candidate).ok()? == std::fs::canonicalize(requested).ok()?;
    same_file.then_some(candidate)
}

/// Read a response body as the shape DAP says it is.
///
/// One place, so an execution request and a query report a malformed answer
/// the same way — and name the command that sent it, which is the only part
/// of a `serde` error a reader can act on.
fn decode<R: DeserializeOwned>(command: &str, body: serde_json::Value) -> Result<R> {
    serde_json::from_value(body).map_err(|source| AdapterError::Malformed {
        command: command.to_string(),
        source,
    })
}

/// Find the binary for `kind`.
///
/// Two of D026's three tiers: the user's config file, then `PATH`. The middle
/// one — a lazydap-managed `{data_dir}/adapters/` — is still not here, because
/// nothing installs an adapter into it. A lookup in a directory no code ever
/// writes to is dead code pretending to be a policy, which is the same reason
/// M5 shipped only `PATH`.
///
/// The config is read on every launch rather than once at daemon start: the
/// daemon may be days old, and somebody who has just pinned a new codelldb
/// build expects the next launch to use it, not the next reboot.
pub fn discover(kind: AdapterKind) -> Result<PathBuf> {
    let config = lazydap_config::load_config().map_err(|source| AdapterError::Config { source })?;
    discover_with(kind, &config)
}

/// Discovery against a config somebody else has already loaded.
///
/// This is what the *client* calls (D050). The config file and `PATH` both
/// describe the machine as the person typing the command sees it, and the
/// daemon sees neither: it may have been started days ago, from another
/// directory, without the `LAZYDAP_CONFIG_PATH` now in force. Resolving there
/// would read a different config than the one the caller set and fall through
/// to `PATH` without saying so.
pub fn discover_with(kind: AdapterKind, config: &lazydap_config::Config) -> Result<PathBuf> {
    discover_in(kind, config, &std::env::var_os("PATH").unwrap_or_default())
}

/// The adapter binary for a launch, honouring one the launch configuration
/// named.
///
/// A `launch.json` debugpy configuration routinely pins its interpreter —
/// `"python": "${workspaceFolder}/.venv/bin/python"` — and that pin is the
/// entire point of a per-project virtualenv: the named interpreter has the
/// project's dependencies and the first one on `PATH` does not. So it replaces
/// discovery rather than seeding it.
///
/// It is checked here, in the client, for the same reason everything else
/// about a launch is (D050) — and checked at all because the failure it
/// prevents is otherwise unrecognisable: an interpreter without debugpy
/// installed starts, fails to import a module, and is reported as an adapter
/// that crashed on startup.
pub fn resolve_with(
    kind: AdapterKind,
    config: &lazydap_config::Config,
    pinned: Option<&Path>,
) -> Result<PathBuf> {
    let Some(pinned) = pinned else {
        return discover_with(kind, config);
    };

    if !is_executable(pinned) {
        return Err(AdapterError::ConfiguredAdapterMissing {
            adapter: kind,
            path: pinned.to_path_buf(),
            pinned_by: PIN_LAUNCH_CONFIG,
        });
    }
    usable(kind, pinned)?;

    tracing::debug!(
        target: "daemon.session",
        adapter = %kind,
        path = %pinned.display(),
        "using the adapter the launch configuration named",
    );
    Ok(pinned.to_path_buf())
}

/// Where a pinned adapter path came from, for an error that has to name it.
/// A caller told to check "lazydap's config" when the path is in their
/// `launch.json` goes looking in the wrong file.
const PIN_USER_CONFIG: &str = "lazydap's config";
const PIN_LAUNCH_CONFIG: &str = "the launch configuration";

/// The lookup itself, with the config and `PATH` passed in so tests do not
/// have to mutate the process environment (which edition 2024 makes `unsafe`,
/// and this workspace forbids).
fn discover_in(
    kind: AdapterKind,
    config: &lazydap_config::Config,
    path: &std::ffi::OsStr,
) -> Result<PathBuf> {
    // What to look for on `PATH`. debugpy is not a binary: it is a module, so
    // what is being searched for is an interpreter that can import it, and
    // both spellings are worth trying because a machine with only `python` is
    // still a machine that can run it.
    let binaries: &[&str] = match kind {
        AdapterKind::Codelldb => &["codelldb"],
        AdapterKind::Debugpy => &["python3", "python"],
        AdapterKind::Delve => &["dlv"],
    };

    // Tier one. A pinned path that is wrong is an error rather than a reason
    // to carry on down the list: falling through to `PATH` would debug a
    // different build of the adapter than the one somebody deliberately
    // chose, and say nothing about having done so.
    if let Some(command) = config.adapter_command(kind) {
        if !is_executable(command) {
            return Err(AdapterError::ConfiguredAdapterMissing {
                adapter: kind,
                path: command.to_path_buf(),
                pinned_by: PIN_USER_CONFIG,
            });
        }
        usable(kind, command)?;
        tracing::debug!(target: "daemon.session", adapter = %kind, path = %command.display(), "using the adapter pinned in the config");
        return Ok(command.to_path_buf());
    }

    let mut searched = Vec::new();
    // An interpreter that was found and cannot do the job. Remembered so the
    // failure can name it: "no python3 anywhere" and "the python3 you have
    // cannot import debugpy" are different problems with different fixes, and
    // reporting the second as the first sends somebody to reinstall Python.
    let mut incomplete: Option<AdapterError> = None;

    for dir in std::env::split_paths(path) {
        for binary in binaries {
            let candidate = dir.join(binary);
            if !is_executable(&candidate) {
                continue;
            }
            match usable(kind, &candidate) {
                Ok(()) => {
                    tracing::debug!(target: "daemon.session", adapter = %kind, path = %candidate.display(), "found adapter");
                    return Ok(candidate);
                }
                // Keep looking: a machine can have several interpreters and
                // only one of them with debugpy installed.
                Err(error) => {
                    tracing::debug!(target: "daemon.session", adapter = %kind, path = %candidate.display(), %error, "found but not usable; still looking");
                    incomplete.get_or_insert(error);
                }
            }
        }
        searched.push(dir);
    }

    Err(incomplete.unwrap_or(AdapterError::NotFound {
        adapter: kind,
        searched,
    }))
}

/// Whether `command` can actually act as this adapter.
///
/// codelldb needs no asking: it *is* the adapter, so a binary at that path is
/// the thing. The other two are each one step removed from what discovery
/// finds, and each has a failure that is unrecognisable without a probe:
///
/// - debugpy is a *module*, so an interpreter on `PATH` proves nothing. Without
///   this the launch gets far enough to spawn a process, which dies with a
///   Python traceback about a missing module, reported as a crashed adapter.
/// - `dlv` is a binary, but DAP is a *subcommand* of it, added in 1.6. An
///   older one — or something else called `dlv` — starts, prints a usage
///   error, and never announces a port, which surfaces as a launch that timed
///   out for no stated reason.
///
/// Costs one process start-up per launch, tens of milliseconds against a launch
/// already measured in seconds, and buys the difference between an honest
/// `AdapterNotFound` and a mystery.
fn usable(kind: AdapterKind, command: &Path) -> Result<()> {
    match kind {
        AdapterKind::Codelldb => Ok(()),
        AdapterKind::Debugpy => {
            // Answered rather than merely exiting zero. A pinned command that
            // is not an interpreter at all — `/bin/echo` is the honest
            // accident, a shell wrapper the interesting one — succeeds at
            // almost any argument list, so "it did not fail" proves nothing.
            // Only something that really ran the program prints this back.
            const PROOF: &str = "lazydap-debugpy-ok";
            let answered = answers(
                command,
                &["-c".into(), format!("import debugpy; print('{PROOF}')")],
                |stdout| stdout.trim() == PROOF,
            );
            answered.then_some(()).ok_or_else(|| AdapterError::Incomplete {
                adapter: kind,
                path: command.to_path_buf(),
                problem: "it cannot import debugpy".to_string(),
                hint: format!(
                    "install it with `{} -m pip install debugpy`",
                    command.display(),
                ),
            })
        }
        AdapterKind::Delve => {
            // `dlv help dap` rather than `dlv version`: the version is printed
            // by every delve ever built, and what has to be true here is that
            // this one has the DAP subcommand at all.
            let answered = answers(command, &["help".into(), "dap".into()], |stdout| {
                stdout.contains("dap")
            });
            answered.then_some(()).ok_or_else(|| AdapterError::Incomplete {
                adapter: kind,
                path: command.to_path_buf(),
                problem: "it does not have delve's `dap` subcommand".to_string(),
                hint: "install a current one with `go install github.com/go-delve/delve/cmd/dlv@latest`"
                    .to_string(),
            })
        }
    }
}

/// Run `command` with `args` and ask whether its output proves what it should.
///
/// Deliberately not "did it exit zero": the commands being probed here are
/// stand-ins for something else, and a wrapper script that exits zero at
/// anything would pass that test while being unable to do the job.
fn answers(command: &Path, args: &[String], proof: impl Fn(&str) -> bool) -> bool {
    std::process::Command::new(command)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|output| output.status.success() && proof(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or(false)
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
            &lazydap_config::Config::default(),
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

    /// A config file and, optionally, a fake adapter binary beside it.
    fn pinned_config(label: &str, executable: bool) -> (lazydap_config::Config, PathBuf, PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("lazydap-discover-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create");

        let binary = dir.join("codelldb");
        if executable {
            std::fs::write(&binary, b"#!/bin/sh\n").expect("write");
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
        }

        let config_path = dir.join("config.toml");
        std::fs::write(
            &config_path,
            format!("[adapter.codelldb]\ncommand = \"{}\"\n", binary.display()),
        )
        .expect("write config");

        let config = lazydap_config::load_config_from(&config_path).expect("load config");
        (config, binary, dir)
    }

    #[test]
    fn an_adapter_pinned_in_the_config_wins_over_one_on_path() {
        let (config, binary, dir) = pinned_config("pinned", true);

        // A PATH that would otherwise answer: /bin has plenty of executables,
        // but none of them are what the config asked for.
        let found = discover_in(AdapterKind::Codelldb, &config, std::ffi::OsStr::new("/bin"))
            .expect("the pinned binary");

        assert_eq!(found, binary);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_pinned_adapter_that_is_not_there_is_an_error_rather_than_a_fallback() {
        // Falling through to PATH would debug a different build of codelldb
        // than the one somebody deliberately pinned, and say nothing about it.
        let (config, binary, dir) = pinned_config("missing", false);

        let error = discover_in(AdapterKind::Codelldb, &config, std::ffi::OsStr::new("/bin"))
            .expect_err("the pinned path does not exist");

        assert!(
            matches!(error, AdapterError::ConfiguredAdapterMissing { .. }),
            "got: {error}",
        );
        let ipc = error.into_ipc();
        assert_eq!(ipc.code, ErrorCode::AdapterNotFound);
        assert_eq!(ipc.details["configured"], serde_json::json!(binary));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A directory laid out like a virtualenv, with `bin/python` either a real
    /// interpreter (a copy of this machine's) or a stub that cannot import
    /// debugpy.
    fn fake_venv(label: &str, real_interpreter: bool) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!("lazydap-venv-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).expect("create");

        let python = bin.join("python");
        let script = match real_interpreter {
            // Delegates to whatever python is on PATH, so `import debugpy`
            // behaves exactly as it does for the interpreter that runs the
            // rest of this suite.
            true => "#!/bin/sh\nexec python3 \"$@\"\n",
            // An interpreter with no debugpy in it. `exit 0` on purpose: the
            // check must not be satisfied by a command that merely succeeds.
            false => "#!/bin/sh\nexit 0\n",
        };
        std::fs::write(&python, script).expect("write");
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        (dir, python)
    }

    #[test]
    fn an_interpreter_named_by_a_launch_configuration_wins_over_discovery() {
        // The virtualenv case: the whole reason the configuration names one is
        // that the interpreter on `PATH` is the wrong one.
        let (dir, python) = fake_venv("wins", true);

        let resolved = resolve_with(
            AdapterKind::Debugpy,
            &lazydap_config::Config::default(),
            Some(&python),
        );

        match resolved {
            Ok(found) => assert_eq!(found, python),
            // A machine with no debugpy cannot tell these apart, and the
            // integration suite already skips loudly for that.
            Err(AdapterError::Incomplete { .. }) => {}
            Err(other) => unreachable!("got: {other}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_named_interpreter_that_cannot_import_debugpy_says_so_and_names_it() {
        // Falling back to `PATH` here would run the program under an
        // interpreter the configuration deliberately did not choose, and say
        // nothing about having done so.
        let (dir, python) = fake_venv("empty", false);

        let error = resolve_with(
            AdapterKind::Debugpy,
            &lazydap_config::Config::default(),
            Some(&python),
        )
        .expect_err("a stub interpreter cannot import debugpy");

        let ipc = error.into_ipc();
        assert_eq!(ipc.code, ErrorCode::AdapterNotFound);
        assert_eq!(ipc.details["found"], serde_json::json!(python));
        assert!(
            ipc.message.contains("debugpy"),
            "the message has to name what is missing: {}",
            ipc.message,
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_interpreter_that_is_not_there_names_the_launch_configuration() {
        // Which file to go and fix. A caller sent to "lazydap's config" when
        // the path is in their launch.json looks in the wrong place.
        let error = resolve_with(
            AdapterKind::Debugpy,
            &lazydap_config::Config::default(),
            Some(Path::new("/nowhere/.venv/bin/python")),
        )
        .expect_err("that path does not exist");

        assert!(
            error.to_string().contains("the launch configuration"),
            "got: {error}",
        );
    }

    #[test]
    fn naming_no_interpreter_falls_back_to_discovery() {
        // Without a pin, `resolve_with` must be exactly `discover_with` —
        // asserted as "the same answer" rather than "some answer", so it holds
        // whether or not this machine has the adapter at all.
        let config = lazydap_config::Config::default();

        let resolved = resolve_with(AdapterKind::Codelldb, &config, None);
        let discovered = discover_with(AdapterKind::Codelldb, &config);

        match (resolved, discovered) {
            (Ok(resolved), Ok(discovered)) => assert_eq!(resolved, discovered),
            (Err(resolved), Err(discovered)) => {
                assert_eq!(resolved.to_string(), discovered.to_string());
            }
            (resolved, discovered) => {
                unreachable!("a pin of `None` must change nothing: {resolved:?} vs {discovered:?}",)
            }
        }
    }

    /// The message codelldb actually sends, captured from a real run.
    fn unbound(message: &str) -> AdapterBreakpoint {
        AdapterBreakpoint {
            id: Some(lazydap_core::BreakpointId(1)),
            adapter_id: Some(1),
            verified: false,
            line: Some(6),
            message: Some(message.to_string()),
        }
    }

    /// A file reachable by two names: one through a symlinked directory, one
    /// not. Exactly the `/tmp` → `/private/tmp` shape, built by hand so the
    /// test does not depend on the host having that particular symlink.
    struct TwoSpellings {
        real: PathBuf,
        through_link: PathBuf,
        root: PathBuf,
    }

    impl TwoSpellings {
        fn new(label: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("lazydap-rebind-{label}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let real_dir = root.join("real");
            std::fs::create_dir_all(&real_dir).expect("create");
            std::fs::write(real_dir.join("main.c"), b"int main(void){return 0;}\n").expect("write");
            std::os::unix::fs::symlink(&real_dir, root.join("link")).expect("symlink");

            Self {
                real: real_dir.join("main.c"),
                through_link: root.join("link").join("main.c"),
                root,
            }
        }
    }

    impl Drop for TwoSpellings {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn an_unbound_breakpoint_is_re_sent_under_the_spelling_the_adapter_named() {
        // Quirk 8, the whole of it: we canonicalised, the debug info did not,
        // and codelldb compared the two as strings.
        let files = TwoSpellings::new("same");
        let applied = [unbound(&format!(
            "Breakpoint at {}:6 could not be resolved, but a valid location was found at {}:6",
            files.real.display(),
            files.through_link.display(),
        ))];

        assert_eq!(
            rebind_source(&files.real, &applied),
            Some(files.through_link.clone()),
        );
    }

    #[test]
    fn a_suggestion_naming_a_different_file_is_refused() {
        // Binding here would set a breakpoint in code the caller never asked
        // about, which is worse than the breakpoint that did not bind.
        let files = TwoSpellings::new("other");
        let elsewhere = files.root.join("real").join("other.c");
        std::fs::write(&elsewhere, b"int other(void){return 1;}\n").expect("write");

        let applied = [unbound(&format!(
            "Breakpoint at {}:6 could not be resolved, but a valid location was found at {}:6",
            files.real.display(),
            elsewhere.display(),
        ))];

        assert_eq!(rebind_source(&files.real, &applied), None);
    }

    #[test]
    fn nothing_is_re_sent_when_part_of_the_file_bound() {
        // Re-sending the whole list under a second path would leave the
        // adapter holding both sets: two adapter breakpoints on one line.
        let files = TwoSpellings::new("partial");
        let applied = [
            AdapterBreakpoint {
                verified: true,
                ..unbound("")
            },
            unbound(&format!(
                "Breakpoint at {}:6 could not be resolved, but a valid location was found at {}:6",
                files.real.display(),
                files.through_link.display(),
            )),
        ];

        assert_eq!(rebind_source(&files.real, &applied), None);
    }

    #[test]
    fn an_unbound_breakpoint_with_no_suggestion_is_left_alone() {
        let applied = [unbound("Breakpoint at main.c:6 could not be resolved")];
        assert_eq!(rebind_source(Path::new("/p/main.c"), &applied), None);
    }

    #[test]
    fn a_suggestion_identical_to_what_was_sent_is_not_worth_a_second_request() {
        let files = TwoSpellings::new("identical");
        let applied = [unbound(&format!(
            "Breakpoint could not be resolved, but a valid location was found at {}:6",
            files.real.display(),
        ))];

        assert_eq!(rebind_source(&files.real, &applied), None);
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
