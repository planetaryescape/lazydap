# M3 — Launch and observe

## What

Send `initialize` + `launch` for a hello-world C binary compiled with `-g`. Stream all events from the adapter for 5 seconds. Then send `disconnect` and exit.

By the end you've seen a real debug session run start to finish.

## Why

This is where the protocol comes alive. You'll see the actual sequence of events DAP emits during a normal program run: `initialized`, `process`, `thread`, `output`, `terminated`, etc. Knowing this sequence is what every later milestone depends on.

## How

### Step 1 — Create a hello-world C program to debug

`examples/c-hello/main.c`:

```c
#include <stdio.h>
#include <unistd.h>

int main(int argc, char **argv) {
    printf("hello from m3\n");
    fflush(stdout);
    sleep(1);
    printf("goodbye\n");
    return 0;
}
```

Build with debug symbols:

```bash
mkdir -p examples/c-hello/build
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
```

Verify it runs: `./examples/c-hello/build/hello` should print "hello from m3", pause 1s, print "goodbye".

### Step 2 — Extend the transport with event streaming

The M2 `request` method discards events. We need to keep them.

Add to `crates/dap/src/transport.rs`:

```rust
use tokio::sync::mpsc;

impl DapTransport {
    /// Same as before, but route events to a channel.
    pub async fn spawn_with_events(
        adapter_path: &str,
        event_tx: mpsc::UnboundedSender<DapEvent>,
    ) -> Result<Self> {
        // ... existing spawn logic ...
        // After connecting, spawn an event-pump task:
        // tokio::spawn(async move { ... read messages from stream, route events to event_tx ... });
        // For M3, simplest: run the pump inline alongside requests.
    }
}
```

Actually, the simplest M3-shape is: keep `DapTransport::spawn` as in M2, but add a `read_event_or_response` method that returns either, and let the M3 example loop:

```rust
pub enum Incoming {
    Response(DapResponse<serde_json::Value>),
    Event(DapEvent),
}

impl DapTransport {
    pub async fn read_incoming(&mut self) -> Result<Incoming> {
        let body = self.read_message_body().await?;
        let value: serde_json::Value = serde_json::from_slice(&body)?;
        match value.get("type").and_then(|v| v.as_str()) {
            Some("response") => {
                let resp = serde_json::from_slice(&body)?;
                Ok(Incoming::Response(resp))
            }
            Some("event") => {
                let evt = serde_json::from_slice(&body)?;
                Ok(Incoming::Event(evt))
            }
            other => Err(TransportError::Dap(format!("unknown message type: {other:?}"))),
        }
    }

    /// Send a request without waiting for the response. Returns the seq for correlation.
    pub async fn send_request<T: Serialize>(&mut self, command: &str, args: &T) -> Result<i64> {
        // ... like request() but don't loop reading ...
    }
}
```

### Step 3 — Define `LaunchArgs` (codelldb shape)

