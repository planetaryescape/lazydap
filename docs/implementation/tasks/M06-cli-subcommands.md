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

---

## Completed 2026-07-30

The full subcommand surface, `--wait`, and per-project breakpoints. Four gates green plus the boundary script; 250 tests, 13 of them driving real codelldb.

### What shipped

- **`crates/store`** (new) — `.lazydap/state.toml` per D006: debounced 500ms, written atomically (write-then-rename), paths stored relative to the project root so the file survives a clone. State a newer lazydap wrote is carried through a rewrite rather than dropped, and a breakpoint added by hand-editing the file is adopted rather than reverted.
- **`crates/daemon/src/wait.rs`** — the centrepiece. Every case from [`10-async-to-sync.md`](../../blueprint/10-async-to-sync.md) §"Tests required for `--wait`".
- **Stepping**: `continue`, `step` (alias `next`), `step-in` (alias `step-into`), `step-out`, `pause` — all with `--wait`/`--timeout`.
- **Inspection**: `stack`, `scopes`, `variables`, `eval`, `threads`, `output`.
- **Breakpoints**: `break <file:line>`, `--list`, `--remove`, `--toggle`, with `--condition`, `--hit-condition`, `--log`, `--disabled`, `--dry-run`.
- **Diagnostics**: `doctor`, `version`, `logs`, `completions`.
- **Formats**: `table`, `json`, `jsonl`, `csv`, `ids`.

### Decisions taken (recorded in [`15-decision-log.md`](../../blueprint/15-decision-log.md))

- **D031** — `BreakpointId` is a small integer, not a UUID. It is typed, piped and read out of a TOML file; a UUID is worse at all three.
- **D032** — protocol version 2. A v1 daemon cannot decode M6's requests, so the bump turns "BadRequest for a command that plainly exists" into a `VersionMismatch` the client already knows how to resolve. `ErrorCode` gains `SessionNotPaused`.
- **D033** — codelldb's `SIGSTOP` entry stop is reported as `reason: "entry"`, with the adapter's own word kept in `raw_reason` (quirk 6, option 3).
- **D034** — `eval` defaults to the `watch` context. `repl` goes to LLDB's *command* interpreter, where `x` means `memory read` (quirk 7, found live).
- **D036** — what `--dry-run` means per command, including why `launch` has none.

### The three bugs live verification found

Each has a unit test now; none would have been caught by the adapter-free tests alone.

1. **A second `--wait` re-reported the first one's output.** `take_undelivered` marked the *buffer drain* as delivered, but a wait goes on consuming events live for as long as it runs, and those stayed undelivered. The second `continue --wait` of a session carried the first one's `hello`, which reads exactly like the program printing twice. Fixed with `Session::mark_delivered`, called with the watermark the wait actually reached.
2. **`eval` was unusable** — see D034.
3. **`--timeout 0` panicked the client.** "Wait forever" was expressed as `Duration::from_secs(u64::MAX / 2)`; `Instant + Duration` panics on overflow, so the one spelling promising to block longest aborted immediately. The timeout is an `Option` end to end now. Found by the post-implementation simplify pass, not by the tests — which is its own lesson about sentinel values.

### Deviations from the plan above

- **Handlers and commands are directories, not single files.** `handlers/{session,inspect,breakpoints}.rs` and `commands/{session,inspect,breakpoints,diagnostics}.rs`. The task file's sketch of one file per subcommand would have made twenty files of thirty lines.
- **`BreakpointSelector` and `NewBreakpoint` live in `lazydap-core`, not the store.** The protocol needs to name them and cannot depend on the store; they are domain vocabulary with no I/O, so core is where they belong.
- **`--wait` holds the D021 execution permit for the DAP request and its acknowledgement only**, not for the whole wait. Holding it longer would queue `pause` — the one command that interrupts a runaway program — behind the runaway program.
- **A finished session no longer holds the launch slot.** M5's follow-up asked whether to auto-reap one; it is reaped at the next `launch`. The slot exists to stop two adapters running at once, and a session with no adapter is not what it is protecting against.
- **`--yes` is not implemented.** See D036.
- **`clap_complete` was added** for `lazydap completions`, which the task file lists. First-party to clap and generated from the same command tree. Flagged for review as a dependency-budget addition.
- **`break --list --format table` shows the effective line** — where the adapter actually put the breakpoint, when it moved it — rather than the line requested. The requested line is still what is persisted.

### Follow-ups discovered

- **codelldb sends two `breakpoint` events per `setBreakpoints`**, about 20ms apart, with identical content. `--wait` coalesces `breakpoint_updates` by identity so a caller does not read one change as two. Worth a quirk entry if it turns out to vary by version.
- **`lazydap logs --level` matches on the line's text**, because the daemon log is `tracing`'s human format. A structured log file would make it exact; that is an M15 concern.
- **`output` reads the buffer without consuming it, while `--wait` consumes.** Documented in the skill, but the two commands having different consumption semantics is the sort of thing that will need re-explaining. Revisit if `Subscribe` (M11) makes a cleaner model available.
- **Inspection requires `paused`, and `threads` deliberately does not** — which threads exist is a fair question about a running program.
- **The multi-threaded `additional_stopped_threads` assertion is weaker than the blueprint's.** How many threads an adapter reports as separately stopped is a race by construction — that is what the coalescing window is *for* — so the integration test asserts the invariant (the named thread is not also in the extras, and the program really is multi-threaded) and the coalescing itself is unit-tested deterministically.
