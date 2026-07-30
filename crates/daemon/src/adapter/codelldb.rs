//! Starting a debuggee under codelldb.
//!
//! The sequence is the one proven in M3/M4 against the real adapter, and its
//! two non-obvious steps are load-bearing:
//!
//! 1. `launch` is written without awaiting its response. codelldb holds that
//!    response until after `configurationDone`, and `configurationDone` is
//!    what we send in reaction to the `initialized` event — waiting on the
//!    response first deadlocks.
//! 2. Nothing here uses a shared read loop. The handshake owns the whole
//!    transport, and hands it to the pump only once the session is live.

use super::{AdapterError, AdapterHandle, Pending, Result, discover, rebind_source, translate};
use lazydap_core::{
    AdapterBreakpoint, AdapterKind, Breakpoint, OutputCategory, OutputChunk, PauseReason,
    SessionState,
};
use lazydap_dap::{
    Capabilities, ConfigurationDoneArgs, DapEvent, DapReader, DapTransport, Incoming,
    InitializeArgs, LaunchArgs, SetBreakpointsArgs, SetBreakpointsResponse, Source,
};
use lazydap_protocol::{AdapterCapabilities, LaunchRequest};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Longest the whole launch may take, start to paused-or-running.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest to wait for any single message during the launch.
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(15);
/// How long to keep reading after `terminated`, in case `exited` is behind it.
const POST_TERMINATION_GRACE: Duration = Duration::from_millis(250);

/// A session that is up, plus the read half the pump still has to be given.
pub struct Launched {
    pub handle: AdapterHandle,
    pub pump: PumpStart,
    pub capabilities: AdapterCapabilities,
    pub state: SessionState,
    pub reason: Option<PauseReason>,
    /// What the adapter called the stop, when we renamed it (quirk 6).
    pub raw_reason: Option<String>,
    pub thread_id: Option<i64>,
    /// What the adapter made of the breakpoints applied during configuration.
    pub breakpoints: Vec<AdapterBreakpoint>,
    /// Set when the debuggee finished during its own launch.
    pub exit_code: Option<i32>,
    /// Debuggee output produced before the pump took over. Seeded into the
    /// session buffer so a `stop_on_entry` launch does not silently lose the
    /// first lines.
    pub output: Vec<OutputChunk>,
    /// The process the adapter started, when it said which (quirk 8).
    ///
    /// Read from the launch output rather than by the pump, because the line
    /// carrying it arrives *during* the handshake — the pump does not own the
    /// reads until after this returns, so it never sees it.
    pub debuggee_pid: Option<u32>,
}

/// The read half plus the map the pump delivers responses into. Opaque on
/// purpose: DAP types do not leave this module.
pub struct PumpStart {
    pub(super) reader: DapReader,
    pub(super) pending: Pending,
}

/// Start `request.program` under codelldb and wait for it to settle.
///
/// `breakpoints` are the project's, grouped by source file. They go in during
/// the configuration phase — after `initialized`, before `configurationDone` —
/// which is the only window DAP gives for breakpoints that must be live before
/// the first instruction runs.
pub async fn launch(
    request: &LaunchRequest,
    breakpoints: &[(PathBuf, Vec<Breakpoint>)],
) -> Result<Launched> {
    // What the client resolved, when it said (D050). Its config file and its
    // `PATH` are the ones the caller meant; falling back to our own lookup is
    // for a client too old to have sent one, and the protocol version makes
    // that impossible today.
    let adapter_path = match &request.adapter_command {
        Some(path) => path.clone(),
        None => discover(AdapterKind::Codelldb)?,
    };
    let mut transport = DapTransport::spawn(&adapter_path.to_string_lossy()).await?;

    match handshake(&mut transport, request, breakpoints).await {
        Ok(outcome) => {
            let (reader, writer) = transport.split();
            let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
            Ok(Launched {
                handle: AdapterHandle::new(writer, Arc::clone(&pending)),
                pump: PumpStart { reader, pending },
                capabilities: outcome.capabilities,
                state: outcome.state,
                reason: outcome.reason,
                raw_reason: outcome.raw_reason,
                thread_id: outcome.thread_id,
                breakpoints: outcome.breakpoints,
                exit_code: outcome.exit_code,
                debuggee_pid: outcome
                    .output
                    .iter()
                    .find_map(|chunk| launched_pid(&chunk.output)),
                output: outcome.output,
            })
        }
        Err(error) => {
            // A failed handshake may have left a read cancelled mid-frame, so
            // the transport is not reusable. Take the adapter down with it
            // rather than leaking a process nobody can talk to.
            let _ = transport.shutdown().await;
            Err(error)
        }
    }
}

