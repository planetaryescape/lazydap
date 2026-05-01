# Architecture

## The core tenet

> **Scriptability before features.** Every operation flows through a single JSON-over-Unix-socket protocol. The CLI is the canonical client. Anyone — in any language — can build a frontend by speaking the protocol. We do not add features that bypass it. Ever.

This is not a soft principle. It's enforced by the crate-dependency graph: clients (TUI, web, scripts) literally cannot reach into daemon-internal crates, so they cannot bypass the IPC contract. If you add a feature only in the TUI, the build breaks. If you want to add it, you go through the protocol.

## High-level shape

```
   Agent skill        TUI          Electron        Web app       Custom script
       │              │              │               │                │
       └──────────────┴──────────────┴───────────────┴────────────────┘
                                     │
                                     │  lazydap protocol
                                     │  (length-delimited JSON over Unix socket)
                                     ▼
                                lazydap-daemon
                                     │
                                     │  DAP (verbose, stateful, raw)
                                     ▼
                       codelldb · debugpy · delve · js-debug · ...
                                     │
                                     ▼
                          your program (the debuggee)
```

## Borrowed wholesale from mxr

lazydap inherits mxr's architectural decisions. They are battle-tested and the rationale is documented in mxr's `docs/blueprint/15-decision-log.md`. Inherited:

1. **Single unified binary** (`lazydap`) with subcommands. Bare `lazydap` enters TUI if interactive. `lazydap daemon --foreground` runs the daemon explicitly.
2. **Daemon auto-spawns.** First subcommand that needs it forks the daemon, drops a PID file, binds the socket. Clients probe and reconnect.
3. **Strict crate boundaries enforced by Cargo, not convention.** `lazydap-core` has no I/O. `lazydap-protocol` defines the IPC contract. `lazydap-daemon` is the only thing that depends on store/adapters/sessions. TUI and other clients can only depend on `core`, `protocol`, `config`.
4. **CLI-first culture, hard rule.** Every TUI action has a CLI equivalent. No feature in one client only.
5. **JSON output is a product feature.** Stable schema, pipeable, auto-detected when stdout is not a TTY. `--format table|json|csv|ids`.
6. **`--dry-run` + `--yes` for every mutation.** Selection logic must match the real mutation path.
7. **`tracing` from the start.** Structured logs to file in background mode, human-readable to stderr in foreground.
8. **Tests with real adapters, not mocks.** A `FakeAdapter` exists for fast unit-style tests, but integration tests cross the full daemon ↔ adapter path.

## Where lazydap diverges from mxr

Two domain-driven differences:

### 1. Push-heaviness

Email is poll-based; debugging is push-based. The DAP adapter fires `stopped` / `output` / `breakpoint` / `thread` events constantly during a live session. Implications:

- The daemon ↔ client event broadcast (`tokio::sync::broadcast` in mxr) is **first-class** in lazydap. Multiple clients (TUI, agent, web) subscribe to event streams in real time.
- `Subscribe { channels: Vec<EventKind> }` is a top-level IPC operation.
- Agents typically run one command at a time, not a long-lived stream. So step/continue commands take an optional `--wait` flag that turns the async push back into a synchronous "do this, return next stable state" response. The bridging logic lives in the daemon.

See [`docs/blueprint/10-async-to-sync.md`](docs/blueprint/10-async-to-sync.md).

### 2. State lifetime

Most lazydap state dies with the debug session. Only a few things persist across sessions, and those go in TOML, not SQLite:

- **`.lazydap/state.toml`** per project: breakpoints, watch expressions, named launch configs.
- **`~/.config/lazydap/config.toml`**: global preferences, default adapter paths.
- Live session state (current frames, threads, scopes, paused state) lives in daemon memory. Dies with the session by design.

TOML over SQLite because the state is small, human-readable, version-controllable, and scriptable from any language. No migrations, no `.db` files in `.gitignore` (or not, your call).

See [`docs/blueprint/08-state-and-config.md`](docs/blueprint/08-state-and-config.md).

## Crate layout

