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

## D031 — `BreakpointId` is a small integer, not a UUID

**Status:** decided (2026-07-30, with M6).

**Why:** The blueprint's examples show `bp-01ABC...`, and AGENTS.md's agent-loop example shows `"breakpoint_id": 1`. They cannot both be right, and the integer wins on every use it actually gets: a person types it (`lazydap break --remove --id 3`), a pipeline pipes it (`break --list --format ids | xargs`), and a human reads it out of `.lazydap/state.toml`. A UUID is worse at all three and better at nothing lazydap does — there is no distributed allocation to avoid colliding with.

The counter is persisted alongside the breakpoints and never goes backwards, including past an id added by hand-editing the file. Ids are therefore not reused after a removal: a script holding a stale id gets `not_found` rather than silently hitting a different breakpoint.

**Consequences:** `BreakpointId` is a `u32` newtype serialising as a bare number. Adapter ids are a separate namespace and never leak — the session maps between them, and a `hitBreakpointIds` entry we cannot map is dropped rather than passed through as a number from the wrong namespace.

---

## D032 — Protocol version 2 at M6

**Status:** decided (2026-07-30, with M6).

**Why:** M6 adds the stepping, inspection and breakpoint requests. A v1 daemon cannot decode any of them. Left at v1, an M6 client talking to a still-running M5 daemon would get `BadRequest` for `continue` — a command that plainly exists — and no path to fixing it. At v2 the same situation is a `VersionMismatch`, which the client already knows how to resolve: stop the old daemon (`Shutdown` is version-exempt) and start its own.

Bumping the version is what D004 requires an entry for. Nothing has shipped, so no external client is broken by it.

**Consequences:** `ErrorCode` gains `SessionNotPaused`, which `docs/blueprint/10-async-to-sync.md` had always specified and M5 had no requests to need. `Request::Shutdown` and `Request::Disconnect` gain `dry_run`, and `Response::Disconnected`/`ShuttingDown` gain the fields to report it.

---

## D033 — codelldb's entry stop is reported as `entry`, with the raw reason kept

**Status:** decided (2026-07-30, with M6). Resolves the M5 follow-up and option 3 of quirk 6.

