# M4 — Pause on breakpoint

## What

Take M3's example and:

1. Wait for the `initialized` event after launch.
2. Send `setBreakpoints` for `examples/c-hello/main.c:7` (the `printf("goodbye")` line).
3. Send `configurationDone`.
4. Observe the program run: `output` event "hello from m3", then a `stopped` event with reason `breakpoint`.
5. Send `continue`.
6. Observe: `output` "goodbye", then `exited`, `terminated`.
7. Send `disconnect` and exit.

By the end you've actually debugged a program: paused at a breakpoint, observed state, resumed.

## Why

This is the moment "we have a debugger" becomes real. Every milestone after this is plumbing on top of "we can pause and resume."

## How

### Step 1 — Update the C program for clear breakpoint targets

Make sure `examples/c-hello/main.c` has distinct line numbers:

```c
#include <stdio.h>      // 1
#include <unistd.h>     // 2
                        // 3
int main(int argc, char **argv) {  // 4
    int x = 5;          // 5
    printf("hello\n");  // 6 — set breakpoint here
    int y = x * 2;      // 7
    printf("y=%d\n", y);// 8
    return 0;           // 9
}                       // 10
```

Recompile.

### Step 2 — Add `setBreakpoints` types

`crates/dap/src/types.rs`:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsArgs {
    pub source: Source,
    pub breakpoints: Vec<SourceBreakpoint>,
    pub source_modified: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct Source {
    pub path: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    pub line: u32,
    pub column: Option<u32>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsResponse {
    pub breakpoints: Vec<Breakpoint>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    pub id: Option<i64>,
    pub verified: bool,
    pub message: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ConfigurationDoneArgs {}

#[derive(Debug, Serialize)]
pub struct ContinueArgs {
    pub thread_id: i64,
}
```

### Step 3 — The example

`crates/daemon/examples/m4_pause_on_breakpoint.rs`:

```rust
use lazydap_dap::{
    transport::Incoming, ConfigurationDoneArgs, ContinueArgs, DapTransport, InitializeArgs,
    LaunchArgs, SetBreakpointsArgs, SetBreakpointsResponse, Source, SourceBreakpoint,
};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let cwd = std::env::current_dir()?;
    let program = cwd.join("examples/c-hello/build/hello");
    let source_path = cwd.join("examples/c-hello/main.c");

    let mut t = DapTransport::spawn("codelldb").await?;

    // initialize
    let _ = t.request_with_value("initialize", &InitializeArgs::default()).await?;
    println!("[ok] initialized");

    // launch (don't await response yet — initialized event comes asynchronously)
    let _ = t.send_request(
        "launch",
        &LaunchArgs {
            adapter_type: "lldb".into(),
            request: "launch".into(),
            program: program.to_string_lossy().into_owned(),
            args: vec![],
            cwd: cwd.to_string_lossy().into_owned(),
            stop_on_entry: false,
            console: "internalConsole".into(),
            env: None,
        },
    )
    .await?;

    // Wait for `initialized` event before sending setBreakpoints.
    println!("[..] waiting for initialized event");
    loop {
        let inc = t.read_incoming().await?;
        if let Incoming::Event(e) = &inc {
            if e.event == "initialized" {
                println!("[ok] initialized event received");
                break;
            }
        }
        // print intermediate stuff
        println!("[..] {inc:?}");
    }

    // setBreakpoints
    let resp: SetBreakpointsResponse = t.request_typed(
        "setBreakpoints",
        &SetBreakpointsArgs {
            source: Source {
                path: source_path.to_string_lossy().into_owned(),
                name: Some("main.c".into()),
            },
            breakpoints: vec![SourceBreakpoint {
                line: 6,
                column: None,
                condition: None,
                hit_condition: None,
                log_message: None,
            }],
            source_modified: None,
        },
    )
    .await?;
    println!("[ok] setBreakpoints: {resp:?}");

    // configurationDone
    let _: serde_json::Value = t.request_with_value("configurationDone", &ConfigurationDoneArgs {}).await?;
    println!("[ok] configurationDone");

    // Now expect: output events, then a `stopped` event when we hit the breakpoint.
    let mut paused_thread_id: Option<i64> = None;
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let inc = tokio::time::timeout(Duration::from_secs(2), t.read_incoming()).await??;
        if let Incoming::Event(e) = &inc {
            println!("[evt] {} {}", e.event, serde_json::to_string(&e.body).unwrap_or_default());
            if e.event == "stopped" {
                paused_thread_id = e.body.as_ref().and_then(|b| b.get("threadId")).and_then(|t| t.as_i64());
                println!("[ok] stopped on thread {paused_thread_id:?}");
                break;
            }
        } else {
            println!("[resp] {inc:?}");
        }
    }
    let tid = paused_thread_id.ok_or_else(|| anyhow::anyhow!("never paused"))?;

    // continue
    let _: serde_json::Value = t
        .request_with_value("continue", &ContinueArgs { thread_id: tid })
        .await?;
    println!("[ok] continue");

    // Drain to terminated.
    let drain_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < drain_deadline {
        let inc = tokio::time::timeout(Duration::from_millis(500), t.read_incoming()).await;
        match inc {
            Ok(Ok(Incoming::Event(e))) => {
                println!("[evt] {}", e.event);
                if e.event == "terminated" {
                    break;
                }
            }
            Ok(Ok(Incoming::Response(r))) => {
                println!("[resp] {} success={}", r.command, r.success);
            }
            _ => break,
        }
    }

    let _ = t.send_request("disconnect", &serde_json::json!({"terminateDebuggee": true})).await?;
    t.shutdown().await?;
    Ok(())
}
```

You'll need to add a `request_typed` helper to the transport that's like `request_with_value` but typed. Or just use the existing `request` (rename `request_with_value` to that). Pick whichever shape matches what M3 left.

### Step 4 — Run

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
cargo run --example m4_pause_on_breakpoint
```

Expected (highlights):

```
[ok] initialized
[..] waiting for initialized event
[..] response launch...
[ok] initialized event received
[ok] setBreakpoints: SetBreakpointsResponse { breakpoints: [Breakpoint { verified: true, line: Some(6), ... }] }
[ok] configurationDone
[evt] thread {"reason":"started","threadId":1}
[evt] output {"category":"console","output":"Launching..."}
[evt] stopped {"reason":"breakpoint","threadId":1,"hitBreakpointIds":[1]}
[ok] stopped on thread Some(1)
[ok] continue
[evt] continued
[evt] output {"category":"stdout","output":"hello\n"}
... (note: M4 is buggy if you set the bp at line 6 BEFORE the printf — adjust to bp at line 8 if you want to see "hello" output before the pause)
[evt] thread {"reason":"exited"}
[evt] exited
[evt] terminated
```

## Success criteria

- The example reaches the `[ok] stopped on thread` line.
- The breakpoint is `verified: true`.
- After `continue`, the program completes (terminated event observed).
- Captured stdout includes the strings the program prints.

## Files

- `examples/c-hello/main.c` — line numbers documented for breakpoint targets
- `crates/dap/src/types.rs` — add `SetBreakpoints*`, `Breakpoint`, `ConfigurationDoneArgs`, `ContinueArgs`, `Source`
- `crates/daemon/examples/m4_pause_on_breakpoint.rs` (new)

## Verify

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
cargo run --example m4_pause_on_breakpoint 2>&1 | grep -E "stopped|breakpoint|terminated"
```

Should see `stopped on thread Some(1)` and `terminated` event.

## Depends on

- [`M03-launch-and-observe`](M03-launch-and-observe.md) — event streaming.

## Notes

- **The order matters.** `initialize` → `launch` (don't wait for response) → wait for `initialized` event → `setBreakpoints` → `configurationDone` → events flow → `stopped` → `continue` → events → `terminated`. Skipping `configurationDone` will hang.
- **Breakpoint resolution can move the line.** If line 6 isn't executable, the adapter snaps to the nearest. The response's `breakpoint.line` may differ from your request.
- **`stopped` reasons:** `breakpoint`, `step`, `exception`, `pause`, `entry`. Same enum lazydap will use later in `PauseReason`.
- **Think about the daemon already.** This logic — wait-for-initialized, set-breakpoints, config-done, react-to-stopped — is what the daemon will own. The example is a single-purpose script; the daemon will be a re-entrant version of the same.
- **After M4, Phase A is done.** You've seen the protocol, set a breakpoint, hit it, observed events. Move to Phase B.
