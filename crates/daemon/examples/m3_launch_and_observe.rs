//! M3 — launch a real debuggee under codelldb and stream every message the
//! adapter sends until the program terminates.
//!
//! Build the debuggee first:
//! `gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello`

use anyhow::{Context, bail};
use lazydap_dap::{
    Capabilities, ConfigurationDoneArgs, DapEvent, DapTransport, DisconnectArgs, Incoming,
    InitializeArgs, LaunchArgs,
};
use std::path::Path;
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(15);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cwd = std::env::current_dir()?;
    let program = cwd.join("examples/c-hello/build/hello");
    if !program.exists() {
        bail!(
            "build the debuggee first: \
             gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello"
        );
    }

    let mut transport = DapTransport::spawn("codelldb").await?;

    let caps: Capabilities = transport
        .request("initialize", &InitializeArgs::new("lldb"))
        .await?;
    println!(
        "[ok] initialize (supportsConfigurationDoneRequest={})",
        caps.supports_configuration_done_request
    );

    // `launch` and the `initialized` event are concurrent tracks. The adapter
    // is allowed to hold the launch response back until after
    // configurationDone, so fire the request and keep reading instead of
    // awaiting it here — awaiting deadlocks on spec-strict adapters.
    let launch_seq = transport
        .send_request("launch", &launch_args(&program, &cwd))
        .await?;
    println!("[ok] launch sent (seq={launch_seq})");

    let mut stdout = String::new();
    let mut exit_code: Option<i64> = None;

    // Terminal condition is the `terminated` event; `exited` is optional and
    // may arrive on either side of it. A read error here means EOF — the
    // adapter died without terminating cleanly.
    loop {
        match next_message(&mut transport).await? {
            Incoming::Event(event) => {
                println!("[evt] {} {}", event.event, body_of(&event));
                match event.event.as_str() {
                    "initialized" => {
                        transport
                            .send_request("configurationDone", &ConfigurationDoneArgs {})
                            .await?;
                        println!("[ok] configurationDone sent");
                    }
                    "output" => {
                        if let Some(chunk) = debuggee_stdout(&event) {
                            stdout.push_str(chunk);
                        }
                    }
                    "exited" => {
                        exit_code = event.body.as_ref().and_then(|b| b["exitCode"].as_i64());
                    }
                    "terminated" => break,
                    _ => {}
                }
            }
            Incoming::Response(resp) => {
                println!(
                    "[rsp] {} success={} (request_seq={})",
                    resp.command, resp.success, resp.request_seq
                );
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

    println!("[ok] debuggee exit code: {exit_code:?}");
    println!("[ok] debuggee stdout: {stdout:?}");
    if !stdout.contains("hello from m3") || !stdout.contains("goodbye") {
        bail!("expected both debuggee lines in captured stdout, got: {stdout:?}");
    }
    println!("[ok] captured both debuggee lines as DAP output events");
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
    }
}

async fn next_message(transport: &mut DapTransport) -> anyhow::Result<Incoming> {
    tokio::time::timeout(READ_TIMEOUT, transport.read_incoming())
        .await
        .context("timed out waiting for the adapter")?
        .context("adapter read failed")
}

/// Best-effort: the adapter may close the socket instead of answering.
async fn drain_until_disconnected(transport: &mut DapTransport, disconnect_seq: i64) {
    while let Ok(Ok(message)) = tokio::time::timeout(DRAIN_TIMEOUT, transport.read_incoming()).await
    {
        match message {
            Incoming::Response(resp) if resp.request_seq == disconnect_seq => {
                println!("[ok] disconnect acknowledged");
                return;
            }
            Incoming::Response(resp) => println!("[rsp] {} success={}", resp.command, resp.success),
            Incoming::Event(event) => println!("[evt] {} {}", event.event, body_of(&event)),
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
