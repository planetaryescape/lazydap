# 01 — Architecture

This is the long version of [`/ARCHITECTURE.md`](../../ARCHITECTURE.md). Read that first for the summary; come here for the depth.

## Three layers, three protocols

```
                     ┌────────────────────────────────────┐
                     │             Clients                │
                     │   TUI · CLI · skill · web · MCP    │
                     └────────────────┬───────────────────┘
                                      │
            ━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━
                                      │  lazydap protocol
                                      │  (length-delimited JSON over Unix socket)
            ━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━
                                      │
                     ┌────────────────▼───────────────────┐
                     │          lazydap-daemon            │
                     │   session manager · adapter pool   │
                     │   event broadcast · state cache    │
                     └────────────────┬───────────────────┘
                                      │
            ━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━
                                      │  DAP protocol
                                      │  (Content-Length-framed JSON over stdio)
            ━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━
                                      │
                     ┌────────────────▼───────────────────┐
                     │           DAP adapter              │
                     │   codelldb · debugpy · delve · ... │
                     └────────────────┬───────────────────┘
                                      │
            ━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━
                                      │  ptrace / syscall / native debug API
            ━━━━━━━━━━━━━━━━━━━━━━━━━━┿━━━━━━━━━━━━━━━━━━━━━━━━━━
                                      │
                     ┌────────────────▼───────────────────┐
                     │          your program              │
                     └────────────────────────────────────┘
```

Three protocols. Three failure surfaces. Three places to debug-the-debugger:

- **lazydap protocol failures** → look at `tracing` logs in the daemon, IPC messages.
- **DAP protocol failures** → look at the adapter's stderr (codelldb is chatty), check the DAP transcript with `LAZYDAP_LOG_DAP=1`.
- **Native debug API failures** → look at the adapter's logs; rare, usually means symbols missing or permissions wrong.

## Crate dependency graph

```
                            ┌───────────┐
                            │   core    │  zero I/O. types, errors, traits.
                            └─────┬─────┘
                  ┌───────────────┼───────────────┐
                  │               │               │
            ┌─────▼─────┐   ┌─────▼─────┐   ┌─────▼─────┐
            │ protocol  │   │   store   │   │  config   │
            └─────┬─────┘   └─────┬─────┘   └─────┬─────┘
                  │               │               │
                  └───────┬───────┴───────────────┘
                          │
                  ┌───────▼───────┐
                  │     dap       │  raw DAP types + transport
                  └───────┬───────┘
                          │
                ┌─────────┴─────────┐
                │                   │
        ┌───────▼───────┐   ┌───────▼───────┐
        │ adapter-      │   │ adapter-fake  │
        │ codelldb      │   └───────┬───────┘
        └───────┬───────┘           │
                │                   │
                └─────────┬─────────┘
                          │
                  ┌───────▼───────┐
                  │   daemon      │  binary `lazydap`
                  └───────────────┘
                          ▲
                          │  IPC
                          │
                  ┌───────┴───────┐
                  │     tui       │  client library
                  └───────────────┘
```

**Hard rules** (CI enforces):

| Crate | May depend on | May NOT depend on |
|---|---|---|
| `core` | (none) | anything internal |
| `protocol` | `core` | `dap`, `store`, daemon, adapters |
| `dap` | (none internal) | core, protocol — keep raw |
| `store` | `core` | `dap`, daemon, adapters |
| `config` | `core` | `dap`, daemon, adapters |
| `adapter-*` | `core`, `dap` | other adapters, daemon, tui, store |
| `tui` | `core`, `protocol`, `config` | daemon, store, dap, any adapter |
| `daemon` | everything except `tui` | `tui` |

If a feature wants to break these rules, the architecture is wrong, not the rule.

## The daemon

One `lazydap-daemon` process per project (instance keyed on project root, see [`08-state-and-config.md`](08-state-and-config.md)). Owns:

- **Active session** (one in v0.1, multiple post-v0.1) — the live debug state.
- **Adapter process** — codelldb / debugpy / etc., spawned per session.
- **DAP request queue** — outgoing DAP requests, one in-flight per adapter (see `10-async-to-sync.md` D021).
- **DAP event pump** — continuously reads adapter stdout, dispatches events.
- **IPC server** — Unix socket, accepts multiple concurrent client connections.
- **Event broadcast** — `tokio::sync::broadcast` channel; clients subscribe by message kind.
- **State cache** — current frame, scopes, breakpoint state. Authoritative for clients.
- **Persistent state loader** — reads `.lazydap/state.toml` on startup, writes on changes.

