# 15 — Decision log

Every load-bearing design decision in lazydap, with rationale. Add D-numbered entries as new decisions arise. **Don't change history** — if a decision changes, add a new entry that supersedes the old, and mark the old one with a `(superseded by Dnnn)` note.

Status legend:

- **decided** — locked in, code reflects it
- **proposed** — written down, not yet exercised in code
- **open** — needs decision, blocking
- **superseded** — replaced by a later decision

---

## D001 — Use Rust as the implementation language

**Status:** decided.

**Why:** Author is learning Rust and wants this project to deepen that. Single-binary distribution is cheap. Async story (tokio) is mature for this domain. ratatui is best-in-class for TUIs. mxr is in Rust and lazydap inherits from it.

**Alternatives considered:** Go (Bubbletea ecosystem more mature for TUIs, lazygit-adjacent), TypeScript (Ink + agent integration easier), Zig (premature). Rust wins on alignment with author's learning goals.

---

## D002 — Single binary with subcommands

**Status:** decided.

**Why:** Inherited from mxr. Bare `lazydap` enters TUI if interactive; subcommands `lazydap launch`, `lazydap break`, etc. CLI is canonical, TUI is one client. Avoids `lazydap-tui` / `lazydap-cli` / `lazydap-daemon` proliferation.

**Alternatives considered:** Separate binaries per role (more cargo overhead, no benefit). Library + thin CLI (still ends up wanting the daemon).

---

## D003 — Daemon-backed architecture, auto-spawning

**Status:** decided.

**Why:** Inherited from mxr. Multiple clients (TUI, agent, scripts) need shared session state. Daemon owns the DAP adapter process and current session. First subcommand that needs the daemon spawns it. PID file at `{data_dir}/daemon.pid`. Socket at `{runtime_dir}/lazydap-{instance}.sock`.

**Alternatives considered:** Stateless CLI (impossible for live debug session). Library that callers embed (forces every client to handle async DAP). Separate daemon binary the user runs explicitly (worse UX).

---

## D004 — Length-delimited JSON over Unix socket

**Status:** decided.

**Why:** Same as mxr's IPC choice. Easy to implement, easy to debug (open the socket with `socat`, read raw JSON). Format: `IpcMessage { id: u64, payload: IpcPayload }` framed by a length prefix. Mirrors LSP/DAP framing patterns.

**Alternatives considered:** gRPC (unnecessary complexity for local-only IPC), Cap'n Proto (compile-time wins not worth it here), bincode (debug ergonomics worse than JSON).

---

## D005 — Strict crate boundaries enforced by Cargo

**Status:** decided.

**Why:** Inherited from mxr. The dependency graph in `Cargo.toml` is the architecture. `tui` literally cannot depend on `daemon` or `store`, so it cannot bypass the IPC contract. Catches violations at build time, not in review. (See [`01-architecture.md`](01-architecture.md) for the full graph.)

---

## D006 — TOML state files, not SQLite

**Status:** decided.

**Why:** lazydap state is small (per-project: a list of breakpoints, a list of watches, a list of named launch configs). TOML is human-readable, version-controllable, scriptable from any language without a DB driver. SQLite would be overkill and would force every potential frontend to depend on a SQLite reader.

State files:

- `.lazydap/state.toml` per project — breakpoints, watches, launch configs (named)
- `~/.config/lazydap/config.toml` — global preferences

**Alternatives considered:** SQLite (mxr-style — overkill for this volume of state), JSON (less human-friendly), no persistent state (loses breakpoints across sessions, bad UX).

**Trade-off accepted:** TOML doesn't index well. If state grows past ~100 breakpoints per project the read cost matters. Cross that bridge when we get there.

---

## D007 — Multi-session designed-for-it now, enforced N=1 in v0.1

**Status:** decided.

**Why:** The user wants multi-session eventually but doesn't want it in v0.1. Compromise: every IPC message includes a `session_id` from M5 onward. The daemon enforces "one session at a time" but the protocol does not. Lifting the constraint later is a daemon-only change; clients keep working.

**What this looks like in v0.1:**

- All session-scoped requests carry `session_id`.
- Daemon rejects `Launch` if a session already exists, with `Error::SessionAlreadyActive`.
- TUI/CLI just hardcode the single active session ID.