In `crates/dap/src/types.rs`:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchArgs {
    #[serde(rename = "type")]
    pub adapter_type: String,           // "lldb"
    pub request: String,                // "launch"
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: String,
    pub stop_on_entry: bool,
    pub console: String,                // "internalConsole"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::BTreeMap<String, String>>,
}
```

### Step 4 — The example

`crates/daemon/examples/m3_launch_and_observe.rs`:

```rust
use lazydap_dap::{DapTransport, InitializeArgs, LaunchArgs, transport::Incoming};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cwd = std::env::current_dir()?;
    let program = cwd.join("examples/c-hello/build/hello");
    if !program.exists() {
        anyhow::bail!("compile examples/c-hello/main.c first: `gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello`");
    }

    let mut t = DapTransport::spawn("codelldb").await?;

    // initialize
    let _caps = t.request_with_value("initialize", &InitializeArgs::default()).await?;
    println!("--> initialized");

    // launch — codelldb DAP launch arguments live in `arguments`.
    // For codelldb the shape is roughly the launch.json entry minus the outer `name`.
    let launch_args = LaunchArgs {
        adapter_type: "lldb".into(),
        request: "launch".into(),
        program: program.to_string_lossy().into_owned(),
        args: vec![],
        cwd: cwd.to_string_lossy().into_owned(),
        stop_on_entry: false,
        console: "internalConsole".into(),
        env: None,
    };
    let _ = t.send_request("launch", &launch_args).await?;
    println!("--> launch sent");

    // Now read everything for 5s, printing as we go.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let result = tokio::time::timeout(remaining, t.read_incoming()).await;
        match result {
            Ok(Ok(Incoming::Response(r))) => {
                println!("[response] command={}, success={}", r.command, r.success);
            }
            Ok(Ok(Incoming::Event(e))) => {
                println!(
                    "[event] {} body={}",
                    e.event,
                    serde_json::to_string(&e.body).unwrap_or_default()
                );
            }
            Ok(Err(e)) => {
                eprintln!("error: {e}");
                break;
            }
            Err(_) => break, // timeout
        }
    }

    println!("--> sending disconnect");
    let _ = t.send_request(
        "disconnect",
        &serde_json::json!({ "terminateDebuggee": true }),
    )
    .await?;

    // Drain a bit more to see disconnect response/terminated.
    let final_deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < final_deadline {
        let result = tokio::time::timeout(Duration::from_millis(200), t.read_incoming()).await;
        match result {
            Ok(Ok(msg)) => println!("post-disconnect: {msg:?}"),
            _ => break,
        }
    }

    t.shutdown().await?;
    Ok(())
}
```

(`request_with_value` is a convenience returning `serde_json::Value` so we don't have to define a typed Capabilities here.)

### Step 5 — Run

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
cargo run --example m3_launch_and_observe
```

Expected output (something like):

```
--> initialized
--> launch sent
[response] command=launch, success=true
[event] initialized body=null
[event] process body={"name":"hello","systemProcessId":...,"isLocalProcess":true,"startMethod":"launch"}
[event] thread body={"reason":"started","threadId":1}
[event] output body={"category":"console","output":"Listening for incoming connection..."}
[event] output body={"category":"stdout","output":"hello from m3\n"}
... (sleeps 1s)
[event] output body={"category":"stdout","output":"goodbye\n"}
[event] thread body={"reason":"exited","threadId":1}
[event] exited body={"exitCode":0}
[event] terminated body=null
--> sending disconnect
post-disconnect: ...
```

The exact event order and content varies, but you should see: `launch` response, `initialized` event (configure-done window opens), then a process/thread/output sequence, then exit/terminated.

## Success criteria

- M3 example builds and runs.
- You see at least: `launch` response, `initialized` event, `process` event, `output` events with stdout content, `exited` event, `terminated` event.
- Captured stdout includes the strings "hello from m3" and "goodbye".
- Process count: no leaked codelldb after exit.

## Files

- `examples/c-hello/main.c` (new)
- `examples/c-hello/build/hello` (built artefact, in `.gitignore`)
- `crates/dap/src/transport.rs` — extend with `read_incoming`, `send_request`, `Incoming`
- `crates/dap/src/types.rs` — add `LaunchArgs`
- `crates/daemon/examples/m3_launch_and_observe.rs` (new)

## Verify

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
cargo run --example m3_launch_and_observe 2>&1 | grep -E "hello from m3|goodbye"
# Should print both lines.

pgrep codelldb || echo "(none)"
```

## Depends on

- [`M02-initialize-handshake`](M02-initialize-handshake.md) — transport with typed request/response.

## Notes

- **`stop_on_entry: false`** here. M4 will set it true when we add breakpoints.
- **No `configurationDone` yet.** Some adapters require it after `initialized` event before they finish the launch. codelldb tolerates not sending it for simple programs. M4 will add it.
- **`request: "launch"`** is the DAP request shape for codelldb. It's what `launch.json` puts in the `request` field. Keep this in mind for `launch.json` import in M15.
- **Don't rely on event order.** Different adapters emit events in different orders. Code defensively — the daemon should react to events when they arrive, not assume a sequence.