The daemon is the only thing that:

- Speaks DAP.
- Owns adapter lifecycle.
- Holds session state.
- Talks to the filesystem.

Clients (TUI, CLI, skill, web, MCP) speak only the lazydap protocol. They cannot touch the DAP adapter directly.

## Daemon lifecycle

1. **Auto-spawn**: first CLI subcommand needing the daemon checks for `{runtime_dir}/lazydap-{instance}.sock`. If missing or unresponsive, fork a daemon child, drop a PID file, bind the socket, ready.
2. **Probe**: clients probe the socket with a `Ping` request before sending real work. If `Ping` fails, retry once after 100ms; if still failing, exit with code 3 ("daemon not running").
3. **Long-running**: daemon stays alive across sessions. A `lazydap launch` after `lazydap disconnect` reuses the daemon, just spawns a new adapter.
4. **Idle shutdown**: optional. Daemon can self-shutdown after N minutes idle (configurable, default disabled).
5. **Crash recovery**: clients detect socket disappearance; auto-respawn daemon on next command. State in `.lazydap/state.toml` survives daemon crashes; live session state does not.
6. **Explicit control**: `lazydap daemon --foreground` runs in fg for debugging; `lazydap restart` kills + restarts.

## IPC contract — four buckets

Every request must classify into one. Adding a fifth bucket requires a `15-decision-log.md` entry.

### Bucket 1 — Session

Live debug session control. The bulk of the protocol. Examples:

```
Launch { adapter, program, args, cwd, env, stop_on_entry }
Attach { adapter, pid, cwd }
Continue { thread_id, wait, timeout, all_threads }
Step { thread_id, granularity }     // step over
StepIn { thread_id, granularity }
StepOut { thread_id }
Pause { thread_id }
SetBreakpoints { source, breakpoints, source_hash }
Eval { expression, frame_id, context }
StackTrace { thread_id, start_frame, levels }
Scopes { frame_id }
Variables { variables_reference, filter, start, count }
Disconnect { terminate }
```

### Bucket 2 — Project

Per-project persistent state. Reads/writes `.lazydap/state.toml`.

```
Breakpoint::List, Breakpoint::Add, Breakpoint::Remove, Breakpoint::Toggle
Watch::List, Watch::Add, Watch::Remove
LaunchConfig::List, LaunchConfig::Run { name }, LaunchConfig::Add
```

### Bucket 3 — Diagnostics

Daemon health and introspection. Doesn't touch sessions.

```
Ping
Status
Logs { since, level, follow }
Doctor { reindex_state, check_adapters }
Adapters::List
Version
```

### Bucket 4 — ClientSpecific

Pane state, focus, scroll offsets. **These never leave clients.** The daemon doesn't see them. Documented here only because clients sometimes share them peer-to-peer (e.g., a TUI and a web client viewing the same session). v0.1 has none of these.

## Async I/O architecture

`#[tokio::main]` in `crates/daemon/src/main.rs`. Three concurrent layers:

### Layer 1 — IPC accept loop

```rust
let listener = UnixListener::bind(&socket_path)?;
loop {
    let (stream, _) = listener.accept().await?;
    tokio::spawn(handle_client(stream, daemon_state.clone()));
}
```

Each client gets its own task. Concurrent clients fine. Per-client task reads framed JSON, dispatches to handlers via `daemon_state`.

### Layer 2 — Per-session DAP I/O

For each active session:

- One task pumping the adapter's stdout into a `tokio::sync::mpsc` channel of typed `DapMessage`s.
- One task on the daemon side consuming the channel, routing responses to pending requests by `request_seq`, dispatching events to the broadcast channel.
- One task for execution-request queue (per D021): only one continue/step/pause in flight at a time.

Diagram:

```
adapter stdout ─→ read pump ─→ mpsc<DapMessage> ─→ session task
                                                      │
                                                      ├─ response → resolve pending
                                                      ├─ event    → broadcast
                                                      └─ output   → buffer + broadcast
```

### Layer 3 — Background tasks

- Adapter process supervisor (detects death via SIGCHLD, emits synthetic `terminated`)
- State persistence (debounced write to `.lazydap/state.toml`)
- Idle-shutdown timer (if enabled)

## Event broadcast and subscription

Clients subscribe to event kinds via `Subscribe { channels: Vec<EventKind> }`. Events stream over the same Unix socket connection.