```
lazydap/                                 ← Cargo workspace root
├── Cargo.toml
├── crates/
│   ├── core/                            ← types, errors, trait `DebugAdapter`. Zero I/O.
│   ├── protocol/                        ← lazydap IPC types: IpcMessage, Request, Response, Event
│   ├── dap/                             ← raw DAP message types + transport (Content-Length framing)
│   ├── adapter-codelldb/                ← codelldb wrapper (C/C++/Rust). Implements DebugAdapter.
│   ├── adapter-debugpy/                 ← debugpy wrapper (Python). Post-v0.1.
│   ├── adapter-js-debug/                ← js-debug wrapper (Node/TS). Post-v0.1.
│   ├── adapter-fake/                    ← in-process fake for tests
│   ├── store/                           ← TOML state persistence (per-project breakpoints + watches)
│   ├── config/                          ← TOML config loader, project-root detection, launch.json import
│   ├── tui/                             ← ratatui client. Talks IPC only.
│   └── daemon/                          ← daemon binary `lazydap` + CLI subcommand handlers
├── examples/                            ← hello-world C/Rust/Python programs to debug
├── docs/                                ← this directory tree
├── tests/                               ← workspace-level integration tests
└── benches/                             ← perf benchmarks (post-v0.1)
```

**Dependency rule (enforced in CI):**

- `core` depends on nothing internal.
- `protocol` depends only on `core`.
- `dap` depends on nothing internal (raw protocol).
- Each `adapter-*` depends on `core`, `dap`. NOT on `daemon`, `store`, or other adapters.
- `store` depends on `core`.
- `config` depends on `core`.
- `tui` depends on `core`, `protocol`, `config`. **NOT** on `daemon`, `store`, `dap`, or any adapter.
- `daemon` depends on everything except `tui`.

## IPC contract

The protocol has four buckets, every new request must classify into one:

| Bucket | Purpose | Examples |
|---|---|---|
| **Session** | Live debug session control | `Launch`, `Attach`, `Continue`, `Step`, `Pause`, `SetBreakpoints`, `Eval`, `StackTrace`, `Scopes`, `Variables` |
| **Project** | Per-project persistent state | `Breakpoint::List/Add/Remove`, `Watch::List/Add/Remove`, `LaunchConfig::List/Add/Run` |
| **Diagnostics** | Daemon health & introspection | `Status`, `Logs`, `Doctor`, `Adapters::List`, `Version` |
| **ClientSpecific** | Pane state, focus, scroll — never leaves clients | (TUI/web only — daemon doesn't see these) |

If a new request doesn't fit, the protocol design is wrong, not the request. Don't add a fifth bucket without a deliberate decision recorded in `docs/blueprint/15-decision-log.md`.

## What this implies for adding features

Adding a new debug capability:

1. Define the request/response in `crates/protocol/src/`.
2. Implement the daemon handler in `crates/daemon/src/handlers/`.
3. Add a CLI subcommand in `crates/daemon/src/cli/`.
4. Wire the TUI to use it — same IPC call.
5. Update `references/commands.md` in the skill so agents discover it.

Steps 3 and 4 are non-negotiable. If you can't do both, don't ship the feature.

## Anti-patterns to avoid (paid for in mxr's pain)

1. Don't bypass crate boundaries with `#[path]` shenanigans. Use real workspace crates.
2. Don't implement a feature only in the TUI. CLI equivalent is mandatory.
3. Don't make `--wait` mean different things in different commands. Pick one definition (see `docs/blueprint/10-async-to-sync.md`) and stick to it.
4. Don't let DAP-protocol details leak past the adapter crates. The daemon talks to `DebugAdapter` traits, not raw DAP.
5. Don't ship mutations without `--dry-run`. Selection logic must match real mutation path.
6. Don't build the TUI before the protocol stabilises. M11 — when DAP first wires into the TUI — is deliberately late.
7. Don't pipeline DAP requests to one adapter. Queue them. (See [`docs/blueprint/10-async-to-sync.md`](docs/blueprint/10-async-to-sync.md) §race conditions.)
8. Don't add AI features into the core protocol. They're external clients of the same protocol. (See [`docs/blueprint/12-ai-future.md`](docs/blueprint/12-ai-future.md).)

## Further reading

- [`docs/blueprint/00-overview.md`](docs/blueprint/00-overview.md) — what lazydap is, scope, principles
- [`docs/blueprint/01-architecture.md`](docs/blueprint/01-architecture.md) — this doc, expanded
- [`docs/blueprint/04-protocol.md`](docs/blueprint/04-protocol.md) — full IPC schema
- [`docs/blueprint/06-cli.md`](docs/blueprint/06-cli.md) — CLI surface
- [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) — every decision with rationale
