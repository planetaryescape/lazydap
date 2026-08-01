//! M4 — set a breakpoint, hit it, resume, and watch the debuggee finish.
//!
//! Build the debuggee first:
//! `gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello`
//!
//! The breakpoint sits on the `goodbye` printf (examples/c-hello/main.c:19),
//! so the "hello from m3" output event arrives *before* the pause.

use anyhow::{Context, bail};
use lazydap_dap::{
    Capabilities, ConfigurationDoneArgs, ContinueArgs, DapEvent, DapTransport, DisconnectArgs,
    Incoming, InitializeArgs, LaunchArgs, SetBreakpointsArgs, SetBreakpointsResponse, Source,
    SourceBreakpoint,
};
use std::path::Path;
use std::time::Duration;
use tokio::time::Instant;

const BREAKPOINT_LINE: u32 = 19;
/// Longest wait for any single message.
const READ_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest a whole read loop may run. A debuggee that emits events forever
/// would otherwise keep resetting the per-message timer and never finish.
const PHASE_TIMEOUT: Duration = Duration::from_secs(60);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cwd = std::env::current_dir()?;
    let program = cwd.join("examples/c-hello/build/hello");
    let source = cwd.join("examples/c-hello/main.c");
    if !program.exists() {
        bail!(
            "build the debuggee first: \
             gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello"
        );
    }

    // See `m2_initialize.rs`: the startup recipe belongs to the adapter
    // module, and copying it here would be a copy that can drift.
    let lazydap_daemon::adapter::Spawn::Tcp(spawn) =
        lazydap_daemon::adapter::for_kind(lazydap_core::AdapterKind::Codelldb)
            .spawn(std::path::Path::new("codelldb"))
    else {
        unreachable!("codelldb speaks DAP over TCP")
    };
    let mut transport = DapTransport::spawn_tcp(&spawn).await?;

    let caps: Capabilities = transport
        .request("initialize", &InitializeArgs::new("lldb"))
        .await?;
    println!(
        "[ok] initialize (supportsConfigurationDoneRequest={})",
        caps.supports_configuration_done_request
    );

    // Fire launch without awaiting its response: the adapter may hold that
    // response until after configurationDone, and configurationDone is what we
    // send in reaction to the `initialized` event below.
    transport
        .send_request("launch", &launch_args(&program, &cwd))
        .await?;
    println!("[ok] launch sent");

    let mut set_breakpoints_seq: Option<i64> = None;
    let mut breakpoint_id: Option<i64> = None;
    let mut stdout_before_pause = String::new();
    let stopped;

    // Phase 1: run the configuration handshake and read in receipt order until
    // the debuggee pauses.
    let configure_deadline = Instant::now() + PHASE_TIMEOUT;
    loop {
        match next_message(&mut transport, configure_deadline).await? {
            // codelldb never sends one. lazydap's daemon answers them
            // properly; an example that only drives codelldb just says so.
            Incoming::ReverseRequest(request) => {
                println!("[req] {} (unanswered by this example)", request.command)
            }
            Incoming::Event(event) => {
                println!("[evt] {} {}", event.event, body_of(&event));
                match event.event.as_str() {
                    "initialized" => {
                        let args = set_breakpoints_args(&source);
                        set_breakpoints_seq =
                            Some(transport.send_request("setBreakpoints", &args).await?);
                        println!("[ok] setBreakpoints sent for line {BREAKPOINT_LINE}");
                    }
                    "breakpoint" => {
                        // codelldb verifies lazily after the target loads, and
                        // can move the line. Reconcile by id.
                        if let Some(body) = event.body.as_ref()
                            && body["breakpoint"]["id"].as_i64() == breakpoint_id
                        {
                            println!(
                                "[ok] breakpoint update: verified={} line={}",
                                body["breakpoint"]["verified"], body["breakpoint"]["line"]
                            );
                        }
                    }
                    "output" => {
                        if let Some(chunk) = debuggee_stdout(&event) {
                            stdout_before_pause.push_str(chunk);
                        }
                    }
                    "stopped" => {
                        stopped = event.body.unwrap_or_default();
                        break;
                    }
                    "terminated" => bail!("debuggee terminated before hitting the breakpoint"),
                    _ => {}
                }
            }
            Incoming::Response(resp) => {
                println!(
                    "[rsp] {} success={} (request_seq={})",
                    resp.command, resp.success, resp.request_seq
                );
                if !resp.success {
                    bail!(
                        "{} failed: {}",
                        resp.command,
                        resp.message.unwrap_or_default()
                    );
                }
                if Some(resp.request_seq) == set_breakpoints_seq {
                    let body: SetBreakpointsResponse =
                        serde_json::from_value(resp.body.unwrap_or_default())
                            .context("setBreakpoints response body")?;
                    let bp = body
                        .breakpoints
                        .first()
                        .context("setBreakpoints returned no breakpoints")?;
                    breakpoint_id = bp.id;
                    println!(
                        "[ok] setBreakpoints: id={:?} verified={} line={:?}",
                        bp.id, bp.verified, bp.line
                    );
                    transport
                        .send_request("configurationDone", &ConfigurationDoneArgs {})
                        .await?;
                    println!("[ok] configurationDone sent");
                }
            }
        }
    }

    let reason = stopped["reason"].as_str().unwrap_or_default();
    let thread_id = stopped["threadId"]
        .as_i64()
        .context("stopped event carried no threadId")?;
    println!("[ok] stopped on thread {thread_id} (reason={reason})");
    if reason != "breakpoint" {
        bail!("expected to stop on a breakpoint, got reason: {reason}");
    }
    if !stdout_before_pause.contains("hello from m3") {
        bail!("expected the hello line before the pause, got: {stdout_before_pause:?}");
    }
    println!("[ok] debuggee stdout before the pause: {stdout_before_pause:?}");

    // Phase 2: resume and read until the debuggee terminates.
    transport
        .send_request("continue", &ContinueArgs { thread_id })
        .await?;
    println!("[ok] continue sent");

    let mut stdout_after_resume = String::new();
    let resume_deadline = Instant::now() + PHASE_TIMEOUT;
    loop {
        match next_message(&mut transport, resume_deadline).await? {
            // codelldb never sends one. lazydap's daemon answers them
            // properly; an example that only drives codelldb just says so.
            Incoming::ReverseRequest(request) => {
                println!("[req] {} (unanswered by this example)", request.command)
            }
            Incoming::Event(event) => {
                println!("[evt] {} {}", event.event, body_of(&event));
                match event.event.as_str() {
                    "output" => {
                        if let Some(chunk) = debuggee_stdout(&event) {
                            stdout_after_resume.push_str(chunk);
                        }
                    }
                    "terminated" => break,
                    _ => {}
                }
            }
            Incoming::Response(resp) => {
                println!("[rsp] {} success={}", resp.command, resp.success);
                // A rejected `continue` would otherwise surface as a timeout
                // rather than as the adapter's own complaint.
                if !resp.success {
                    bail!(
                        "{} failed: {}",
                        resp.command,
                        resp.message.unwrap_or_default()
                    );
                }
            }
        }
    }

    let disconnect_seq = transport
        .send_request(
            "disconnect",
            &DisconnectArgs {
                terminate_debuggee: true,
            },
        )
        .await?;
    drain_until_disconnected(&mut transport, disconnect_seq).await;
    transport.shutdown().await?;

    println!("[ok] debuggee stdout after resume: {stdout_after_resume:?}");
    if !stdout_after_resume.contains("goodbye") {
        bail!("expected the goodbye line after resuming, got: {stdout_after_resume:?}");
    }
    println!("[ok] paused at the breakpoint, resumed, ran to termination");
    Ok(())
}