```rust
enum EventKind {
    SessionStarted,
    SessionEnded,
    Stopped,           // program paused
    Continued,         // program resumed
    Output,            // captured stdout/stderr
    BreakpointUpdated, // breakpoint state changed
    ThreadStarted,
    ThreadExited,
    LogEntry,          // daemon's own logs (for `lazydap logs --follow`)
}
```

Multiple clients can subscribe simultaneously. The TUI subscribes to all session events. The agent skill typically does NOT subscribe — it uses `--wait` for synchronous blocking instead.

## Where state lives

| State | Where | Lifetime |
|---|---|---|
| Session structure (frame, scope) | Daemon RAM | Session |
| Breakpoint definitions | `.lazydap/state.toml` + Daemon RAM | Across sessions |
| Watch expressions | `.lazydap/state.toml` + Daemon RAM | Across sessions |
| Named launch configs | `.lazydap/state.toml` (or `.vscode/launch.json`) | Across sessions |
| Adapter binaries map | `~/.config/lazydap/config.toml` | Forever |
| Live captured output | Daemon RAM (ring buffer) | Session |
| Pending DAP requests | Daemon RAM | Until response or timeout |
| Daemon logs | `{data_dir}/lazydap.log` (rotating) | 100MB cap |

No SQLite. (See D006.)

## Where adapter binaries come from

Priority order:

1. Per-project config: `[adapter.codelldb] command = "/abs/path"` in `.lazydap/state.toml` or `~/.config/lazydap/config.toml`.
2. lazydap-managed install: `~/.local/share/lazydap/adapters/codelldb` (post-v0.1, optional).
3. PATH lookup: `which codelldb`.
4. Common locations: Mason install (`~/.local/share/nvim/mason/bin/codelldb`), VS Code extensions (`~/.vscode/extensions/...`).

The fallback chain handles existing nvim/VS Code users who already have adapters installed.

## Failure modes and recovery

| Failure | Detection | Recovery |
|---|---|---|
| Adapter dies mid-session | SIGCHLD | Synthetic `terminated` event broadcast, daemon awaits new `Launch` |
| DAP message malformed | parse error | Log, skip message, continue |
| Daemon dies | client socket EOF | Client retries `Ping`; if still down, auto-spawn |
| Client dies mid-request | TCP close | Daemon cancels pending DAP request if possible, releases queue slot |
| Disk full (state write) | I/O error | Log, retry once, then warn user, continue with in-memory state |
| Adapter not found at spawn | exec error | Return `Error::AdapterNotFound { tried_paths }` to client |

## Testing strategy

Inherited from mxr.

- **Unit tests** in each crate: pure functions, type roundtripping.
- **Adapter tests** with `adapter-fake`: deterministic, no I/O.
- **Integration tests** (`tests/`): real `codelldb` against tiny fixture binaries in `examples/`. Must run in CI.
- **CLI integration tests**: spawn real daemon, real CLI, real adapter, real fixture binary, assert JSON output shape.
- **No mocks of `DebugAdapter`** — that's `adapter-fake`'s job. Mocking is for external systems we don't own.

Snapshot tests via `insta` for JSON output stability.

## Performance considerations (post-v0.1)

Not optimised yet, but good defaults to keep:

- Variable expansion (`Variables { variables_reference }`) is lazy. Don't fetch a 10K-element array's children unless the user expands it.
- Source pane in TUI streams the file once and reuses it (a 50K-line source isn't loaded per render).
- Output ring buffer in daemon: 1MB cap by default, oldest dropped.
- DAP request timeout: 5s default for non-execution requests, override in config.

## What this architecture deliberately doesn't support

- **Cross-machine debug sessions.** lazydap is local-first. Want remote? Tunnel codelldb (it has a `--port` mode) and run lazydap on the remote.
- **Concurrent multiple-adapter sessions in one process.** Each session is one adapter. If you want to debug C and Python in parallel, run two `lazydap` daemons (different instances).
- **Hot-swappable adapters.** Killing the adapter ends the session. Replacing the adapter binary while it's running is undefined.
- **Persistent in-memory state across daemon restarts.** Live session state is not snapshotted. Persist breakpoints/watches; let the rest die.

## See also

- [`02-data-model.md`](02-data-model.md) — types and shapes
- [`03-adapters.md`](03-adapters.md) — `DebugAdapter` trait, adapter quirks
- [`04-protocol.md`](04-protocol.md) — full IPC schema
- [`10-async-to-sync.md`](10-async-to-sync.md) — `--wait` semantics, race conditions
