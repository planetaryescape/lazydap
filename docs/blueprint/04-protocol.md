# 04 — Protocol

The lazydap IPC protocol. Length-delimited JSON over Unix socket. Stable across clients. Versioned.

## Frame format

```
+----+----+----+----+----+----+----+----+----+----+----+----+
| 4-byte big-endian length      | JSON body (length bytes)  |
+-------------------------------+---------------------------+
```

4-byte big-endian unsigned length prefix, then exactly that many UTF-8 bytes of JSON. Same as LSP's framing minus the `Content-Length: \r\n\r\n` header — simpler, slightly less self-descriptive, easier to parse.

(Inherited from mxr.)

## Top-level message

```rust
pub struct IpcMessage {
    pub version: u32,                        // LAZYDAP_PROTOCOL_VERSION
    pub id: u64,                             // monotonic per-connection
    pub payload: IpcPayload,
}

pub enum IpcPayload {
    Request(Request),
    Response(Response),
    Event(Event),
    Error(IpcError),
}
```

`id` correlates request and response. Events have id = 0.

## Request types — bucketed

Per [`01-architecture.md`](01-architecture.md), four buckets. Each variant maps to a CLI subcommand and (for non-`ClientSpecific` ones) to a daemon handler.

### Bucket 1 — Session

```rust
pub enum Request {
    // Session lifecycle
    Launch(LaunchRequest),
    Attach(AttachRequest),
    Disconnect { session_id: SessionId, terminate: bool },

    // Stepping
    Continue { session_id: SessionId, thread_id: Option<ThreadId>, wait: WaitMode, all_threads: bool },
    Step { session_id: SessionId, thread_id: ThreadId, granularity: StepGranularity, wait: WaitMode },
    StepIn { session_id: SessionId, thread_id: ThreadId, target: Option<i64>, wait: WaitMode },
    StepOut { session_id: SessionId, thread_id: ThreadId, wait: WaitMode },
    Pause { session_id: SessionId, thread_id: Option<ThreadId> },

    // Breakpoints (live)
    SetBreakpoints { session_id: SessionId, source: PathBuf, breakpoints: Vec<SourceBreakpoint> },

    // Inspection
    Threads { session_id: SessionId },
    StackTrace { session_id: SessionId, thread_id: ThreadId, start_frame: Option<u32>, levels: Option<u32> },
    Scopes { session_id: SessionId, frame_id: FrameId },
    Variables { session_id: SessionId, variables_reference: VariablesReference, filter: VariableFilter, start: Option<u32>, count: Option<u32> },
    Eval { session_id: SessionId, expression: String, frame_id: Option<FrameId>, context: EvalContext },
    Source { session_id: SessionId, source_reference: i64 },

    // Convenience: snapshot (for AI clients)
    GetStateSnapshot { session_id: SessionId, depth: u32, source_radius: u32 },

    // Subscription (replaces polling)
    Subscribe { channels: Vec<EventKind> },
    Unsubscribe { channels: Vec<EventKind> },

    // ... bucket 2 + 3 below
}
```

`WaitMode`:

```rust
pub enum WaitMode {
    NoWait,                                  // fire and forget
    Wait { timeout_ms: Option<u32> },        // None = use default 30s
}
```

### Bucket 2 — Project

```rust
pub enum Request {
    // ... bucket 1 above

    // Persistent breakpoints
    BreakpointList,
    BreakpointAdd(SourceBreakpoint),
    BreakpointRemove { id: BreakpointId },
    BreakpointToggle { id: BreakpointId },

    // Watches
    WatchList,
    WatchAdd { expression: String, label: Option<String> },
    WatchRemove { id: WatchId },

    // Launch configs
    LaunchConfigList,
    LaunchConfigAdd(LaunchConfig),
    LaunchConfigRun { name: String, dry_run: bool },
}
```

### Bucket 3 — Diagnostics

```rust
pub enum Request {
    Ping,
    Status,
    Logs { since: Option<SystemTime>, level: Option<String>, follow: bool, limit: Option<u32> },
    Doctor { check_adapters: bool, check_state: bool },
    AdaptersList,
    Version,
}
```