fn launch_args(program: &Path, cwd: &Path) -> LaunchArgs {
    LaunchArgs {
        adapter_type: "lldb".into(),
        request: "launch".into(),
        program: program.to_string_lossy().into_owned(),
        args: vec![],
        cwd: cwd.to_string_lossy().into_owned(),
        stop_on_entry: false,
        env: None,
        // codelldb defaults to the integrated terminal, which needs a
        // runInTerminal reverse request we deliberately do not advertise.
        // "console" keeps the debuggee attached so its stdout arrives as DAP
        // output events.
        terminal: Some("console".into()),
        source_languages: None,
    }
}

fn set_breakpoints_args(source: &Path) -> SetBreakpointsArgs {
    SetBreakpointsArgs {
        source: Source {
            path: source.to_string_lossy().into_owned(),
            name: Some("main.c".into()),
        },
        breakpoints: vec![SourceBreakpoint {
            line: BREAKPOINT_LINE,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
        }],
        source_modified: None,
    }
}

/// Read the next message, bounded both per-message and by the enclosing
/// phase's deadline. Any timeout here is terminal for the transport:
/// `read_incoming` is not cancellation-safe, so the caller must bail rather
/// than read again.
async fn next_message(transport: &mut DapTransport, deadline: Instant) -> anyhow::Result<Incoming> {
    let read_deadline = deadline.min(Instant::now() + READ_TIMEOUT);
    match tokio::time::timeout_at(read_deadline, transport.read_incoming()).await {
        Ok(message) => message.context("adapter read failed"),
        Err(_) if read_deadline == deadline => bail!("phase deadline exceeded"),
        Err(_) => bail!("timed out waiting for the adapter"),
    }
}

/// Best-effort: the adapter may close the socket instead of answering.
async fn drain_until_disconnected(transport: &mut DapTransport, disconnect_seq: i64) {
    let deadline = Instant::now() + PHASE_TIMEOUT;
    while let Ok(Ok(message)) = tokio::time::timeout_at(
        deadline.min(Instant::now() + DRAIN_TIMEOUT),
        transport.read_incoming(),
    )
    .await
    {
        match message {
            Incoming::Response(resp) if resp.request_seq == disconnect_seq => {
                if resp.success {
                    println!("[ok] disconnect acknowledged");
                } else {
                    println!(
                        "[warn] disconnect rejected: {}",
                        resp.message.unwrap_or_default()
                    );
                }
                return;
            }
            Incoming::Response(resp) => println!("[rsp] {} success={}", resp.command, resp.success),
            Incoming::Event(event) => println!("[evt] {} {}", event.event, body_of(&event)),
            // codelldb never sends one; lazydap answers them properly in
            // `crates/daemon/src/adapter/`. Here, saying so is enough.
            Incoming::ReverseRequest(request) => {
                println!("[req] {} (unanswered by this example)", request.command)
            }
        }
    }
}

fn body_of(event: &DapEvent) -> String {
    event
        .body
        .as_ref()
        .map(|b| b.to_string())
        .unwrap_or_default()
}

/// The debuggee's own stdout, as opposed to the adapter's `console` chatter.
fn debuggee_stdout(event: &DapEvent) -> Option<&str> {
    let body = event.body.as_ref()?;
    if body["category"].as_str() != Some("stdout") {
        return None;
    }
    body["output"].as_str()
}
