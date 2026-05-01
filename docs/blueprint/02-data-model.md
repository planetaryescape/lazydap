# 02 — Data model

The core types. All live in `crates/core/src/types.rs`. Zero I/O dependencies — these are plain data.

## Identifiers

Newtype wrappers around UUIDs (UUIDv7, time-sortable, mxr convention). Prevents mixing IDs at compile time.

```rust
pub struct SessionId(uuid::Uuid);
pub struct ThreadId(i64);          // adapter-assigned, not UUID — DAP uses i64
pub struct FrameId(i64);           // adapter-assigned
pub struct VariablesReference(i64); // adapter-assigned, lazy-eval cookie
pub struct BreakpointId(uuid::Uuid);
pub struct WatchId(uuid::Uuid);
pub struct LaunchConfigId(uuid::Uuid);
```

Why mix UUIDs and i64s: adapter-controlled IDs (thread, frame, variables-reference) come from DAP and we don't get to choose their type. lazydap-controlled IDs (breakpoint, watch, session) are ours and use UUIDv7 so they're time-sortable for logs.

## Session

```rust
pub struct Session {
    pub id: SessionId,
    pub adapter_kind: AdapterKind,           // codelldb, debugpy, ...
    pub project_root: PathBuf,
    pub launched_at: SystemTime,
    pub state: SessionState,
    pub program: PathBuf,
    pub program_args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub threads: BTreeMap<ThreadId, ThreadInfo>,
    pub captured_output: VecDeque<OutputChunk>,  // ring buffer
}

pub enum SessionState {
    Initializing,                            // sent `initialize`, awaiting response
    Configuring,                             // setting breakpoints before configurationDone
    Running,                                 // program executing
    Paused { reason: PauseReason, thread_id: ThreadId },
    Exited { code: Option<i32> },
    Terminated,
    AdapterDied { exit_status: i32 },        // synthetic state
}

pub enum PauseReason {
    Step,
    Breakpoint { ids: Vec<BreakpointId> },
    Exception { description: String },
    Pause,                                   // user-requested
    Entry,                                   // stop_on_entry
    Goto,
    FunctionBreakpoint,
    DataBreakpoint,
    InstructionBreakpoint,
    Other(String),
}
```

## Thread, frame, scope, variable

