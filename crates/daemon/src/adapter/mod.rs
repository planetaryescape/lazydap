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

    #[error("{adapter} is pinned to {path} in lazydap's config, and that is not an executable")]
    ConfiguredAdapterMissing { adapter: AdapterKind, path: PathBuf },

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
            Self::ConfiguredAdapterMissing { adapter, ref path } => IpcError::new(
                ErrorCode::AdapterNotFound,
                format!(
                    "{adapter} is pinned to {} in lazydap's config, and that is not an executable",
                    path.display(),
                ),
            )
            .with_details(serde_json::json!({
                "adapter": adapter.as_str(),
                "configured": path,
            })),
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

/// The lookup itself, with the config and `PATH` passed in so tests do not
/// have to mutate the process environment (which edition 2024 makes `unsafe`,
/// and this workspace forbids).
fn discover_in(
    kind: AdapterKind,
    config: &lazydap_config::Config,
    path: &std::ffi::OsStr,
) -> Result<PathBuf> {
    let binary = match kind {
        AdapterKind::Codelldb => "codelldb",
    };

    // Tier one. A pinned path that is wrong is an error rather than a reason
    // to carry on down the list: falling through to `PATH` would debug a
    // different build of the adapter than the one somebody deliberately
    // chose, and say nothing about having done so.
    if let Some(command) = config.adapter_command(kind) {
        if is_executable(command) {
            tracing::debug!(target: "daemon.session", adapter = %kind, path = %command.display(), "using the adapter pinned in the config");
            return Ok(command.to_path_buf());
        }
        return Err(AdapterError::ConfiguredAdapterMissing {
            adapter: kind,
            path: command.to_path_buf(),
        });
    }

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