**Why:** codelldb implements `--stop-on-entry` by letting the process start and sending it `SIGSTOP`; LLDB classifies a signal stop as an exception, so a launch that did exactly what was asked reports `"reason": "exception"` ([quirk 6](../reference/codelldb-quirks.md#6---stop-on-entry-reports-reason-exception-not-entry-on-macos)). An agent reading that concludes the program crashed before `main` — the single most expensive kind of wrong answer, because it looks like a real finding.

"JSON output is a product feature" (non-negotiable #3) settles it: `reason` is lazydap's vocabulary, and the adapter's idiosyncrasy belongs below the seam (non-negotiable #5). So the **first** stop of a launch that asked for `stop_on_entry`, and only that one, is reported as `entry` when the raw reason is exception-class and the description names `SIGSTOP`.

The normalisation is visible rather than silent: `raw_reason` carries the adapter's own word, and is absent when the two agree. That is option 3 from the quirk, chosen over option 2 (map it quietly — a lie by omission) and option 1 (leave it — every agent has to learn that "exception" sometimes means "entry").

**Consequences:** the guard is deliberately narrow. A real exception at the entry point does not carry `SIGSTOP` and passes through; a `SIGSTOP` nobody asked for passes through; every later stop passes through. The normalisation lives in `crates/daemon/src/adapter/codelldb.rs`, not in the pump — by the time the pump is running, the adapter's reasons are its own.

---

## D034 — `lazydap eval` defaults to the `watch` context, not `repl`

**Status:** decided (2026-07-30, with M6).

**Why:** DAP's `evaluate` takes a `context`, and codelldb reads `repl` literally — "a line typed at the debug console" — handing it to LLDB's *command* interpreter rather than its expression evaluator. `lazydap eval "x"` on a program with an `int x = 5` therefore failed with *"memory read takes a start address expression"*, because `x` is LLDB's alias for `memory read` ([quirk 7](../reference/codelldb-quirks.md#7-evaluate-with-context-repl-runs-an-lldb-command-not-an-expression)).

`lazydap eval "x + y"` is asking about the program, not driving the debugger. `watch` and `hover` both evaluate the string as an expression; `watch` is the default, and `EvalContext::default()` matches so the protocol and the CLI cannot disagree.

**Alternatives considered:** prefixing `repl` expressions with `?` as codelldb's console suggests — codelldb-specific syntax leaking into what a caller types, and `lazydap eval` would mean something different per adapter. Keeping `repl` as the default and documenting the trap — a documented footgun is still a footgun, and this one costs an agent a turn every time.

**Consequences:** `--context repl` is still there and still means "run an adapter command", which for codelldb is a real feature rather than a mistake.

---

## D035 — `commands.md` is generated by an example, not a second binary

**Status:** decided (2026-07-30, with M7).

**Why:** The skill's command reference has to be generated or it drifts (M7's own note). The question was what generates it.

It walks the real `Cli` type via `clap`'s `CommandFactory`, rather than parsing `--help` output: help text is a rendering, and a parser for it would be a second, worse model of something clap already has — quietly losing whatever it did not understand.

It lives in `crates/daemon/examples/gen_skill_commands.rs` rather than a second `[[bin]]`, because a binary in the product crate is installed onto users' machines by `cargo install` for no reason. Examples are still compiled by `cargo check --all-targets`, so it cannot rot silently, and `cargo run --example` is the same one-line invocation a build script would need.

**Consequences:** `scripts/build-skill.sh` regenerates the reference and packs the ZIP; CI runs it and fails if the committed artefacts differ. The ZIP is built reproducibly — fixed entry timestamps, sorted entries — because a committed binary artefact that changes on every build is one nobody can review and everybody re-commits by accident.

---

## D036 — every mutation is dry-runnable, including the ones that only tear down

**Status:** decided (2026-07-30, with M6).

**Why:** Non-negotiable #4 requires `--dry-run` on mutations, using the same selection logic as the real thing. M6 settles what that means per command:

- **`break --remove` / `--toggle`** — the preview and the mutation both call `ProjectStore::select` with the same selector. Not "the same logic": the same function. A toggle preview shows the state it *would* leave behind, since echoing the current state answers a question nobody asked.
- **`break` (add)** — has no selector, so its preview answers the question a caller actually has: is this new, or do I already have one there? It reports the existing breakpoint when there is one.
- **`disconnect` / `shutdown`** — dry-runnable. They destroy something (a session, every session), and "what would this take down?" is a fair question to be able to ask first.
- **`launch`** — deliberately none. It creates rather than destroys, and the honest preview of a launch is a launch. A `--dry-run` that reported "would start codelldb" without finding out whether the program is debuggable would be a prediction, not a preview, and the failure it would fail to predict is exactly the one worth knowing about. `lazydap doctor` covers "is this set up correctly" without pretending to be a launch.

**Consequences:** `--yes` is not implemented. The blueprint pairs it with `--dry-run`, but it exists to skip a confirmation prompt, and lazydap has no prompts — a flag that skipped nothing would be a promise to add one. It lands with the first command that actually asks.

---

## D037 — the daemon crate may depend on the TUI crate; the arrow never points back

**Status:** decided (2026-07-30, with M8).

**Why:** `ARCHITECTURE.md` said "`daemon` depends on everything except `tui`", which cannot be true while D002 holds: there is one binary, `lazydap`, and it is built from `crates/daemon`, so something in that crate has to be able to call `lazydap_tui::run`. The alternative is a second binary crate whose only job is to depend on both, which buys nothing.

What the rule was protecting is the *other* direction, and that is untouched: `lazydap-tui` may depend on `core`, `protocol` and `config` and nothing else. With no path to the daemon, the store or DAP, a TUI-only feature is not something that can be written — it has to become a protocol request, and a protocol request is one the CLI can send too. That is non-negotiable #2 enforced by Cargo rather than by review.

**Implementation:** both rows are in `scripts/check_architecture_boundaries.sh`, which CI runs. Anything a client needs that requires a *process* — spawning a daemon, resolving an instance — happens in `crates/daemon/src/commands/tui.rs` and is handed to the TUI as data (a socket path).

---

## D038 — `Subscribe` is answered with a state snapshot, and replays nothing

**Status:** decided (2026-07-30, with M11).

**Why:** A long-lived client needs two things at startup: what the state is now, and what changes from here. Asking them as two questions leaves a window between the answers, and an event that lands in that window is either lost (snapshot first, subscribe second) or double-counted (the other way round). Answering `Subscribe` with `Response::Status` closes it: the snapshot is taken at the moment the stream is attached, under the same call.

It also avoids a new `Response` variant. Two builds both claiming protocol v2 must agree on the shape of every frame, and `Subscribe` previously answered `Unsupported` — so a new variant would have been a wire change without a version to signal it. Reusing `Status` keeps v2 honest (no bump; see D032).

**Nothing buffered is replayed.** Three reasons: the snapshot already accounts for the state those events produced; replaying a `Stopped` would send a TUI chasing a position the program left long ago; and the buffer's delivery watermark belongs to `--wait` (M6), so a subscriber consuming it would silently steal events from the next `continue --wait`. Debuggee output produced before the subscription is still readable through `Request::Output`, which reads without draining — and unlike an event stream, that is a request the CLI makes too.

**Consequences:** subscribing again replaces the set of kinds rather than adding to it, so a client can narrow what it watches without reconnecting. A subscriber that falls behind the broadcast loses the oldest events (the channel is capacity-bounded, drop-oldest) and the daemon logs it; the TUI resynchronises at the next stop, because a stop is followed by a stack fetch that asks for the truth rather than reconstructing it.

---

## D039 — the TUI is verified in a real pseudo-terminal, not only against a test backend

**Status:** decided (2026-07-30, with M8–M11).

**Why:** ratatui's `TestBackend` renders into a buffer, which is exactly right for "does this state draw the marker on line 19" and useless for "does `q` give the terminal back". Both matter, and the second is the one that ruins somebody's shell. So the TUI has two kinds of test:

- **`TestBackend` snapshots** for what is on screen, comparing symbols only — colour and emphasis are real decisions but not ones a string comparison judges usefully, and a test that breaks whenever a border changes shade gets deleted rather than fixed.
- **A pseudo-terminal drive** for the things only a real terminal has: entering and leaving the alternate screen, raw mode, exit codes, and the cross-process scenario where a `lazydap continue` typed in another shell moves the marker in a running TUI.

The PTY driver is a throwaway script rather than part of the suite (it needs a tty, a built binary and a live codelldb), so its output is pasted into the milestone notes as evidence. What *is* in the suite is the headless half: the reducer, exhaustively, and the IPC client against a real Unix socket.

**Consequences:** this caught a bug that no unit test could have. The input pump runs on `spawn_blocking`, which cannot be aborted and which the runtime *waits for* at shutdown — so `q` left the process alive until somebody pressed another key. It showed up as an exit code of 255 in a PTY run and nowhere else.

---

## D040 — the TUI's reducer numbers its own requests, and drops answers that have been overtaken

**Status:** decided (2026-07-30, with M12–M13).

**Why:** Until Phase D the TUI could get away with not correlating a reply to the request that caused it. `ipc_client` said so in as many words: a `Response` says what it is, the daemon answers one request at a time in order, so the last answer of a given kind is the freshest. The scopes pane breaks that in two places.

`Response::Variables` is a bare `Vec<Variable>`. Nothing in it says which node was being expanded — not even the `variables_reference` that was asked for. With two expansions in flight there is no way to tell which is which, and the pane would fill the wrong row.

The second is sharper and applies to the stack too. A stack trace names **frame ids**, and the adapter only keeps those valid until the program moves. An answer for the stop before last is not merely out of date: every id in it addresses nothing, so the pane would populate, look right, and fail on the next expansion. "The last answer wins" is exactly wrong here — the last answer to *arrive* may be the older one.

So `Cmd::SendIpc` carries an `id` chosen by [`AppState::next_request_id`], and the write pump sends that rather than one of its own. Ids are monotonic because every request goes through one function, which is what makes "this has been overtaken" decidable at all.

**How staleness is decided.** One rule per kind of answer, keyed on the id:

- `latest_stack` and `latest_scopes` hold the newest request of each kind. An answer whose id is not the latest is logged and dropped.
- `pending_variables` maps a request id to the *index path* of the node that asked **and the generation of the tree that path was resolved against**. Review found the first version clearing the map when a new `Scopes` was *requested*, which cannot work: an expansion pressed in the gap before the answer arrives is inserted after the clear, and lands in a tree it was never about — the caller's node filled with the callee's values, at the right position, with nothing on screen to say so. The generation is the tree's own, not the newest request's, for the same reason: between asking for a frame's scopes and being given them, what is on screen is still the previous frame's.
- **The panes are marked stale the moment a stop is reported**, not merely refreshed when its answer lands. Between the two, every frame id and `variables_reference` on screen belongs to a frame the adapter has discarded, and acting on one sent a dead handle *and* superseded the legitimate request the new stop had just made.
- `pending_breakpoints` holds ids for the mutations `b` sends. It is not about staleness — breakpoints outlive stops — but about failure: a refused mutation leaves the gutter showing an intention the daemon did not carry out, and nothing in the error says which way it went, so the answer is to ask for the whole list again.

This is the same discipline M11 already used for file reads (`latest_load`), generalised. The reserved-id floor (`RESERVED_IDS`) keeps the reducer's numbering clear of the handshake's, so a `Pong` can never be mistaken for an answer to something the reducer asked.

**Consequences:** `IpcClient::send` takes an id. `Msg::Connected` exists so that *initialisation* is a reducer decision rather than something the loop hard-codes — which is what lets M19's reconnection replay the opening moves (`Subscribe` + `BreakpointList`) instead of keeping a second copy of them.

---

## D041 — `Cmd::Batch`, sequential and dumb

**Status:** decided (2026-07-30, with M12).

**Why:** One message can genuinely need two things done. A stop needs the stack *and* the scopes; `<CR>` on a stack frame needs the frame's file *and* its variables. Before Phase D every `Msg` produced at most one `Cmd`, and the two ways out of that were both worse than a list: a `Cmd` variant per pair (which grows quadratically and puts the pairing in the wrong place), or asking for the second thing only after the first answer arrives (which adds a round trip to every step of every debug session).

`Cmd::Batch(Vec<Cmd>)` is deliberately the least clever option available. The loop runs the commands in order, one after another, and that is all it does — no parallelism, no dependency between elements, no result threading. Anything a batch could express that a `Vec` cannot is something that belongs in the reducer, where it is testable.

Asking for two things at once does not pipeline anything at the adapter (non-negotiable 6): the daemon queues requests to one adapter regardless, so this shortens the TUI's latency without changing what the adapter sees.

**Consequences:** a batch of one is never constructed — the reducer returns the bare `Cmd` instead — so a test that asserts on a single request does not have to know whether it happens to be wrapped. M16's task file already anticipated this; watches will use it.

---

## D042 — the TUI reconnects by calling back into the CLI, not by learning to spawn

**Status:** decided (2026-07-30, with M19).

**Why:** A TUI left open across `lazydap shutdown` — or across a daemon crash — used to be a dead screen with no way back. Reconnecting needs two things the TUI cannot do: start a daemon process, and take the spawn lock that stops two clients racing to start two.

Neither can move into `lazydap-tui`. It may depend on `core`, `protocol` and `config` and nothing else (D037), and that boundary is the thing making non-negotiable #2 true. So `run` takes an `EnsureDaemon` callback and `crates/daemon/src/commands/tui.rs` supplies one that calls the same `ensure_daemon_running` every subcommand takes (D003) — spawn lock, stale-socket removal, version-mismatch replacement and all. A TUI reviving a daemon does it by exactly the path a CLI command would.

**What the reducer owns.** The retry curve, so it is testable without waiting for it: 250 ms doubling to a 4 s ceiling. It does **not** give up — see D044, which corrects the six-attempt limit this shipped with. The delay is a `Cmd::Reconnect { attempt, delay_ms }` the loop sleeps on *in a task*, never inline: four seconds of a blocked loop is four seconds in which `q` does not work.

**How the screen becomes true again.** Nothing is reconstructed. A reconnection replays the opening moves of the first connection (`Msg::Connected` → `Subscribe` + `BreakpointList`), and the `Subscribe` reply is a state snapshot taken at the moment the stream attaches (D038). A session started from another terminal while the daemon was down is therefore picked up rather than waited for.

**What survives and what does not.** Everything about the session goes the moment the daemon does — marker, stack, scopes, pending fetches — because none of it can be checked any more. The breakpoints stay: they are the project's, recorded in `.lazydap/state.toml`, and clearing the gutter would suggest they had been lost when they had not. The `BreakpointList` that follows the reconnection refreshes their verification state, which correctly drops back to unverified when the new daemon has no session.

---

## D043 — a breakpoint change is either an adapter's opinion or the project's, and the event says which

**Status:** decided (2026-07-30, review round after M12–M14/M19). **Protocol v2 → v3.**

**Why:** `lazydap break` with nothing running persisted the breakpoint and announced nothing, so an open TUI's gutter went on drawing the previous set indefinitely. That is M14's "the gutter and `break --list` agree" criterion failing in the direction nobody checked. It is not only a between-sessions problem either: an adapter is handed the new list for a source file and says nothing whatever about what is no longer in it, so a *removal* is invisible to a client watching adapter events even with a session live.

The fix is for every breakpoint mutation to announce itself. The question was what to announce it as, and reusing `BreakpointUpdated` unchanged would have been a lie: that event carries an `AdapterBreakpoint` — `verified`, the line the adapter moved it to — and a `lazydap break` between sessions has no adapter and therefore no opinion. A client applying those fields would be inventing a claim nobody made, and marking a breakpoint verified on the strength of a program that is not running.

So `session_id` became `Option<SessionId>` and the two scopes are distinguished by it:

- **`Some(id)`** — that session's adapter changed its mind. The payload is its opinion, true only while it lives.
- **`None`** — the project's list changed. The payload names *which* breakpoint, and nothing more; the verification fields carry no information. What it means to a client is "read the list again".

The TUI does exactly that: a project-scope update produces a `BreakpointList` rather than a guess. One extra round trip on a human-paced action, and it is correct for adds, removals and toggles alike without the event having to express any of them.

**Why a version bump.** Two builds both claiming v2 would fail to decode each other's events, which is the exact hazard the version exists to turn into a clean restart (D032, and the same argument D038 made for not adding a variant silently). `ensure_daemon_running` already replaces a daemon whose version differs, so the cost is one automatic restart.

**Consequences:** `Event::session_id()` returns `Option<SessionId>`. A `--wait` already ignores events belonging to another session, so a project-scope one is correctly not part of any wait's blob; it is broadcast but never buffered, because the buffer is a session's history and this belongs to no session.

---

## D044 — a reconnecting TUI never gives up, and every attempt is identified

**Status:** decided (2026-07-30, review round after M19). Supersedes the give-up rule as first built.

**Why:** M19 shipped with six attempts and a terminal `Lost` state. Six failures take under ten seconds; a daemon that became startable fifteen seconds later was never reached, on a screen the user was still sitting in front of, with no way back but quitting. The mistake was modelling this as a network reconnect. It is not one — every attempt runs `ensure_daemon_running`, which *starts* a daemon rather than waiting for one, so "cannot reach it" is never a settled fact about the world. The machine it would run on is the one the TUI is already on.

So the ladder runs for as long as the TUI is open: 250 ms doubling to a 4 s ceiling, then 4 s forever. The ceiling is what makes that affordable — retrying every four seconds costs nothing and bounds how long the user waits once things recover. `Connection::Lost` is gone; there is nothing for it to mean.

**Attempts are numbered**, and that is the other half. Without an identity on each one:

- a reply from an attempt that had already been superseded was taken for the current one, and started a second ladder alongside it;
- a `DaemonGone` arriving while an attempt was in flight — which a daemon dying just after a handshake produces — started a second ladder outright;
- a connection handed back by whichever attempt lost the race replaced a working connection with an unsubscribed one, after which every request went somewhere nobody was listening.

`Cmd::Reconnect` and `Msg::Reconnected` both carry the attempt. The reducer ignores an answer that is not the one it is waiting on, and refuses to start a second ladder while one is climbing. The loop checks the same thing before installing a connection, because *installing* is its decision and the currency test is one line of state either can read.

---

## D045 — a debuggee we launched dies with its debugger, even when the debugger is killed

**Status:** decided (2026-07-30, review round after M12–M14/M19).

**Why:** codelldb spawns the debuggee as its own child and reaps it on a clean shutdown. On an unclean one — a crash, an OOM kill, a `kill -9` — it never gets the chance: the debuggee is reparented to init and keeps running, with nothing left in the system that knows it is a debuggee. The daemon's adapter-death path (pump EOF → synthesise `AdapterDied` → kill the adapter) killed the adapter process it owns and stopped there, so the debuggee was nobody's problem.

Found by counting, not by reading: 46 orphaned test fixtures had accumulated across worktrees, one per run of the suite that SIGKILLs an adapter mid-wait. The test was only the reproduction. The same thing happens to a real user's program whenever codelldb crashes — a program stopped at a breakpoint stays suspended forever, and one that was running busy-loops forever.

**The daemon now records the debuggee's pid and kills it if the adapter dies without stopping it.** Three details that are not obvious:

- **The pid is scraped from console output, not taken from an event.** DAP defines a `process` event carrying `systemProcessId` and that would be the right source. codelldb does not send it: the string does not appear anywhere in its binary, and a full launch-to-exit stream contains `output`, `initialized`, `module`, `continued`, `exited` and `terminated` and nothing else. What it does print is `Launched process 1234 from '/path'`, so that is where the pid comes from (quirk 9). This is best-effort by design — a parse that fails leaves things exactly as they were, and says so in the log.
- **The line arrives during the handshake, not on the pump.** The launch's own event loop owns the transport until the session is live, so the pump never sees it — a first attempt hooked the pump's `output` handling and never fired once. The pid is read out of the launch outcome instead.
- **Identity is checked before anything is killed.** A daemon can outlive many programs, and a recycled pid belongs to a stranger. The recorded pid is only killed when `ps` still reports the program we launched at it; otherwise it is logged and left alone. Leaking the process we were looking for is much better than killing one we were not.

**Only for programs we launched.** When `attach` lands it must not record a pid: the point of attaching is that the process was somebody else's first, and killing it because our adapter crashed would destroy something we were only ever looking at. The record is set on the launch path alone, and both the field and the call site say so.

**Consequences:** the synthesised `AdapterDied` ending now says what became of the program, so a user whose adapter crashed is told whether their debuggee went with it. The codelldb suite asserts in teardown that no fixture outlived its session, scoped to the running build's fixture directory so parallel worktrees do not fail on each other's processes — a leak now fails the test that caused it instead of accumulating silently across five waves of review.

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
