# 03 — Adapters

The adapter layer is where DAP-protocol details hide. Above this layer, lazydap-typed events; below it, raw DAP JSON.

## The trait

`crates/core/src/adapter.rs`:

```rust
#[async_trait]
pub trait DebugAdapter: Send + Sync {
    /// Adapter family identifier.
    fn kind(&self) -> AdapterKind;

    /// Spawn the adapter process and complete the DAP handshake.
    async fn launch(&self, config: LaunchConfig) -> Result<Capabilities>;

    /// Send a DAP request. Returns the typed response.
    async fn request(&self, req: dap::Request) -> Result<dap::Response>;

    /// Stream of DAP events from the adapter.
    fn events(&self) -> mpsc::Receiver<dap::Event>;

    /// Clean shutdown.
    async fn disconnect(&self, terminate: bool) -> Result<()>;
}
```

Each adapter crate (`adapter-codelldb`, `adapter-debugpy`, etc.) provides a struct implementing `DebugAdapter`. The daemon routes by `AdapterKind`.

## Why a trait, not raw DAP everywhere

If the daemon talked raw DAP, every adapter quirk would leak into the daemon. codelldb's `runInTerminal` workaround. debugpy's request-serialisation behaviour. js-debug's child-session orchestration. None of that should be the daemon's problem.

The trait is the seam. Adapter-specific quirks stay in adapter crates. Daemon talks to a clean interface.

## DAP transport

`crates/dap/` owns the wire protocol:

- **Framing:** `Content-Length: N\r\n\r\n` headers, JSON body.
- **Read pump:** `tokio::io::AsyncRead` → typed `DapMessage` over `mpsc`.
- **Write pump:** typed request → JSON → framed write to `stdin`.
- **Codec types:** `dap::Request`, `dap::Response`, `dap::Event` (all `serde::{Serialize, Deserialize}`).

Each adapter spawns a child process, reads stdout via the read pump, writes stdin via the write pump. Stderr is captured to the daemon log (adapter chatter).

## codelldb (M0 adapter — v0.1 target)

