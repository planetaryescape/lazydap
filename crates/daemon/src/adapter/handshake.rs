//! Starting a debuggee, in the parts every adapter does the same way.
//!
//! The sequence is the one proven in M3/M4 against real codelldb and, since
//! M18, real debugpy. Its two non-obvious steps are load-bearing for both:
//!
//! 1. `launch` is written without awaiting its response. Both adapters hold
//!    that response until after `configurationDone`, and `configurationDone`
//!    is what we send in reaction to the `initialized` event — waiting on the
//!    response first deadlocks. debugpy is the stricter of the two: it does
//!    not send `initialized` until it has seen a `launch` at all.
//! 2. Nothing here uses a shared read loop. The handshake owns the whole
//!    transport, and hands it to the pump only once the session is live.
//!
//! What differs between adapters — which transport, which `adapterID`, which
//! launch arguments, what a stop is called, where the debuggee's pid comes
//! from — reaches this module through [`DebugAdapter`](super::DebugAdapter).

use super::{
    AdapterError, AdapterHandle, DebugAdapter, Pending, Result, Spawn, StopContext, discover,
    for_kind, rebind_source, translate,
};
use lazydap_core::{
    AdapterBreakpoint, Breakpoint, OutputCategory, OutputChunk, PauseReason, SessionState,
};
use lazydap_dap::{
    Capabilities, ConfigurationDoneArgs, DapEvent, DapReader, DapTransport, Incoming,
    InitializeArgs, SetBreakpointsArgs, SetBreakpointsResponse, Source,
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
    /// What the adapter called the stop, when we renamed it (D033).
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
    /// The process the adapter started, when it said which.
    ///
    /// Read during the handshake rather than by the pump, because the message
    /// carrying it arrives *while* the handshake owns the reads. debugpy and
    /// delve say so in the DAP `process` event; codelldb only says it in
    /// console text (quirk 9). The pump watches for a late `process` event as
    /// well, because nothing guarantees it arrives before the launch settles.
    pub debuggee: Option<StartedProcess>,
}

/// The read half plus the map the pump delivers responses into. Opaque on
/// purpose: DAP types do not leave this module.
pub struct PumpStart {
    pub(super) reader: DapReader,
    pub(super) pending: Pending,
}