struct Outcome {
    capabilities: AdapterCapabilities,
    state: SessionState,
    reason: Option<PauseReason>,
    raw_reason: Option<String>,
    thread_id: Option<i64>,
    breakpoints: Vec<AdapterBreakpoint>,
    exit_code: Option<i32>,
    output: Vec<OutputChunk>,
}

async fn handshake(
    transport: &mut DapTransport,
    request: &LaunchRequest,
    breakpoints: &[(PathBuf, Vec<Breakpoint>)],
) -> Result<Outcome> {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;

    let capabilities: Capabilities = with_timeout(
        "initialize",
        deadline,
        transport.request("initialize", &InitializeArgs::new("lldb")),
    )
    .await??;

    let launch_seq = transport
        .send_request("launch", &launch_args(request))
        .await?;
    tracing::debug!(target: "daemon.session", program = %request.program.display(), "launch sent");

    let mut configuration_done_seq: Option<i64> = None;
    // Which source each outstanding `setBreakpoints` was for, so its response
    // can be paired back up with what we asked for.
    let mut breakpoint_seqs: HashMap<i64, usize> = HashMap::new();
    // Sources already re-sent under the adapter's own spelling (quirk 8). One
    // retry each: the second answer is taken as final however it reads, so two
    // components disagreeing about a path cannot loop here.
    let mut rebound: HashSet<usize> = HashSet::new();
    let mut launch_answered = false;
    let mut outcome = Outcome {
        capabilities: translate_capabilities(&capabilities),
        state: SessionState::Running,
        reason: None,
        raw_reason: None,
        thread_id: None,
        breakpoints: Vec::new(),
        exit_code: Option::None,
        output: Vec::new(),
    };

    loop {
        // Waiting for a `stopped` we asked for is not optional: a caller that
        // said `--stop-on-entry` and got "running" back would be lied to.
        // Neither is waiting for the breakpoints: reporting a launch as ready
        // while the adapter has not answered whether the breakpoints took
        // would make `verified` a race.
        let settled = launch_answered
            && configuration_done_seq.is_some()
            && breakpoint_seqs.is_empty()
            && (!request.stop_on_entry || outcome.reason.is_some());
        if settled || outcome.state == SessionState::Terminated {
            return Ok(outcome);
        }

        match with_timeout("adapter message", deadline, transport.read_incoming()).await?? {
            Incoming::Event(event) => match event.event.as_str() {
                // The configuration window. Breakpoints first, then
                // `configurationDone` — in that order on the wire, because
                // that order is what makes them live before the first
                // instruction runs.
                "initialized" => {
                    for (index, (source, in_source)) in breakpoints.iter().enumerate() {
                        let seq = transport
                            .send_request(
                                "setBreakpoints",
                                &set_breakpoints_args(source, in_source),
                            )
                            .await?;
                        breakpoint_seqs.insert(seq, index);
                    }
                    configuration_done_seq = Some(
                        transport
                            .send_request("configurationDone", &ConfigurationDoneArgs {})
                            .await?,
                    );
                }
                "output" => outcome.output.extend(output_chunk(&event)),
                "stopped" => {
                    let body = event.body.unwrap_or_default();
                    let raw = body["reason"].as_str().unwrap_or("unknown");
                    let description = body["description"].as_str().unwrap_or_default();

                    outcome.state = SessionState::Paused;
                    let (reason, raw_reason) =
                        normalise_stop(raw, description, request.stop_on_entry);
                    outcome.reason = Some(reason);
                    outcome.raw_reason = raw_reason;
                    outcome.thread_id = body["threadId"].as_i64();
                }
                // A program short enough to finish during its own launch is
                // unusual but not an error; report it as it happened. The exit
                // code arrives on its own event, and losing it here would mean
                // a debuggee that finished this fast could never report how it
                // went.
                "exited" => outcome.exit_code = exit_code_of(&event),
                "terminated" => {
                    outcome.state = SessionState::Terminated;
                    // `exited` may still be behind `terminated` on the wire —
                    // DAP does not fix their order — and returning the instant
                    // the session ends would lose the exit code from the
                    // `Launched` response for good. The pump can record it
                    // later, but the ending has already been emitted by then.
                    // Same grace window M6's `--wait` needs, for the same
                    // reason.
                    drain_for_exit_code(transport, &mut outcome).await;
                    return Ok(outcome);
                }
                _ => {}
            },
            Incoming::Response(response) => {
                if !response.success {
                    return Err(AdapterError::Rejected {
                        command: response.command,
                        message: response.message.unwrap_or_default(),
                    });
                }
                if response.request_seq == launch_seq {
                    launch_answered = true;
                }
                if let Some(index) = breakpoint_seqs.remove(&response.request_seq) {
                    let (source, in_source) = &breakpoints[index];
                    let applied = applied_breakpoints(in_source, response.body);

                    // Quirk 8: the adapter would not bind the path we sent but
                    // named one it could. Ask again under that name, and let
                    // the retry's answer replace this one rather than join it
                    // — the caller would otherwise see each breakpoint twice.
                    match rebind_source(source, &applied).filter(|_| rebound.insert(index)) {
                        Some(path) => {
                            tracing::debug!(
                                target: "daemon.session",
                                requested = %source.display(),
                                rebound = %path.display(),
                                "re-sending breakpoints under the path the adapter named (quirk 8)",
                            );
                            let seq = transport
                                .send_request(
                                    "setBreakpoints",
                                    &set_breakpoints_args(&path, in_source),
                                )
                                .await?;
                            breakpoint_seqs.insert(seq, index);
                        }
                        None => outcome.breakpoints.extend(applied),
                    }
                }
            }
        }
    }
}