### Bucket 4 — ClientSpecific

Empty in v0.1. Reserved namespace for client-to-client peer protocol (e.g., a TUI sharing focus state with a web client viewing the same daemon). Daemon doesn't process these.

## Response types

```rust
pub enum Response {
    Launched { session_id: SessionId, capabilities: Capabilities, state: SessionState },
    Disconnected,
    Continued,
    Stepped { state: StableState },          // when wait was used
    Paused(StableState),                     // alternative shape if continue --wait returns paused
    Threads(Vec<ThreadInfo>),
    StackTrace { frames: Vec<StackFrame>, total: Option<u32> },
    Scopes(Vec<Scope>),
    Variables(Vec<Variable>),
    EvalResult { value: String, type_name: Option<String>, variables_reference: VariablesReference },
    Source { content: String, mime_type: Option<String> },
    StateSnapshot(StateSnapshot),

    BreakpointList(Vec<SourceBreakpoint>),
    Breakpoint(SourceBreakpoint),

    WatchList(Vec<Watch>),
    WatchValues(Vec<WatchValue>),

    LaunchConfigList(Vec<LaunchConfig>),
    LaunchConfigRun { session_id: SessionId, dry_run: bool, would_launch: LaunchPreview },

    Pong { uptime: Duration, instance: String },
    Status(StatusReport),
    LogChunk(Vec<LogEntry>),
    DoctorReport(DoctorReport),
    AdaptersList(Vec<AdapterInfo>),
    Version { lazydap: String, protocol: u32 },

    Subscribed,
    Unsubscribed,
}
```

`StableState` is the "what just happened during the wait" structure:

```rust
pub struct StableState {
    pub state: SessionState,                 // Paused / Exited / Terminated / Timeout / AdapterDied
    pub reason: Option<PauseReason>,
    pub thread_id: Option<ThreadId>,
    pub all_threads_stopped: bool,
    pub additional_stopped_threads: Vec<ThreadId>,
    pub hit_breakpoint_ids: Vec<BreakpointId>,
    pub exit_code: Option<i32>,
    pub frame: Option<StackFrame>,           // top frame, populated when paused
    pub captured_output: Vec<OutputChunk>,
    pub breakpoint_updates: Vec<AdapterBreakpoint>,
    pub thread_updates: Vec<ThreadUpdate>,
    pub elapsed_ms: u64,
}
```

This is the "thing agents care about" shape. One JSON blob per `--wait` call.

## Event types

Streamed to subscribed clients. Same `IpcMessage` envelope, `payload: IpcPayload::Event(...)`.

```rust
pub enum Event {
    SessionStarted { session_id: SessionId, adapter: AdapterKind },
    SessionEnded { session_id: SessionId, reason: EndReason },
    Stopped { session_id: SessionId, thread_id: ThreadId, reason: PauseReason, all_threads_stopped: bool },
    Continued { session_id: SessionId, thread_id: Option<ThreadId>, all_threads_continued: bool },
    Output(OutputChunk),
    BreakpointUpdated(AdapterBreakpoint),
    ThreadStarted { session_id: SessionId, thread_id: ThreadId, name: String },
    ThreadExited { session_id: SessionId, thread_id: ThreadId },
    LogEntry(LogEntry),
}
```

Event kinds map 1:1 to subscription channels. `Subscribe { channels: [Stopped, Output] }` only delivers those.

## Errors

```rust
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
    pub details: serde_json::Value,
}

pub enum ErrorCode {
    AdapterNotFound,
    AdapterCrashed,
    AdapterTimeout,
    SessionNotFound,
    SessionAlreadyActive,
    InvalidLaunchConfig,
    InvalidProjectRoot,
    DapProtocolError,
    DaemonInternalError,
    Unsupported,                             // adapter doesn't support this
    Timeout,
    Cancelled,
    BadRequest,
    VersionMismatch,
}
```

Errors flow as `IpcPayload::Error` for the corresponding request `id`. Always paired with a request.

## Versioning