**Alternatives considered:** Truly single-session-no-IDs (forces protocol break later). Full multi-session in v0.1 (real complexity — which session does `step` apply to? — out of scope).

---

## D008 — `.vscode/launch.json` supported from day 1

**Status:** decided.

**Why:** Most repos with non-trivial debug setups already have `.vscode/launch.json`. Inheriting it makes lazydap useful in any existing repo immediately. The format is a de-facto standard; DAP itself uses the same shape internally.

**Implementation:** `lazydap.config` crate parses `.vscode/launch.json` (with comments — VS Code's JSON-with-comments dialect). Treated as read-only. Project-local `.lazydap.toml` can reference launch configurations by name from `launch.json`.

**Alternatives considered:** Custom format only (worse UX for existing repos). Custom format with `launch.json` import command (extra step).

---

## D009 — Same `.skill` ZIP shape as mxr

**Status:** decided.

**Why:** Author already has agent tooling around this format. Reuse what works. `lazydap.skill` is a ZIP containing `SKILL.md` (concise quick reference) and `references/commands.md` (full subcommand reference).

---

## D010 — One daemon per project, keyed by repo root

**Status:** decided.

**Why:** Debugging is project-scoped. Cross-project breakpoints make no sense. Inherits mxr's `MXR_INSTANCE` pattern: `LAZYDAP_INSTANCE` env var or auto-detected from project root. Daemon socket path includes the instance: `{runtime_dir}/lazydap-{instance}.sock`.

**Project root detection:** walk up looking for `.lazydap/`, then `.git/`, then `Cargo.toml` / `package.json` / `pyproject.toml`. First match wins.

---

## D011 — `--wait` is the bridge from async to sync

**Status:** decided.

**Why:** Stepping/continue commands fire-and-forget by default (lazygit-style instant return). With `--wait`, they block until the next stable state (paused / exited / terminated / timeout) and return one JSON blob describing what happened. Agents always use `--wait`. TUIs can use either.

**Default timeout:** 30 seconds. Override via `--timeout=N` or `LAZYDAP_TIMEOUT` env var. `0` = infinite.

**Alternatives considered:** Always-blocking (bad TUI UX, async event flow gets blocked). Always-async with separate poll command (forces agents to poll, ugly).

**See [`10-async-to-sync.md`](10-async-to-sync.md) for full semantics.**

---

## D012 — Hand-rolled Elm Architecture for TUI state

**Status:** decided.

**Why:** Author is learning Rust + ratatui + DAP + tokio. Adding a TUI framework (Iocraft, tui-realm) on top of all that overflows the unknowns budget. Plain ratatui + a hand-written `(State, Msg) -> (State, Cmd)` reducer is ~50 lines of boilerplate, zero magic, full understanding.

**M10 is dedicated to this refactor.** Don't skip it.

**Alternatives considered:** Iocraft (React-style with proc macros — too much magic on top of a learning curve). tui-realm (Elm-style on ratatui — still adds another dependency). Plain ratatui with mutate-from-anywhere event handlers (collapses by month 3).

---

## D013 — Initial v0.1 adapter: codelldb only

**Status:** decided.

**Why:** Author is currently debugging C. codelldb covers C, C++, Rust — three of the most-debugged native languages. Other adapters wait until v0.1+ (debugpy → M18, then delve, js-debug).

**Alternatives considered:** Multi-adapter from start (each adapter has quirks; debugging two adapter-specific bugs in parallel is slower than fixing them in series).

---

## D014 — Tests use real adapters, not mocks

**Status:** decided.

**Why:** Inherited from mxr. A `FakeAdapter` exists (in-process, deterministic) for fast unit-style tests. Integration tests run real codelldb against tiny fixture binaries. Mocks of `DebugAdapter` are last-resort — they pass unit tests but miss real adapter quirks.

---

## D015 — `tracing` from the first line of `main`

**Status:** decided.

**Why:** Inherited from mxr. You cannot `println!` your way through a TUI. Structured logs to file in background mode, human-readable to stderr in foreground. Default file: `{data_dir}/lazydap.log`.

---

## D016 — License: MIT OR Apache-2.0

**Status:** proposed (user to confirm).

**Why:** Rust ecosystem convention. Same as mxr. Maximises downstream usability.

**Alternatives:** GPL (creates compatibility issues with Rust ecosystem), MIT-only (locks out Apache-preferring users), proprietary (defeats the "anyone can build a frontend" goal).

---

## D017 — Repository: `github.com/planetaryescape/lazydap`

**Status:** proposed (user to confirm).

**Why:** Same org as mxr. Discoverable next to it.

**Alternatives:** `github.com/{user}/lazydap` (less discoverable). Self-hosted (extra friction).

---

## D018 — `--wait` waits for paused OR exited OR terminated

**Status:** decided. (See research in [`10-async-to-sync.md`](10-async-to-sync.md).)

**Why:** Don't make agents poll after a program exits. The response includes a `state` discriminator: `"paused" | "exited" | "terminated" | "timeout" | "adapter_died"`.

---

## D019 — `--wait` returns intervening events in the response

**Status:** decided.

**Why:** During a `continue --wait`, the program may emit `output` events (its stdout/stderr), `breakpoint` events (state changed), `thread` events. Buffering these into the response means agents get the full picture in one call. mcp-dap-server discards them; we don't.

**Response shape includes:** `captured_output: [{category, output}]`, `breakpoint_updates: [...]`, `thread_updates: [...]`, `additional_stopped_threads: [tid]`.

---

## D020 — Coalesce additional `stopped` events for 50ms

**Status:** decided. (See [`10-async-to-sync.md`](10-async-to-sync.md).)

**Why:** Multi-threaded programs can fire multiple `stopped` events in rapid succession (one per thread). Returning on the first leaves the others invisible. Coalescing for 50ms after the first lets us include them as `additional_stopped_threads`.

**Default behaviour:** return on first stopped event. **`--all-threads` flag** waits for `allThreadsStopped: true`.

---

## D021 — One in-flight execution request per session (queue, don't pipeline)

**Status:** decided.

**Why:** ptvsd issue #1502 documents that some adapters serialize requests; pipelining can deadlock. Queue execution requests (continue, step, pause) per session. Non-execution requests (eval, setBreakpoint, scopes) can be parallel because they're typically synchronous.

---

## D022 — Synthetic `terminated` event when adapter process exits

**Status:** decided.

**Why:** Adapters die. VS Code issue #102037 documents UIs getting stuck when adapters never send `terminated`. lazydap detects adapter exit (SIGCHLD / process status) and emits a synthetic `terminated` event so all clients see it.

---

## D023 — AI features are external clients of the protocol

**Status:** decided.

**Why:** lazydap stays focused. AI advisors, MCP servers, autonomous bug-finders — all build on top of the protocol via two primitives:

1. **Event subscription API** (`Subscribe { channels }`) — already in the design.
2. **`getStateSnapshot` command** — returns rich JSON for one-shot context (frame, locals recursive depth-limited, recent step history, source slice, breakpoints, watches).

We ship those two. We don't ship AI features in core. (See [`12-ai-future.md`](12-ai-future.md).)

---

## Open decisions

These need user input.

### O01 — Default project root detection priority

**Question:** When detecting project root, what's the order? `.lazydap/` → `.git/` → language manifests (`Cargo.toml`, `package.json`, ...) → cwd?

**Why it matters:** If two `.git/` repos are nested (worktrees, submodules, monorepo), which wins?

### O02 — Should `lazydap doctor` write to the project's `.lazydap/`?

**Question:** Does the doctor command write diagnostics to a file the user might commit? Or always print to stdout?

### O03 — Adapter binary discovery

**Question:** How does lazydap find `codelldb`?

- (a) `which codelldb` — relies on PATH
- (b) Mason-style: `~/.local/share/lazydap/adapters/codelldb`
- (c) Per-project config: `[adapter.codelldb] command = "/path/to/codelldb"`
- (d) All of the above with priority order

mxr-style answer: (d) with priority — config > Mason-managed > PATH.

### O04 — `lazydap.skill` distribution

**Question:** Where does the `.skill` ZIP live? Bundled in the binary at compile time? Distributed alongside? Auto-extracted on first run?

mxr does it as a sibling ZIP in the repo root. Probably the same.

---

## Decisions to revisit at v0.1 → v0.2 boundary

- D013 (codelldb-only) → debugpy + js-debug + delve.
- D007 (single-session enforcement) → multi-session lift.
- D023 (AI external) → re-evaluate. May want to ship a thin `lazydap-mcp` server crate as an officially-maintained client.