/// Start `request.program` under the adapter it names and wait for it to
/// settle.
///
/// `breakpoints` are the project's, grouped by source file. They go in during
/// the configuration phase — after `initialized`, before `configurationDone` —
/// which is the only window DAP gives for breakpoints that must be live before
/// the first instruction runs.
pub async fn launch(
    request: &LaunchRequest,
    breakpoints: &[(PathBuf, Vec<Breakpoint>)],
) -> Result<Launched> {
    let adapter = for_kind(request.adapter);

    // What the client resolved, when it said (D050). Its config file and its
    // `PATH` are the ones the caller meant; falling back to our own lookup is
    // for a client too old to have sent one, and the protocol version makes
    // that impossible today.
    let adapter_path = match &request.adapter_command {
        Some(path) => path.clone(),
        None => discover(request.adapter)?,
    };

    let mut transport = match adapter.spawn(&adapter_path) {
        Spawn::Tcp(spawn) => DapTransport::spawn_tcp(&spawn).await?,
        Spawn::Stdio { program, args } => {
            DapTransport::spawn_stdio(program.as_os_str(), &args).await?
        }
    };

    match handshake(adapter, &mut transport, request, breakpoints).await {
        Ok(outcome) => {
            let (reader, writer) = transport.split();
            let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
            Ok(Launched {
                handle: AdapterHandle::new(
                    writer,
                    Arc::clone(&pending),
                    adapter,
                    outcome.capabilities,
                ),
                pump: PumpStart { reader, pending },
                capabilities: outcome.capabilities,
                state: outcome.state,
                reason: outcome.reason,
                raw_reason: outcome.raw_reason,
                thread_id: outcome.thread_id,
                breakpoints: outcome.breakpoints,
                exit_code: outcome.exit_code,
                // codelldb sends no `process` event, so its pid is scraped out
                // of console text and comes with no program name — which is
                // right for codelldb, whose debuggee *is* the program that was
                // launched (quirk 9).
                debuggee: outcome.debuggee.or_else(|| {
                    outcome
                        .output
                        .iter()
                        .find_map(|chunk| adapter.debuggee_pid_in(&chunk.output))
                        .map(|pid| StartedProcess { pid, program: None })
                }),
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
    debuggee: Option<StartedProcess>,
}

async fn handshake(
    adapter: &'static dyn DebugAdapter,
    transport: &mut DapTransport,
    request: &LaunchRequest,
    breakpoints: &[(PathBuf, Vec<Breakpoint>)],
) -> Result<Outcome> {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;

    let capabilities: Capabilities = with_timeout(
        "initialize",
        deadline,
        transport.request("initialize", &InitializeArgs::new(adapter.adapter_id())),
    )
    .await??;

    let launch_seq = transport
        .send_request("launch", &adapter.launch_args(request))
        .await?;
    tracing::debug!(
        target: "daemon.session",
        adapter = %adapter.kind(),
        program = %request.program.display(),
        "launch sent",
    );

    let mut configuration_done_seq: Option<i64> = None;
    // Which source each outstanding `setBreakpoints` was for, so its response
    // can be paired back up with what we asked for.
    let mut breakpoint_seqs: HashMap<i64, usize> = HashMap::new();
    // Sources already re-sent under the adapter's own spelling (quirk 8, the /tmp rebind). One
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
        debuggee: None,
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
                // The debuggee's pid, said the way the spec says to say it.
                // debugpy sends this; codelldb does not (quirk 9), which is
                // what `debuggee_pid_in` is for.
                "process" => outcome.debuggee = started_process(&event),
                "stopped" => {
                    let body = event.body.unwrap_or_default();
                    let raw = body["reason"].as_str().unwrap_or("unknown");
                    let description = body["description"].as_str().unwrap_or_default();

                    outcome.state = SessionState::Paused;
                    let (reason, raw_reason) = adapter.normalise_stop(
                        raw,
                        description,
                        StopContext {
                            stop_on_entry: request.stop_on_entry,
                            // There is no session yet, so nobody can have asked
                            // for a pause.
                            pause_requested: false,
                        },
                    );
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
            // Nothing lazydap advertises should provoke one of these, and
            // every launch it builds is configured not to. An adapter that
            // asks anyway gets a refusal it can read, rather than a silence it
            // waits out until the handshake deadline blames the wrong thing.
            Incoming::ReverseRequest(reverse) => transport.refuse(&reverse).await?,
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

/// The process the adapter started, as it described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedProcess {
    pub pid: u32,
    /// What the adapter says it actually ran.
    ///
    /// Not always what lazydap asked it to run, and that difference is why
    /// this is carried at all. delve's `mode: "debug"` *compiles* the `.go`
    /// file it is given and runs the resulting binary, so the process in the
    /// table is a path lazydap never named. The reaper identifies a debuggee by
    /// matching its command line against what was launched (D045), and against
    /// the `.go` path that match fails — so it declines to kill, and a Go
    /// debuggee whose adapter died survives. Taking the adapter's word for what
    /// it ran fixes that for every adapter at once (D061).
    pub program: Option<PathBuf>,
}

/// The debuggee from a DAP `process` event.
///
/// Only a *local* process is reported. `isLocalProcess: false` means the
/// adapter is describing something on another machine, and a pid from another
/// machine's namespace names an unrelated process on this one — which the
/// reaper in [`crate::debuggee`] would then be entitled to kill.
pub(super) fn started_process(event: &DapEvent) -> Option<StartedProcess> {
    let body = event.body.as_ref()?;
    if body["isLocalProcess"] == serde_json::Value::Bool(false) {
        return None;
    }
    Some(StartedProcess {
        pid: body["systemProcessId"].as_u64()? as u32,
        // Only an absolute path is worth having. `name` is documented as
        // something to show a user, and an adapter that fills it with a bare
        // program name or a label would have the reaper matching that against
        // `ps` output and killing whatever agreed.
        program: body["name"]
            .as_str()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute()),
    })
}

fn translate_capabilities(capabilities: &Capabilities) -> AdapterCapabilities {
    AdapterCapabilities {
        supports_configuration_done_request: capabilities.supports_configuration_done_request,
        supports_function_breakpoints: capabilities.supports_function_breakpoints,
        supports_conditional_breakpoints: capabilities.supports_conditional_breakpoints,
        supports_variable_paging: capabilities.supports_variable_paging,
    }
}

/// One chunk of output from a DAP `output` event.
pub(super) fn output_chunk(event: &DapEvent) -> Option<OutputChunk> {
    let body = event.body.as_ref()?;
    let output = body["output"].as_str()?;
    let category = OutputCategory::from(body["category"].as_str().unwrap_or("console"));
    Some(OutputChunk::new(category, output))
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
    use std::path::Path;

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
    fn an_output_event_without_a_body_is_not_a_chunk() {
        let event: DapEvent = serde_json::from_str(r#"{"seq":1,"type":"event","event":"output"}"#)
            .expect("deserialise");
        assert!(output_chunk(&event).is_none());
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

    /// The event debugpy actually sends, captured from a real run.
    #[test]
    fn a_process_event_carries_the_debuggee_s_pid() {
        let event: DapEvent = serde_json::from_str(
            r#"{"seq":1,"type":"event","event":"process","body":{
                "startMethod":"launch","isLocalProcess":true,
                "systemProcessId":93720,"name":"/tmp/main.py","pointerSize":64}}"#,
        )
        .expect("deserialise");

        let started = started_process(&event).expect("a process");
        assert_eq!(started.pid, 93720);
        assert_eq!(started.program.as_deref(), Some(Path::new("/tmp/main.py")));
    }

    /// The event delve actually sends for `mode: "debug"`, captured from a real
    /// run. `name` is the binary it compiled — not the `.go` file lazydap
    /// asked it to run, which is the whole reason the name is read (D061).
    #[test]
    fn a_compiled_debuggee_is_identified_by_what_the_adapter_ran() {
        let event: DapEvent = serde_json::from_str(
            r#"{"seq":1,"type":"event","event":"process","body":{
                "name":"/tmp/lazydap-delve-90235-1785499820892284000",
                "systemProcessId":90248,"isLocalProcess":true,
                "startMethod":"launch"}}"#,
        )
        .expect("deserialise");

        let started = started_process(&event).expect("a process");
        assert_eq!(
            started.program.as_deref(),
            Some(Path::new("/tmp/lazydap-delve-90235-1785499820892284000")),
            "matching the .go path against this would decline to reap it",
        );
    }

    #[test]
    fn a_name_that_is_not_a_path_is_not_used_to_identify_anything() {
        // `name` is documented as something to show a user. An adapter that
        // fills it with a label would have the reaper matching that against
        // `ps` output and killing whatever agreed.
        let event: DapEvent = serde_json::from_str(
            r#"{"seq":1,"type":"event","event":"process","body":{
                "name":"node","systemProcessId":42,"isLocalProcess":true}}"#,
        )
        .expect("deserialise");

        let started = started_process(&event).expect("a process");
        assert_eq!(started.pid, 42);
        assert_eq!(started.program, None, "falls back to what was launched");
    }

    #[test]
    fn a_pid_on_another_machine_is_not_ours_to_reap() {
        // A remote pid names an unrelated local process, and the reaper would
        // be entitled to kill it.
        let event: DapEvent = serde_json::from_str(
            r#"{"seq":1,"type":"event","event":"process","body":{
                "isLocalProcess":false,"systemProcessId":1234}}"#,
        )
        .expect("deserialise");

        assert_eq!(started_process(&event), None);
    }
}