/// Read whatever is already on its way for a moment, hoping for the `exited`
/// that carries the debuggee's status.
///
/// Best-effort by construction: the session is over, so nothing here is worth
/// waiting long for, and everything is worth having if it turns up.
///
/// A timeout leaves the reader mid-frame, which is normally fatal — but not
/// here. The only consumer left is the pump, whose remaining job is to notice
/// the adapter is gone and reap it, and a misparse gets it there just as
/// surely as a clean EOF. The session has already ended, so its `end_once` is
/// a no-op either way.
async fn drain_for_exit_code(transport: &mut DapTransport, outcome: &mut Outcome) {
    let deadline = Instant::now() + POST_TERMINATION_GRACE;

    while outcome.exit_code.is_none() {
        let Ok(Ok(message)) = tokio::time::timeout_at(deadline, transport.read_incoming()).await
        else {
            return;
        };
        if let Incoming::Event(event) = message {
            match event.event.as_str() {
                "exited" => outcome.exit_code = exit_code_of(&event),
                "output" => outcome.output.extend(output_chunk(&event)),
                _ => {}
            }
        }
    }
}

/// What lazydap calls a stop, and what the adapter called it if those differ.
///
/// codelldb implements entry-stop by letting the process start and sending it
/// `SIGSTOP`; LLDB classifies a signal stop as an exception, so a launch that
/// did exactly what was asked reports `reason: "exception"`
/// (`docs/reference/codelldb-quirks.md`, quirk 6). An agent reading that
/// concludes the program crashed before `main`.
///
/// So the first stop of a `--stop-on-entry` launch, and only that one, is
/// renamed to `entry` — and the adapter's own word is kept in `raw_reason`,
/// so the normalisation is visible rather than a quiet substitution (D033).
/// The guard is deliberately narrow: a real exception at the entry point
/// would not carry `SIGSTOP`, and every later stop passes through untouched.
fn normalise_stop(
    raw: &str,
    description: &str,
    stop_on_entry: bool,
) -> (PauseReason, Option<String>) {
    let reason = PauseReason::from(raw);
    let is_entry_signal =
        matches!(reason, PauseReason::Exception) && description.contains("SIGSTOP");

    if stop_on_entry && is_entry_signal {
        tracing::debug!(
            target: "daemon.session",
            raw_reason = raw,
            description,
            "reporting codelldb's SIGSTOP entry stop as `entry` (quirk 6)",
        );
        return (PauseReason::Entry, Some(raw.to_string()));
    }
    (reason, None)
}

