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

**Status:** decided (2026-07-30 — LICENSE-MIT and LICENSE-APACHE shipped at workspace setup; confirmed settled).

**Why:** Rust ecosystem convention. Same as mxr. Maximises downstream usability.

**Alternatives:** GPL (creates compatibility issues with Rust ecosystem), MIT-only (locks out Apache-preferring users), proprietary (defeats the "anyone can build a frontend" goal).

---

## D017 — Repository: `github.com/planetaryescape/lazydap`

**Status:** decided (2026-07-30 — repo is live and public at this address; confirmed settled).

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

## D024 — Project root detection walks one marker tier at a time (resolves O01)

**Status:** decided (2026-07-30, with M5).

**Why:** D010 keys one daemon per project, so "which project am I in?" has to have exactly one answer. The order is `.lazydap/`, then `.git/`, then a language manifest (`Cargo.toml`, `package.json`, `pyproject.toml`), then the working directory.

The load-bearing detail is that each tier is searched **all the way up before the next one is tried**, rather than taking the first marker of any kind found in the nearest directory. That is what makes `.lazydap/` usable as a deliberate override: in a monorepo or a submodule, a `.lazydap/` further up beats a `.git/` right here. Nesting between markers of the *same* tier resolves to the nearest, which is what people expect of nested repositories.

**Implementation:** `crates/config/src/paths.rs`. `LAZYDAP_INSTANCE` and `--instance` both override the whole business.

**Alternatives considered:** first-marker-wins per directory (makes `.lazydap/` useless as an override — a nearer `.git/` always shadows it); `.git/` only (breaks non-git projects and every worktree).

---

## D025 — `lazydap doctor` only ever writes to stdout (resolves O02)

**Status:** decided (2026-07-30). **Lands at M6**, recorded now because it was answered alongside O01/O03/O04.

**Why:** Diagnostics are for reading and for piping, not for committing. A command that drops a file into the project is a command that eventually gets that file reviewed, stale, and arguing with reality. `lazydap doctor --format json > report.json` covers every case where somebody genuinely wants a file, and puts them in charge of where it goes.

---

## D026 — Adapter discovery: config, then managed directory, then PATH (resolves O03)

**Status:** decided (2026-07-30). **PATH lookup implemented at M5**; the earlier tiers land with the config loader (M15) and the second adapter (M18).

**Why:** Option (d) from O03, matching mxr. Priority order:

1. Per-project or global config — `[adapter.codelldb] command = "/path/to/codelldb"`. Explicit beats implicit, and pinning a specific build is the whole reason someone reaches for this.
2. A lazydap-managed directory (`{data_dir}/adapters/codelldb`), for adapters lazydap installed itself.
3. `PATH`.

**What M5 ships:** step 3 only, in `crates/daemon/src/adapter/mod.rs::discover`. The other two tiers need config loading that does not exist yet; adding empty lookups now would be dead code dressed up as policy. The failure carries `ErrorCode::AdapterNotFound` and lists the directories it searched, so the message stays useful when the earlier tiers arrive.

---

## D027 — `lazydap.skill` ships as a sibling ZIP (resolves O04)

**Status:** decided (2026-07-30). **Lands at M7.**

**Why:** Same as mxr (D009), for the same reason: the author's agent tooling already understands that shape. A ZIP next to the binary can be updated without rebuilding, inspected without extracting, and versioned in the repo. Embedding it in the binary would mean a recompile to fix a typo in a doc; auto-extracting on first run would mean writing to the user's disk uninvited.

---

## D028 — IPC framing uses `tokio_util`'s `LengthDelimitedCodec`

**Status:** decided (2026-07-30, with M5).

**Why:** D004 settled the wire format — a 4-byte big-endian length then that many bytes of JSON. This decides who implements it: `tokio_util::codec::LengthDelimitedCodec` wrapped in a serde_json encoder/decoder pair (`crates/protocol/src/codec.rs`), which is the proven shape from mxr's `crates/protocol/src/codec.rs`.

Hand-rolling `read_exact` on a 4-byte header is about fifteen lines and gets the easy cases right. It gets the hard ones wrong: partial reads, a frame split across two `read` calls, and — the expensive one — an attacker-or-bug-supplied length prefix that makes the daemon allocate gigabytes. `LengthDelimitedCodec` has a `max_frame_length` (16 MiB here) and correct partial-frame handling for free.

**Consequences:** `tokio-util` and `bytes` join the dependency budget. Malformed JSON surfaces as `io::ErrorKind::InvalidData` so the daemon can answer `BadRequest` before hanging up, rather than dropping the connection silently.

**Note:** the connection wrapper (`IpcConnection`) drives the codec by hand instead of using `tokio_util::codec::Framed`, because `Framed` needs the `futures` `SinkExt`/`StreamExt` traits and a whole dependency for two method calls is a poor trade.

---

## D029 — The adapter seam is a module boundary, not a `DebugAdapter` trait (yet)

**Status:** decided (2026-07-30, with M5). **Revisit at M18.**

**Why:** `ARCHITECTURE.md` requires that DAP details never leak past the adapter layer, and names a `DebugAdapter` trait as the mechanism. M5 implements the requirement without the trait.

v0.1 ships one adapter (D013). A trait with a single implementor does not abstract anything — it adds `async_trait` or boxed futures, a `dyn` indirection, and an interface designed against exactly one example, which is the reliable way to design the wrong one. Worse, it *looks* like the seam while the real seam goes unchecked.

**What we do instead:** `crates/daemon/src/adapter/` is the only module in the daemon that may name a `lazydap_dap` type. Everything outside it — `state`, `server`, `handlers`, `commands` — works in `lazydap_core` and `lazydap_protocol` vocabulary. The boundary is one `grep` away from being checkable, and the module already exposes the shape a trait would need: `launch`, a handle with `disconnect`/`kill`, and a pump that translates DAP events into `lazydap_protocol::Event`.

**Trigger to revisit:** M18, when debugpy gives us a second implementor and therefore a real basis for the interface. At that point this module becomes the trait plus `adapter-codelldb`, and `crates/adapter-*` appears in the boundary script.

**Alternatives considered:** the trait in `lazydap-core` now (core would have to name adapter concepts it otherwise knows nothing about, and the trait would be written blind); no seam at all (DAP types spread through the daemon, which is the anti-pattern that paid for this rule).

---

## D030 — `SessionId` is a UUID v4

**Status:** decided (2026-07-30, with M5).

**Why:** The blueprint's examples show ULID-shaped ids (`01ABC...`), which are attractive because they sort by creation time. Nothing in lazydap sorts session ids: there is one live session (D007), and the daemon holds them in a map. Sortability would buy nothing, and the `ulid` crate would be a dependency bought for aesthetics.

`uuid` is already in the budget, universally understood, and parses from a string on every platform a client might be written for.

**Consequences:** ids look like `02ef9b0b-7288-4f0b-89b5-de539f8d2e29` rather than `01ABC...`. `SessionId` is a newtype in `lazydap-core` serialising transparently as a string, so the wire format does not care what is inside it.

---

## Open decisions

These need user input.

*(O01–O04 were answered on 2026-07-30 and became D024, D025, D026 and D027.)*

None outstanding. One question is parked for M15: whether to publish crates to crates.io. Default is no — `publish = false` stays, matching mxr.

---

## Decisions to revisit at v0.1 → v0.2 boundary

- D013 (codelldb-only) → debugpy + js-debug + delve.
- D007 (single-session enforcement) → multi-session lift.
- D023 (AI external) → re-evaluate. May want to ship a thin `lazydap-mcp` server crate as an officially-maintained client.
