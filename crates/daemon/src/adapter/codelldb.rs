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

use super::{AdapterError, AdapterHandle, Pending, Result, discover};
use lazydap_core::{AdapterKind, OutputCategory, OutputChunk, PauseReason, SessionState};
use lazydap_dap::{
    Capabilities, ConfigurationDoneArgs, DapEvent, DapReader, DapTransport, Incoming,
    InitializeArgs, LaunchArgs,
};
use lazydap_protocol::{AdapterCapabilities, LaunchRequest};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

/// Longest the whole launch may take, start to paused-or-running.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest to wait for any single message during the launch.
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(15);

/// A session that is up, plus the read half the pump still has to be given.
pub struct Launched {
    pub handle: AdapterHandle,
    pub pump: PumpStart,
    pub capabilities: AdapterCapabilities,
    pub state: SessionState,
    pub reason: Option<PauseReason>,
    pub thread_id: Option<i64>,
    /// Set when the debuggee finished during its own launch.
    pub exit_code: Option<i32>,
    /// Debuggee output produced before the pump took over. Seeded into the
    /// session buffer so a `stop_on_entry` launch does not silently lose the
    /// first lines.
    pub output: Vec<OutputChunk>,
}

/// The read half plus the map the pump delivers responses into. Opaque on
/// purpose: DAP types do not leave this module.
pub struct PumpStart {
    pub(super) reader: DapReader,
    pub(super) pending: Pending,
}

/// Start `request.program` under codelldb and wait for it to settle.
pub async fn launch(request: &LaunchRequest) -> Result<Launched> {
    let adapter_path = discover(AdapterKind::Codelldb)?;
    let mut transport = DapTransport::spawn(&adapter_path.to_string_lossy()).await?;

    match handshake(&mut transport, request).await {
        Ok(outcome) => {
            let (reader, writer) = transport.split();
            let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
            Ok(Launched {
                handle: AdapterHandle::new(writer, Arc::clone(&pending)),
                pump: PumpStart { reader, pending },
                capabilities: outcome.capabilities,
                state: outcome.state,
                reason: outcome.reason,
                thread_id: outcome.thread_id,
                exit_code: outcome.exit_code,
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
    thread_id: Option<i64>,
    exit_code: Option<i32>,
    output: Vec<OutputChunk>,
}

async fn handshake(transport: &mut DapTransport, request: &LaunchRequest) -> Result<Outcome> {
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
    let mut launch_answered = false;
    let mut outcome = Outcome {
        capabilities: translate_capabilities(&capabilities),
        state: SessionState::Running,
        reason: None,
        thread_id: None,
        exit_code: None,
        output: Vec::new(),
    };

    loop {
        // Waiting for a `stopped` we asked for is not optional: a caller that
        // said `--stop-on-entry` and got "running" back would be lied to.
        let settled = launch_answered
            && configuration_done_seq.is_some()
            && (!request.stop_on_entry || outcome.reason.is_some());
        if settled || outcome.state == SessionState::Terminated {
            return Ok(outcome);
        }

        match with_timeout("adapter message", deadline, transport.read_incoming()).await?? {
            Incoming::Event(event) => match event.event.as_str() {
                "initialized" => {
                    configuration_done_seq = Some(
                        transport
                            .send_request("configurationDone", &ConfigurationDoneArgs {})
                            .await?,
                    );
                }
                "output" => outcome.output.extend(output_chunk(&event)),
                "stopped" => {
                    let body = event.body.unwrap_or_default();
                    outcome.state = SessionState::Paused;
                    outcome.reason = Some(PauseReason::from(
                        body["reason"].as_str().unwrap_or("unknown"),
                    ));
                    outcome.thread_id = body["threadId"].as_i64();
                }
                // A program short enough to finish during its own launch is
                // unusual but not an error; report it as it happened. The exit
                // code arrives on its own event, and losing it here would mean
                // a debuggee that finished this fast could never report how it
                // went.
                "exited" => {
                    outcome.exit_code = event
                        .body
                        .as_ref()
                        .and_then(|body| body["exitCode"].as_i64())
                        .map(|code| code as i32);
                }
                "terminated" => outcome.state = SessionState::Terminated,
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
            }
        }
    }
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
    fn an_output_event_without_a_body_is_not_a_chunk() {
        let event: DapEvent = serde_json::from_str(r#"{"seq":1,"type":"event","event":"output"}"#)
            .expect("deserialise");
        assert!(output_chunk(&event).is_none());
    }
}