fn set_breakpoints_args(
    source: &std::path::Path,
    breakpoints: &[Breakpoint],
) -> SetBreakpointsArgs {
    SetBreakpointsArgs {
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
    }
}

/// Read a `setBreakpoints` response body, or report nothing rather than
/// failing the launch.
///
/// A launch that succeeded should not be thrown away because the adapter
/// described its breakpoints oddly: the program is running, and unverified
/// breakpoints are visible in `break --list` either way.
fn applied_breakpoints(
    requested: &[Breakpoint],
    body: Option<serde_json::Value>,
) -> Vec<AdapterBreakpoint> {
    let Some(body) = body else {
        return Vec::new();
    };
    match serde_json::from_value::<SetBreakpointsResponse>(body) {
        Ok(response) => translate::reconcile_breakpoints(requested, response.breakpoints),
        Err(error) => {
            tracing::warn!(
                target: "daemon.session",
                %error,
                "could not read the adapter's setBreakpoints answer",
            );
            Vec::new()
        }
    }
}

fn exit_code_of(event: &DapEvent) -> Option<i32> {
    event
        .body
        .as_ref()
        .and_then(|body| body["exitCode"].as_i64())
        .map(|code| code as i32)
}

fn launch_args(request: &LaunchRequest) -> LaunchArgs {
    LaunchArgs {
        adapter_type: "lldb".into(),
        request: "launch".into(),
        program: request.program.to_string_lossy().into_owned(),
        args: request.args.clone(),
        cwd: request.cwd.to_string_lossy().into_owned(),
        stop_on_entry: request.stop_on_entry,
        env: if request.env.is_empty() {
            None
        } else {
            Some(request.env.clone().into_iter().collect())
        },
        // codelldb defaults to the integrated terminal, which needs a
        // runInTerminal reverse request we deliberately do not advertise.
        // "console" keeps the debuggee attached so its stdout arrives as DAP
        // output events.
        terminal: Some("console".into()),
    }
}

fn translate_capabilities(capabilities: &Capabilities) -> AdapterCapabilities {
    AdapterCapabilities {
        supports_configuration_done_request: capabilities.supports_configuration_done_request,
        supports_function_breakpoints: capabilities.supports_function_breakpoints,
        supports_conditional_breakpoints: capabilities.supports_conditional_breakpoints,
    }
}

/// One chunk of output from a DAP `output` event.
pub(super) fn output_chunk(event: &DapEvent) -> Option<OutputChunk> {
    let body = event.body.as_ref()?;
    let output = body["output"].as_str()?;
    let category = OutputCategory::from(body["category"].as_str().unwrap_or("console"));
    Some(OutputChunk::new(category, output))
}

/// The debuggee's pid, scraped from the line codelldb prints when it starts one.
///
/// DAP has a `process` event carrying `systemProcessId` and this is what it is
/// for. codelldb does not send it — the string is not in its binary, and a full
/// launch-to-exit stream carries `output`, `initialized`, `module`, `continued`,
/// `exited` and `terminated` and nothing else. What it does print, to the
/// console category, is:
///
/// ```text
/// Launched process 56254 from '/path/to/program'
/// ```
///
/// So that is where the pid comes from (quirk 8). Scraping a human-readable
/// line is exactly as brittle as it looks, which is why every caller treats a
/// `None` as "carry on without it": the only thing it costs is the best-effort
/// cleanup in [`crate::debuggee`].
pub(super) fn launched_pid(output: &str) -> Option<u32> {
    let rest = output.strip_prefix("Launched process ")?;
    let (pid, _) = rest.split_once(char::is_whitespace)?;
    pid.parse().ok()
}

