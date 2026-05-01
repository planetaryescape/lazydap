# 10 — Async-to-sync: the `--wait` pattern

The single hardest design decision in lazydap. DAP is push-based; agents and shell scripts want pull-based. The bridge is `--wait`.

This doc spells out exactly what `--wait` does, every edge case, and the rationale for each choice. It's the longest blueprint doc by intent — every ambiguity here costs months of bug reports.

## The problem in one paragraph

A DAP adapter (codelldb, debugpy) communicates by exchanging requests and responses, and by emitting events asynchronously: `stopped` when the program pauses, `output` when the program writes to stdout, `terminated` when it ends, etc. The events arrive in unpredictable order; they're not request-bound. A shell agent invokes one command and expects one JSON response. To bridge: when the agent says `lazydap continue --wait`, the daemon sends DAP `continue`, then collects events until the program reaches a *stable state* (paused / exited / terminated / timeout), and returns one combined JSON blob describing what happened.

## What "stable state" means

After a `continue` or `step` request, the program transitions through:

```
running ──┬─→ stopped (paused on bp/step/exception/pause)
          ├─→ exited (process exit code observed)
          └─→ terminated (debug session ended; with or without exited)
```

`stable state` = `Paused | Exited | Terminated`. Plus our synthetic states: `Timeout` (no stable state in time) and `AdapterDied` (adapter process exited unexpectedly).

It is **not** safe to query `stack` / `scopes` / `variables` outside a stable state. The program is running; the adapter's response would be undefined. lazydap enforces this: requests for those during `Running` get `Error::SessionNotPaused`.

## The `--wait` flow

### Without `--wait`

```
client                                  daemon
  │   continue (no wait)                  │
  ├──────────────────────────────────────►│
  │                                       │   DAP continue request
  │                                       ├──────────────────────────► adapter
  │                                       │
  │   Response::Continued                 │
  │◄──────────────────────────────────────┤
```

Returns immediately. Useful for the TUI: don't block the UI thread; let the user keep typing while the program runs. Events flow to subscribed clients via `Subscribe { channels: [Stopped, ...] }`.

### With `--wait`

```
client                                  daemon                      adapter
  │  continue { wait: Wait }              │                            │
  ├──────────────────────────────────────►│                            │
  │                                       │   DAP continue             │
  │                                       ├───────────────────────────►│
  │                                       │   continue resp (ack only) │
  │                                       │◄───────────────────────────┤
  │                                       │                            │
  │                                       │   ◄── output event ────┐   │
  │                                       │   ◄── output event ────┤   │
  │                                       │   ◄── breakpoint event ┤   │
  │                                       │   ◄── stopped event ───┘   │
  │                                       │                            │
  │  Response::Stepped(StableState{       │                            │
  │    state: Paused,                     │                            │
  │    captured_output: [...],            │                            │
  │    breakpoint_updates: [...],         │                            │
  │    frame: {...} })                    │                            │
  │◄──────────────────────────────────────┤                            │
```

The daemon buffers all events between request and pause, includes them in the response. Agents get the full picture in one call.

## Decisions, with rationale

### D1: Block until `Paused` OR `Exited` OR `Terminated`

Don't make agents poll after a program exits. The response includes a discriminator:

```rust
pub enum WaitOutcome {
    Paused,
    Exited,
    Terminated,
    Timeout,
    AdapterDied,
}
```

mcp-dap-server (`go-delve/mcp-dap-server`) does this: returns "Program terminated" on terminated, full context on stopped. We follow the same pattern. (See [research](../articles/agent-driven-debugging.md).)

### D2: Buffer intervening events into the response

During the wait:

- `output` events — buffered into `captured_output: Vec<OutputChunk>`. Categorised stdout/stderr/console.
- `breakpoint` events — buffered into `breakpoint_updates: Vec<AdapterBreakpoint>`. The adapter may resolve, move, or invalidate breakpoints during execution.
- `thread` events — buffered into `thread_updates: Vec<ThreadUpdate>`.
- `module` events — discarded by default. Set `--verbose` to include.

mcp-dap-server discards `output` during wait. We don't, because captured stdout is usually exactly what the agent wants.

### D3: Default timeout 30 seconds, configurable

Long-running programs need bigger timeouts. Tight loops need smaller. Default 30s strikes a balance: short enough that hung sessions don't lock up an agent, long enough for normal debugging.

```bash
lazydap continue --wait                           # 30s default
lazydap continue --wait --timeout 60              # 60s
lazydap continue --wait --timeout 0               # infinite (caller responsible)
LAZYDAP_TIMEOUT=120 lazydap continue --wait       # env override
```

