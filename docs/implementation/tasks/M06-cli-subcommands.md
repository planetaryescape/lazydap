# M6 — CLI subcommands

## What

Full CLI surface from [`/docs/blueprint/06-cli.md`](../../blueprint/06-cli.md), excluding TUI-specific commands. Each shells out to the daemon via IPC. `--wait` semantics implemented per [`/docs/blueprint/10-async-to-sync.md`](../../blueprint/10-async-to-sync.md).

Subcommands to add:

- Stepping: `continue`, `step`, `step-into`, `step-out`, `pause`
- Inspection: `stack`, `scopes`, `variables`, `eval`
- Breakpoints: `break <file:line>`, `break --list`, `break --remove`, `break --toggle`
- Output: `output`
- Diagnostics: `logs`, `doctor`, `version`, `completions`

## Why

This is what makes lazydap a useful CLI. After M6, you can debug a C program from the shell without touching the TUI.

## How

For each subcommand, the pattern from M5:

1. Add a clap subcommand in `crates/daemon/src/cli/`.
2. Add the corresponding `Request` variant in `crates/protocol/src/lib.rs`.
3. Add a handler in `crates/daemon/src/handlers/` that maps the request to DAP calls.
4. Add a `Response` variant.
5. Wire the CLI handler to format the response.

### `--wait` implementation

In `crates/daemon/src/handlers/session.rs`, the `continue` handler with `wait: Wait`:

```rust
async fn handle_continue_with_wait(
    session: Arc<Session>,
    timeout_ms: Option<u32>,
) -> Result<StableState> {
    let timeout = Duration::from_millis(timeout_ms.unwrap_or(30_000) as u64);
    let started = Instant::now();
    let mut output_buf = Vec::new();
    let mut bp_updates = Vec::new();
    let mut thread_updates = Vec::new();
    let mut additional_stops = Vec::new();
    let mut event_rx = session.subscribe_events();

    // Send DAP continue
    session.transport.lock().await.request_typed::<_, ContinueResponse>(...).await?;

    let stop_or_end = loop {
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            return Ok(StableState::timeout(output_buf, ..., elapsed));
        }
        let remaining = timeout - elapsed;

        match tokio::time::timeout(remaining, event_rx.recv()).await {
            Ok(Ok(Event::Output(chunk))) => {
                if output_buf.iter().map(|c: &OutputChunk| c.output.len()).sum::<usize>() < 1_000_000 {
                    output_buf.push(chunk);
                }
            }
            Ok(Ok(Event::BreakpointUpdated(bp))) => bp_updates.push(bp),
            Ok(Ok(Event::ThreadStarted { .. } | Event::ThreadExited { .. })) => thread_updates.push(...),
            Ok(Ok(Event::Stopped { thread_id, reason, all_threads_stopped, .. })) => {
                break StopOrEnd::Stopped { thread_id, reason, all_threads_stopped };
            }
            Ok(Ok(Event::SessionEnded { reason: EndReason::ProgramExited, exit_code, .. })) => {
                break StopOrEnd::Exited(exit_code);
            }
            Ok(Ok(Event::SessionEnded { reason: EndReason::AdapterCrashed, exit_code, .. })) => {
                break StopOrEnd::AdapterDied(exit_code);
            }
            // ...
            Err(_) => return Ok(StableState::timeout(...)),
        }
    };

    // After stopping: coalesce 50ms for additional stopped threads.
    let coalesce_until = Instant::now() + Duration::from_millis(50);
    while Instant::now() < coalesce_until {
        match tokio::time::timeout(coalesce_until - Instant::now(), event_rx.recv()).await {
            Ok(Ok(Event::Stopped { thread_id, .. })) => additional_stops.push(thread_id),
            _ => break,
        }
    }

    // Fetch top frame for the response.
    let frame = if let StopOrEnd::Stopped { thread_id, .. } = &stop_or_end {
        Some(session.fetch_top_frame(*thread_id).await?)
    } else {
        None
    };

    Ok(StableState { state: ..., reason: ..., thread_id: ..., frame, captured_output: output_buf, ... })
}
```

This is the centerpiece of M6. Test it carefully with the cases from [`/docs/blueprint/10-async-to-sync.md`](../../blueprint/10-async-to-sync.md).

### Output format dispatch

`crates/daemon/src/output.rs`:

```rust
pub fn format_response<T: Serialize + Display>(resp: &T, fmt: OutputFormat) {
    match fmt {
        OutputFormat::Json => println!("{}", serde_json::to_string(resp).unwrap()),
        OutputFormat::Table => println!("{resp}"),
        OutputFormat::Csv => csv_format(resp),
        OutputFormat::Ids => ids_format(resp),
    }
}

pub fn auto_format() -> OutputFormat {
    if atty::is(atty::Stream::Stdout) {
        OutputFormat::Table
    } else {
        OutputFormat::Json
    }
}
```

Each subcommand picks `--format` arg or falls back to `auto_format()`.

### Breakpoint persistence

When `lazydap break <file:line>` runs:

1. Create a `SourceBreakpoint` with a fresh `BreakpointId`.
2. Add it to in-memory state.
3. If a session is active, send `setBreakpoints` for the file (combined with all other breakpoints in that file).
4. Persist to `.lazydap/state.toml`.
5. Return the breakpoint with the adapter's verified status.

`crates/store/src/lib.rs` does the TOML read/write. Debounced 500ms.

## Success criteria

- All listed subcommands work end-to-end.
- `lazydap continue --wait --format json` returns one JSON blob with `state`, `frame`, `captured_output`, etc.
- Persistent breakpoints survive across sessions (start session, set bp, disconnect, restart, bp still there).
- `lazydap break --list --format ids | xargs -I{} lazydap break --remove --id {}` works (composable).
- All commands return appropriate exit codes (0/1/2/3/4).

## Files

- `crates/daemon/src/cli/{continue.rs, step.rs, break.rs, stack.rs, eval.rs, ...}` (new)
- `crates/daemon/src/handlers/{session.rs, breakpoint.rs, ...}` (new)
- `crates/protocol/src/lib.rs` — add all `Request`/`Response`/`Event` variants from blueprint
- `crates/store/Cargo.toml`, `src/lib.rs` (new — TOML state read/write)

## Verify

Integration test:

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello

# Set breakpoints, launch, continue, eval, disconnect.
lazydap break examples/c-hello/main.c:6
lazydap launch ./examples/c-hello/build/hello --stop-on-entry --format json
lazydap continue --wait --format json | jq '.state'   # expect "Paused"
lazydap eval "x" --format json | jq '.value'           # expect "5"
lazydap continue --wait --format json | jq '.state'    # expect "Exited" or "Terminated"
lazydap disconnect

# Persistence
lazydap break --list --format json    # expect bp at main.c:6 still there
```

Add a `tests/integration_cli.rs` running the above against `adapter-fake` to keep CI fast.

## Depends on

- [`M05-ipc-protocol-daemon`](M05-ipc-protocol-daemon.md).

## Notes

- **`--wait` is the most-tested code in lazydap.** Cover the cases in `/docs/blueprint/10-async-to-sync.md` §"Tests required for `--wait`".
- **The daemon owns the broadcast channel for events.** Subcommand handlers subscribe per-request, drop the receiver after.
- **Don't pipeline DAP requests** to one adapter. Per D021. One execution request in flight.
- **`--dry-run` for breakpoint mutations** — must use the same selection logic as the actual mutation.
- **codelldb's `setBreakpoints` replaces all breakpoints in a source file.** When you add one, you have to send the full list for that file. Don't forget breakpoints already in the file.