/// Bound a step of the handshake by both its own timeout and the overall one.
///
/// A timeout here is terminal for the transport — reads are not
/// cancellation-safe — which is why every caller of this abandons the adapter
/// rather than trying again.
async fn with_timeout<F: Future>(command: &str, deadline: Instant, future: F) -> Result<F::Output> {
    let step_deadline = deadline.min(Instant::now() + MESSAGE_TIMEOUT);
    tokio::time::timeout_at(step_deadline, future)
        .await
        .map_err(|_| AdapterError::Timeout {
            command: command.to_string(),
            timeout: MESSAGE_TIMEOUT,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn request() -> LaunchRequest {
        LaunchRequest {
            adapter: AdapterKind::Codelldb,
            program: PathBuf::from("/tmp/hello"),
            args: vec!["--fast".into()],
            cwd: PathBuf::from("/tmp"),
            env: BTreeMap::new(),
            stop_on_entry: true,
            adapter_command: None,
        }
    }

    #[test]
    fn launch_arguments_keep_the_debuggee_attached_to_the_adapter() {
        let json = serde_json::to_string(&launch_args(&request())).expect("serialise");

        assert!(json.contains(r#""terminal":"console""#), "got: {json}");
        assert!(json.contains(r#""stopOnEntry":true"#), "got: {json}");
        assert!(json.contains(r#""args":["--fast"]"#), "got: {json}");
        assert!(
            !json.contains("env"),
            "an empty environment must be omitted, not sent as null: {json}",
        );
    }

    #[test]
    fn an_output_event_becomes_a_typed_chunk() {
        let event: DapEvent = serde_json::from_str(
            r#"{"seq":1,"type":"event","event":"output",
                "body":{"category":"stdout","output":"hello\n"}}"#,
        )
        .expect("deserialise");

        let chunk = output_chunk(&event).expect("a chunk");
        assert_eq!(chunk.category, OutputCategory::Stdout);
        assert_eq!(chunk.output, "hello\n");
        assert!(chunk.category.is_debuggee());
    }

    #[test]
    fn a_sigstop_entry_pause_is_reported_as_entry_with_the_adapter_s_word_kept() {
        // Quirk 6: this is what codelldb actually sends for a stop-on-entry
        // launch on macOS. An agent reading "exception" concludes the program
        // crashed before main.
        let (reason, raw) = normalise_stop("exception", "signal SIGSTOP", true);
        assert_eq!(reason, PauseReason::Entry);
        assert_eq!(raw.as_deref(), Some("exception"), "nothing is hidden");
    }

    #[test]
    fn a_real_exception_at_the_entry_point_is_still_an_exception() {
        let (reason, raw) = normalise_stop("exception", "EXC_BAD_ACCESS", true);
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None, "nothing was renamed, so nothing to disclose");
    }

    #[test]
    fn a_sigstop_stop_nobody_asked_for_is_left_alone() {
        // Without `--stop-on-entry` a SIGSTOP is somebody else's doing, and
        // calling it an entry stop would be an invention.
        let (reason, raw) = normalise_stop("exception", "signal SIGSTOP", false);
        assert_eq!(reason, PauseReason::Exception);
        assert_eq!(raw, None);
    }

    #[test]
    fn an_adapter_that_follows_the_spec_needs_no_normalising() {
        let (reason, raw) = normalise_stop("entry", "", true);
        assert_eq!(reason, PauseReason::Entry);
        assert_eq!(raw, None);
    }

    #[test]
    fn breakpoints_are_sent_with_the_source_named_as_well_as_pathed() {
        let breakpoints = [Breakpoint {
            id: lazydap_core::BreakpointId(1),
            source: PathBuf::from("/tmp/main.c"),
            line: 19,
            column: None,
            condition: Some("x > 5".into()),
            hit_condition: None,
            log_message: None,
            enabled: true,
        }];
        let json = serde_json::to_string(&set_breakpoints_args(
            std::path::Path::new("/tmp/main.c"),
            &breakpoints,
        ))
        .expect("serialise");

        assert!(json.contains(r#""path":"/tmp/main.c""#), "got: {json}");
        assert!(json.contains(r#""name":"main.c""#), "got: {json}");
        assert!(json.contains(r#""condition":"x > 5""#), "got: {json}");
    }

    #[test]
    fn a_launch_survives_an_unreadable_set_breakpoints_answer() {
        let applied = applied_breakpoints(&[], Some(serde_json::json!({ "nonsense": true })));
        assert!(
            applied.is_empty(),
            "the program is running; throwing the launch away over this would be worse",
        );
    }

    #[test]
    fn an_output_event_without_a_body_is_not_a_chunk() {
        let event: DapEvent = serde_json::from_str(r#"{"seq":1,"type":"event","event":"output"}"#)
            .expect("deserialise");
        assert!(output_chunk(&event).is_none());
    }
}