`version: u32` in every `IpcMessage`. Daemon and clients pin a version at compile time. On connect, the first message exchange checks: if mismatch, daemon returns `Error::VersionMismatch` with both versions, client exits with code 3.

Bumping `LAZYDAP_PROTOCOL_VERSION` requires a [`15-decision-log.md`](15-decision-log.md) entry. Don't bump casually.

## Streaming responses (post-v0.1)

For long-running queries (e.g., `Variables` on a 100K-element array, `Logs --follow`), responses can be streamed. Same envelope, but multiple `Response` payloads with the same `id`, ending with a sentinel `Response::StreamEnd`. Out of scope for v0.1; mention here for posterity.

## What this protocol is *not*

- **Not DAP.** lazydap protocol uses lazydap types. Adapters speak DAP. Translation is the daemon's job.
- **Not gRPC.** No service definition language. Plain enums + serde. JSON for debuggability.
- **Not a transport contract.** This document defines messages. The Unix socket framing is in [`01-architecture.md`](01-architecture.md). Other transports (HTTP, WebSocket) are out-of-tree and translate this protocol.
- **Not stable yet.** Until v0.1 ships, the protocol can change without notice. Once v0.1 ships, breaking changes require a major version bump.

## Examples

### Launch

Request:
```json
{
  "version": 1,
  "id": 1,
  "payload": {
    "Request": {
      "Launch": {
        "adapter": "CodeLldb",
        "program": "/Users/bhekanik/code/c-beans/cmake-build-debug/c_beans",
        "args": [],
        "cwd": "/Users/bhekanik/code/c-beans",
        "env": {},
        "stop_on_entry": true
      }
    }
  }
}
```

Response:
```json
{
  "version": 1,
  "id": 1,
  "payload": {
    "Response": {
      "Launched": {
        "session_id": "01ABC...",
        "capabilities": { "supports_configuration_done_request": true, ... },
        "state": { "Paused": { "reason": "Entry", "thread_id": 1 } }
      }
    }
  }
}
```

### Continue --wait

Request:
```json
{
  "version": 1,
  "id": 5,
  "payload": {
    "Request": {
      "Continue": {
        "session_id": "01ABC...",
        "thread_id": null,
        "wait": { "Wait": { "timeout_ms": 30000 } },
        "all_threads": false
      }
    }
  }
}
```

Response:
```json
{
  "version": 1,
  "id": 5,
  "payload": {
    "Response": {
      "Stepped": {
        "state": "Paused",
        "reason": { "Breakpoint": { "ids": ["bp-01XYZ..."] } },
        "thread_id": 1,
        "all_threads_stopped": true,
        "additional_stopped_threads": [],
        "hit_breakpoint_ids": ["bp-01XYZ..."],
        "exit_code": null,
        "frame": {
          "id": 42,
          "name": "main",
          "source": { "name": "main.c", "path": "/Users/bhekanik/code/c-beans/main.c", "source_reference": null },
          "line": 42,
          "column": 1,
          ...
        },
        "captured_output": [
          { "category": "Stdout", "output": "Starting...\n", "timestamp": "..." }
        ],
        "breakpoint_updates": [],
        "thread_updates": [],
        "elapsed_ms": 124
      }
    }
  }
}
```

### Subscribe

Request:
```json
{ "version": 1, "id": 2, "payload": { "Request": { "Subscribe": { "channels": ["Stopped", "Output"] } } } }
```

Response:
```json
{ "version": 1, "id": 2, "payload": { "Response": "Subscribed" } }
```

Then events flow as new messages on the same connection:

```json
{ "version": 1, "id": 0, "payload": { "Event": { "Output": { "category": "Stdout", "output": "hello\n", ... } } } }
{ "version": 1, "id": 0, "payload": { "Event": { "Stopped": { "session_id": "...", "thread_id": 1, "reason": "Step", ... } } } }
```

## See also

- [`10-async-to-sync.md`](10-async-to-sync.md) — `WaitMode` semantics, what `Stepped` actually means
- [`02-data-model.md`](02-data-model.md) — types referenced here
- [`06-cli.md`](06-cli.md) — how subcommands map to requests