Mirror DAP shapes, but lazydap-typed (so we don't leak `dap::Thread` into clients).

```rust
pub struct ThreadInfo {
    pub id: ThreadId,
    pub name: String,
    pub state: ThreadState,
}

pub enum ThreadState {
    Running,
    Stopped { reason: PauseReason },
    Exited,
}

pub struct StackFrame {
    pub id: FrameId,
    pub name: String,                        // function name
    pub source: Option<Source>,
    pub line: u32,
    pub column: u32,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
    pub presentation_hint: FramePresentationHint,
}

pub enum FramePresentationHint {
    Normal,
    Label,                                   // synthetic frame, e.g. "<error>"
    Subtle,                                  // de-emphasise (framework code)
}

pub struct Source {
    pub name: String,
    pub path: Option<PathBuf>,               // None for synthetic sources
    pub source_reference: Option<i64>,       // for adapter-served virtual sources
}

pub struct Scope {
    pub name: String,                        // "Locals", "Arguments", "Globals"
    pub variables_reference: VariablesReference,
    pub presentation_hint: ScopePresentationHint,
    pub expensive: bool,                     // suggest UI lazy-loads
}

pub enum ScopePresentationHint {
    Arguments,
    Locals,
    Registers,
    Other(String),
}

pub struct Variable {
    pub name: String,
    pub value: String,                       // already-formatted display value
    pub type_name: Option<String>,
    pub variables_reference: VariablesReference, // 0 if no children, else use to expand
    pub indexed_variables: Option<u32>,      // for arrays
    pub named_variables: Option<u32>,        // for structs
    pub memory_reference: Option<String>,
    pub presentation_hint: VariablePresentationHint,
}

pub struct VariablePresentationHint {
    pub kind: Option<String>,                // "property" | "method" | "class" | ...
    pub attributes: Vec<String>,             // "readOnly" | "constant" | ...
    pub visibility: Option<String>,          // "public" | "private" | ...
}
```

The `variables_reference` cookie is the lazy-eval handle. UI calls `Variables { variables_reference: vr }` only when the user expands a row. Don't pre-fetch.

## Breakpoint

Two layers: source breakpoint (what the user wants) and adapter breakpoint (what the adapter set). The adapter may move/resolve breakpoints (line wasn't executable; adapter snaps to nearest).

```rust
pub struct SourceBreakpoint {
    pub id: BreakpointId,                    // lazydap-assigned, stable across sessions
    pub source: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
    pub condition: Option<String>,           // "x > 5"
    pub hit_condition: Option<String>,       // ">= 10"
    pub log_message: Option<String>,         // logpoint: prints instead of pausing
    pub enabled: bool,
}

pub struct AdapterBreakpoint {
    pub source_breakpoint: BreakpointId,
    pub adapter_id: i64,                     // DAP's id, may differ across sessions
    pub verified: bool,
    pub message: Option<String>,             // adapter's note ("source unavailable")
    pub source: Option<Source>,
    pub line: Option<u32>,                   // post-resolution; may differ from request
    pub column: Option<u32>,
    pub end_line: Option<u32>,
    pub end_column: Option<u32>,
}
```

The persistent `.lazydap/state.toml` stores `SourceBreakpoint`s (lazydap's view). On session start, the daemon sends them via `setBreakpoints`, gets back resolution info, populates `AdapterBreakpoint`s in RAM only.

## Watch

```rust
pub struct Watch {
    pub id: WatchId,
    pub expression: String,
    pub label: Option<String>,               // user-supplied display name
    pub enabled: bool,
}

pub struct WatchValue {
    pub watch_id: WatchId,
    pub value: String,
    pub error: Option<String>,
    pub variables_reference: VariablesReference,  // for expansion
    pub evaluated_at: SystemTime,
}
```

`Watch` persists. `WatchValue` is per-session, computed on each pause via `evaluate`.

## Launch config

```rust
pub struct LaunchConfig {
    pub id: LaunchConfigId,
    pub name: String,                        // human label
    pub adapter: AdapterKind,
    pub kind: LaunchKind,                    // launch | attach
    pub program: Option<PathBuf>,            // for launch
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,                // default: project root
    pub env: BTreeMap<String, String>,
    pub stop_on_entry: bool,
    pub source: LaunchConfigSource,
}

pub enum LaunchKind {
    Launch,
    Attach { pid: Option<i64> },             // pid optional — prompt at runtime
}

pub enum LaunchConfigSource {
    LazydapState,                            // .lazydap/state.toml
    VsCodeLaunchJson { name: String },       // imported from .vscode/launch.json
    AdHoc,                                   // built from CLI args, not persisted
}
```

## Adapter kind

```rust
pub enum AdapterKind {
    CodeLldb,
    DebugPy,                                 // post-v0.1
    JsDebug,                                 // post-v0.1
    Delve,                                   // post-v0.1
    LldbDap,                                 // ships with LLVM, alternative to codelldb
    Fake,                                    // for tests
    Custom { name: String },                 // user-defined adapter
}
```

`AdapterKind` is for routing: tells the daemon which adapter crate to dispatch to. Each adapter crate registers itself.

## Output

```rust
pub struct OutputChunk {
    pub category: OutputCategory,
    pub output: String,
    pub source: Option<Source>,              // some adapters report origin
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub timestamp: SystemTime,
}

pub enum OutputCategory {
    Stdout,
    Stderr,
    Console,                                 // adapter's own messages
    Telemetry,                               // adapter telemetry, usually ignored
    Important,                               // user-visible warnings
}
```

The daemon keeps a ring buffer of output per session (default 1MB, configurable). `lazydap output --since <ts>` returns chunks since a timestamp. `Subscribe { channels: [Output] }` streams them live.

## Errors

```rust
pub enum LazydapError {
    AdapterNotFound { kind: AdapterKind, tried_paths: Vec<PathBuf> },
    AdapterCrashed { kind: AdapterKind, exit_code: i32 },
    AdapterTimeout { request_kind: String, after: Duration },
    SessionNotFound { id: SessionId },
    SessionAlreadyActive { existing: SessionId },  // v0.1 only
    InvalidLaunchConfig { reason: String },
    DapProtocolError { message: String, raw: Option<String> },
    Io(io::Error),
    Toml(toml::Error),
    InvalidProjectRoot { path: PathBuf, reason: String },
    DaemonUnreachable,
    Timeout(Duration),
}
```

Errors serialise to `{ "error": "AdapterNotFound", "message": "...", "details": {...} }` for JSON output. Exit codes mapped per `AGENTS.md`.

## What's NOT in the data model

- **Pane state** (focused frame, scroll offset) — that's TUI-only, lives in `crates/tui/`. Daemon doesn't see it.
- **Adapter capabilities** — we model them in `crates/dap/` as `Capabilities`, not in core. Daemon negotiates with adapter; clients don't need to know.
- **Raw DAP messages** — they live in `crates/dap/`. Core types are lazydap's vocabulary, not DAP's.
- **Render-formatting state** (highlights, syntax themes) — TUI-only.

## Versioning

The `crates/protocol` IPC types and `crates/core` types are versioned together. Breaking changes to either are visible to all clients and require a major version bump. We pin the IPC version in every message; clients refuse to talk if the daemon version doesn't match.

```rust
pub const LAZYDAP_PROTOCOL_VERSION: u32 = 1;
```

Bumping this is a 15-decision-log entry.

## See also

- [`04-protocol.md`](04-protocol.md) — how these types ride over IPC
- [`03-adapters.md`](03-adapters.md) — how DAP types map to these core types
- [`08-state-and-config.md`](08-state-and-config.md) — TOML serialisation of persistable types