`crates/adapter-codelldb/`. Wraps [vadimcn/codelldb](https://github.com/vadimcn/codelldb).

### Spawning

codelldb operates in two modes:

- **stdio mode**: `codelldb --port 0` writes the chosen port to stderr; client connects via TCP.
- **TCP server mode**: `codelldb --port <N>` listens.

We use TCP server mode (`--port 0`, parse port from stderr line `Listening on port N`). codelldb does NOT support raw stdio DAP — has to be TCP. Surprising but true; first quirk to encode.

### Capabilities

codelldb advertises:

- `supportsConfigurationDoneRequest: true`
- `supportsFunctionBreakpoints: true`
- `supportsConditionalBreakpoints: true`
- `supportsHitConditionalBreakpoints: true`
- `supportsEvaluateForHovers: true`
- `supportsLogPoints: true`
- `supportsSetVariable: true`
- `supportsGotoTargetsRequest: true`
- `supportsCompletionsRequest: true`
- `supportsRestartFrame: true`
- `supportsValueFormattingOptions: true`
- `supportsExceptionInfoRequest: true`
- `supportsTerminateThreadsRequest: true`
- `supportsRestartRequest: true`
- `supportsExceptionOptions: true`
- `supportsDisassembleRequest: true`
- `supportsModulesRequest: true`
- `supportsLoadedSourcesRequest: true`
- `supportsReadMemoryRequest: true`
- `supportsWriteMemoryRequest: true`
- `supportsCancelRequest: true`

Plus codelldb-specific extensions (LLDB commands, raw expressions, etc.) under `customCommands`. We expose these in v0.2+, not v0.1.

### Launch config shape (codelldb-specific)

```json
{
  "type": "lldb",
  "request": "launch",
  "program": "/path/to/binary",
  "args": ["..."],
  "cwd": "/project/root",
  "env": { "KEY": "value" },
  "stopOnEntry": true,
  "preRunCommands": ["..."],
  "initCommands": ["..."],
  "exitCommands": ["..."],
  "console": "integratedTerminal" | "internalConsole" | "externalTerminal",
  "terminal": "integrated" | "external" | "console",
  "expressions": "simple" | "python" | "native"
}
```

`expressions: "native"` lets you eval raw LLDB expressions (`p (int)x` etc.). `expressions: "simple"` is friendlier syntax. We default to `"simple"`.

### Known quirks

- **TCP-only transport.** Already mentioned. Not stdio.
- **Slow first launch.** Cold starts can take 2–4s while codelldb loads its Python runtime. Not a bug.
- **Crashes on missing debug symbols.** Returns a clear error; not a lazydap concern.
- **`runInTerminal` reverse request** — adapter asks the client to spawn a terminal for I/O. We respond "no, use console" by setting `console: "internalConsole"`. Reverse requests are routed through the IPC contract as a `Reverse` event clients can opt into; default policy refuses runInTerminal.

## debugpy (post-v0.1)

`crates/adapter-debugpy/`. Wraps [microsoft/debugpy](https://github.com/microsoft/debugpy).

### Spawning

```sh
python -m debugpy --listen 0 --wait-for-client <script.py> ...
```

Or `debugpy-adapter` (separate binary, simpler):

```sh
debugpy-adapter [--port N]
```

Use `debugpy-adapter` if available; fall back to `python -m debugpy`.

### Quirks

- **Multiple adapters can launch simultaneously** but the protocol gets confused; queue per-session.
- **`pathMappings`** for source resolution between local and remote (Docker). v0.2+ feature.
- **Subprocess debugging** (`subProcess: true`). Out of scope until multi-session support.

## Adapter quirks visible in the protocol

We made the call (in [`15-decision-log.md`](15-decision-log.md) D023-equivalent for adapters) to keep adapter quirks visible *where they matter*, not papered over. Examples:

- codelldb's `expressions: "native"` is exposed as a launch config option.
- debugpy's source path mapping is exposed; we don't pretend everything's local.
- Adapter capability differences are returned in `lazydap status --format json` so clients know what's available.

This is mxr's "label-vs-folder seam" pattern: provider differences are visible where they cost users behaviourally. We don't over-abstract.

## Adapter discovery

Per [`15-decision-log.md`](15-decision-log.md) O03 (proposed):

1. Per-project: `[adapter.codelldb] command = "/abs/path"` in `.lazydap/state.toml`
2. Per-user: same in `~/.config/lazydap/config.toml`
3. lazydap-managed: `~/.local/share/lazydap/adapters/{kind}` (post-v0.1)
4. PATH lookup: `which codelldb`
5. Common locations:
   - `~/.local/share/nvim/mason/bin/{kind}` (Mason)
   - `~/.vscode/extensions/vadimcn.vscode-lldb-*/adapter/codelldb` (VS Code extension)

`lazydap doctor` reports which path it found and which it would use.

## The fake adapter

`crates/adapter-fake/`. In-process, no subprocess. Implements `DebugAdapter` against a hand-rolled state machine. Used for:

- Unit tests (fast, deterministic)
- Demo / docs (record a scripted session)
- CI (runs everywhere without installing real adapters)

The fake adapter cannot run real binaries. It returns canned events (`stopped` after 100ms, hand-crafted stack frames, etc.). Useful for testing the lazydap layer; useless for testing real DAP integration.

Real adapters get their own integration test suite that runs in CI but only on platforms where they install.

## How to add a new adapter

1. Create `crates/adapter-XXXX/`.
2. Add to root `Cargo.toml` workspace members.
3. Implement `DebugAdapter` for `XxxxAdapter` struct.
4. Register the adapter kind in `crates/core/src/types.rs::AdapterKind`.
5. Wire daemon dispatch in `crates/daemon/src/adapters/mod.rs`.
6. Add discovery paths to `crates/config/src/discovery.rs`.
7. Write an integration test in `tests/adapter_xxxx.rs` against a fixture program in `examples/`.
8. Update `references/commands.md` if the adapter exposes unique flags.

A new adapter is ~500–1500 LoC depending on how weird it is. codelldb is ~1000. debugpy probably similar. js-debug is the gnarly one (multi-session orchestration).

## See also

- [`02-data-model.md`](02-data-model.md) — the lazydap types adapters produce
- [`04-protocol.md`](04-protocol.md) — IPC contract above adapters
- [`docs/reference/dap-protocol-cheatsheet.md`](../reference/dap-protocol-cheatsheet.md) — the DAP messages we use (TODO)