On timeout: program keeps running. `state: Timeout` returned. User can call `lazydap pause` to interrupt. We do **not** auto-pause on timeout — that would be surprising and could mask real bugs.

mcp-dap-server has no timeout (will hang forever). LLDB Python uses explicit timeouts. We follow LLDB.

### D4: Multi-thread semantics — return on first `stopped`, coalesce 50ms

Multi-threaded programs can fire several `stopped` events in rapid succession (one per thread). The DAP spec sets `allThreadsStopped: true` only on the first.

Default: return on the first `stopped`. **Coalesce** for 50ms after — any additional `stopped` events arriving in that window are added to `additional_stopped_threads: Vec<ThreadId>`.

Override:

```bash
lazydap continue --wait --all-threads
```

This waits until `allThreadsStopped: true`, no coalescing window. Good for "I want a complete cross-thread snapshot."

### D5: Include breakpoint updates in the response

Breakpoint state can change mid-execution: adapter resolves a previously-unverified breakpoint, lazy-loads a module that contains a breakpoint location, etc. Agents that just set breakpoints want confirmation that they took.

`breakpoint_updates: Vec<AdapterBreakpoint>` includes any breakpoint events that arrived during the wait.

### D6: One in-flight execution request per session — queue, don't pipeline

ptvsd issue #1502 documents that some adapters serialise requests internally. Pipelining can deadlock if the request you're waiting on is itself blocked on a future event.

The daemon enforces: at most one of `continue`, `step`, `step-in`, `step-out`, `pause` in flight per session at a time. New ones wait in a queue.

Non-execution requests (eval, scopes, variables, stackTrace) can be parallel — they're synchronous within a stable state.

### D7: Synthetic `terminated` on adapter death

VS Code issue #102037 documents UIs getting stuck when adapters never send `terminated`. Adapters die. We detect via SIGCHLD / process status; emit a synthetic `Event::SessionEnded { reason: AdapterCrashed }` to all subscribers; turn the in-flight wait response into `state: AdapterDied { exit_code }`.

The crucial part: never trust the protocol-level `terminated` event alone. Watch the OS process.

### D8: Generation counter on thread state

nvim-dap issue #1365: a `continued` event arrives while an `update_threads` coroutine is yielded. The resumed coroutine unconditionally sets `thread.stopped = true`, overwriting the now-correct `false`.

Fix: each thread state mutation carries a generation counter. On resume from yield, check the counter — if it changed, abort the mutation. Pseudocode:

```rust
let generation = self.thread_state[tid].generation;
let result = adapter.threads_request(tid).await?;
if self.thread_state[tid].generation != generation {
    // Some other event invalidated this; drop the result.
    return Ok(());
}
self.thread_state[tid].set(...);
```

Tedious but necessary. Race conditions in async event handlers are subtle.

### D9: Coalescing window for output events

Programs that print a lot can flood `output` events. We could buffer all of them and return everything; we could deduplicate adjacent same-category chunks; we could cap the buffer.

Decision: cap at 1MB per wait window (configurable). If exceeded, truncate and add `output_truncated: true` to the response with the count of dropped chunks.

### D10: Disconnect race

DAP spec issue #126: debuggee dies before sending the disconnect response; client times out.

Fix: `disconnect` has its own short timeout (1s by default, configurable). Absence of response after timeout is treated as success. We close our side of the socket and reap the adapter process.

### D11: Read pump never blocks

Continuous adapter stdout read pump in a dedicated tokio task. The pump never blocks on a producer — uses bounded `mpsc` with backpressure on consumer side, drops oldest output if buffer full (with a warning event).

Fix for: dropped events when stdout pipe buffer fills during heavy `output` traffic.

### D12: `pause` does not wait by default

`pause` is itself the request that triggers a future `stopped` event. Adding `--wait` to `pause` is meaningful only if the user wants to block until the pause-induced stop arrives.

```bash
lazydap pause                # send pause request, return ack
lazydap pause --wait         # send pause, block until stopped event
```

mcp-dap-server's `pause` does not wait. We allow it as an option.

## The full response shape

```rust
pub struct StableState {
    pub state: WaitOutcome,                  // Paused | Exited | Terminated | Timeout | AdapterDied
    pub reason: Option<PauseReason>,
    pub thread_id: Option<ThreadId>,
    pub all_threads_stopped: bool,
    pub additional_stopped_threads: Vec<ThreadId>,
    pub hit_breakpoint_ids: Vec<BreakpointId>,
    pub exit_code: Option<i32>,
    pub frame: Option<StackFrame>,           // top frame, populated when paused
    pub captured_output: Vec<OutputChunk>,
    pub output_truncated: bool,
    pub breakpoint_updates: Vec<AdapterBreakpoint>,
    pub thread_updates: Vec<ThreadUpdate>,
    pub elapsed_ms: u64,
}
```

When `state == Paused`:

- `reason` populated (Breakpoint / Step / Exception / Pause / Entry / ...)
- `thread_id` populated
- `frame` populated with top frame (we fetch a single-frame `stackTrace(threadId, levels: 1)` for the user's convenience)
- `hit_breakpoint_ids` populated when reason is Breakpoint

When `state == Exited`:

- `exit_code` populated
- `frame` is None
- Adapter process may still be alive (terminated comes after exited)

When `state == Terminated`:

- Session is over. Subsequent commands on this session_id return `Error::SessionNotFound`.

When `state == Timeout`:

- Program is still running. `lazydap pause` to interrupt.

When `state == AdapterDied`:

- `exit_code` populated with adapter's exit status (negative for signals).
- Session is unrecoverable. Caller should `lazydap disconnect`.

## Examples

### Hit a breakpoint

```bash
$ lazydap break main.c:42 --format json
{"breakpoint_id": "bp-01ABC...", "verified": true}

$ lazydap continue --wait --format json
{
  "state": "Paused",
  "reason": { "Breakpoint": { "ids": ["bp-01ABC..."] } },
  "thread_id": 1,
  "frame": { "name": "main", "source": "main.c", "line": 42 },
  "captured_output": [{"category":"Stdout","output":"Starting...\n"}],
  "elapsed_ms": 124
}
```

### Program exits before breakpoint

```bash
$ lazydap break main.c:9999 --format json     # line beyond file
{"breakpoint_id": "bp-01XYZ", "verified": false}

$ lazydap continue --wait --format json
{
  "state": "Exited",
  "exit_code": 0,
  "captured_output": [{"category":"Stdout","output":"hello\n"}],
  "elapsed_ms": 45
}
```

### Timeout

```bash
$ lazydap continue --wait --timeout 1 --format json
{
  "state": "Timeout",
  "captured_output": [{"category":"Stdout","output":"...still running...\n"}],
  "elapsed_ms": 1003
}

$ lazydap pause --wait --format json
{
  "state": "Paused",
  "reason": "Pause",
  "thread_id": 1,
  "frame": { ... }
}
```

### Adapter dies

```bash
$ lazydap continue --wait --format json
{
  "state": "AdapterDied",
  "exit_code": 139,                          # SIGSEGV
  "captured_output": [],
  "elapsed_ms": 50
}
```

## Edge cases worth knowing about

- **`stopped` for a thread the adapter has already auto-resumed** (nvim-dap #1363). Don't trust `stopped` events as definitive without checking thread state.
- **`continued` events without prior `stopped`** (some adapters fire on launch). Treat as "thread is running" without state side-effects.
- **`output` events with `source` field** (codelldb sometimes attaches a source location). Pass through; clients can display.
- **`exited` without `terminated`** (some adapters). Treat as session-ending.
- **`terminated` without `exited`** (most adapters when user disconnects). exit_code is `None`.
- **Multiple `stopped` events for the same thread in the coalescing window.** Coalesce: keep the first reason. (Reality: DAP spec says one stopped per thread per pause.)
- **No `stopped` event ever** (program is in an infinite loop). Hits the timeout. Caller's responsibility.

## Tests required for `--wait`

These belong in `tests/` integration tests with a real `codelldb`:

- Continue → breakpoint → assert `state: Paused, reason: Breakpoint`
- Continue → exit cleanly → assert `state: Exited, exit_code: 0`
- Continue → segfault → assert `state: Paused, reason: Exception` (or `state: Terminated` depending on adapter)
- Continue → timeout → assert `state: Timeout`
- Continue when program prints heavily → assert `captured_output` non-empty, ordered correctly
- Continue when adapter killed externally → assert `state: AdapterDied`
- Multi-thread: continue → multiple stops → assert `additional_stopped_threads` populated
- Pipelining: send two continues; second should queue, both eventually return correct outcomes

## See also

- [`04-protocol.md`](04-protocol.md) — `WaitMode`, `StableState`, `Continue` request
- [`05-sessions.md`](05-sessions.md) — session state machine
- [`docs/articles/agent-driven-debugging.md`](../articles/agent-driven-debugging.md) — research that informed these decisions
