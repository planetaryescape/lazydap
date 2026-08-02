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

**Resolved at M18 — see D052.** The trait was written and the interface it landed on is narrower than this entry expected: the launch, and nothing after it. The second half of the trigger did not happen and should not — the adapters stayed *modules* inside `crates/daemon/src/adapter/` rather than becoming `crates/adapter-*`, because separate crates would buy nothing the module boundary and its `grep` do not already buy, and would spread `lazydap_dap` across more manifests to do it.

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

**Amended by D064 (2026-08-01).** That last clause was wrong, and cost a release's worth of `lazydap pause` reporting a crash. An adapter's reasons stop being its own the moment lazydap asks it for something, and `pause` is the second thing codelldb implements with a `SIGSTOP`. `normalise_stop` is now called from the pump as well, against the execution request the program has not answered yet.

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

## D046 — the JSONC dialect is read by a hand-rolled scanner, not a crate

**Status:** decided (2026-07-30, with M15).

**Why:** `.vscode/launch.json` is JSON with `//` and `/* */` comments and trailing commas, and `serde_json` reads none of that. The alternatives were `json5`, `jsonc-parser` or `serde_jsonrc`; the cost is roughly a hundred lines of scanner against a dependency in a budget the project keeps deliberately small (AGENTS.md). The scanner is in `crates/config/src/launch_json.rs::strip_jsonc` and turns JSONC into JSON, which `serde_json` then parses — so the *parser* is still a real one, and only the dialect is handled here.

**What makes it correct rather than a regex:** it is string-aware. `"https://example.com"` is not a comment and `{"sep": ","}` is not a trailing comma, and both are the bug every naive stripper has. Comments become the whitespace they occupied — their newlines are kept — so a `serde_json` error still points at the line the reader is looking at in their editor.

**What it deliberately does not do:** single quotes, unquoted keys, hex numbers, or anything else JSON5 adds. VS Code does not accept them either, so a file using them was never going to work in the editor that owns the format.

**Revisit if:** a second file format needs the same treatment, or the scanner grows a third special case. One more and the dependency is the cheaper side.

---

## D047 — launch configurations are read by the client, and `run` sends an ordinary `Launch`

**Status:** decided (2026-07-30, with M15).

**Why:** both files are found by walking up from the working directory. The daemon's working directory is wherever it happened to be started (D024's detection runs in whoever is asking), so a daemon reading `.vscode/launch.json` would read a different project's, or none. Every other path in lazydap is resolved client-side for exactly this reason — see the module comment on `crates/daemon/src/commands/mod.rs`.

**What follows from it:** `lazydap launches run <name>` resolves the configuration, then sends the same `Request::Launch` that `lazydap launch` sends. No new protocol request, no protocol version bump, and nothing about launch configurations enters the daemon — which also means the TUI could grow a configuration picker later without either side learning a new message.

**The precedence rule:** `.lazydap/state.toml` beats `.vscode/launch.json` when both name a configuration the same, with a warning naming it. lazydap's own file is the one somebody chose to write for lazydap; picking silently between two things with one name is how you debug the wrong binary.

**Scope taken, and not:** state.toml's `[[launch_configs]]` are **read-only**. Nothing writes them — there is no `launches add` — and they are read out of the store's `unknown` table rather than modelled as a field, because a typed field serialising as empty would delete a hand-written configuration the first time somebody set a breakpoint.

---

## D048 — an unbound breakpoint is re-sent under the path the adapter names (resolves quirk 8)

**Status:** decided (2026-07-30, with M15). Implemented in `crates/daemon/src/adapter/mod.rs::rebind_source`.

**Why:** three components disagreed about a file's name and the user paid for it. lazydap canonicalises source paths so the daemon and the adapter agree regardless of anyone's working directory; a compiler records the path as it was typed on its command line; codelldb compares the two as strings. Under a symlinked directory — `/tmp` on macOS is `/private/tmp` — every breakpoint in the program silently fails to bind, and the only signal is a `verified: false` inside a message that reads like a success.

lazydap has both spellings at the moment it matters. When a `setBreakpoints` response reports nothing bound and names a location it could have used, that file's breakpoints are sent again under that name, and the second answer is final.

**What bounds it:**

- **One retry per source**, tracked per launch. The second answer is taken however it reads, so two components disagreeing about a path cannot loop.
- **Only when nothing in the file bound.** A partial rebind would leave the adapter holding two live breakpoints for one of ours, on one line.
- **Only when the suggestion resolves to the same file**, checked through the filesystem rather than compared as text. An adapter naming a path that resolves elsewhere is offering to break in code the caller never asked about.
- **The stored path does not change.** Only the spelling on the wire does, so breakpoint ids, `break --list` and `.lazydap/state.toml` are untouched.

**Alternatives considered:** *stop canonicalising* — loses the reason canonicalisation was added (a typo becomes a silent `verified: false` twenty minutes later, and the daemon's cwd stops mattering). *Send both spellings* — two `setBreakpoints` for one file, and a program that stops twice when both happen to bind. *Leave it documented* — what M15 inherited; the workaround is a directory move, which is fine for a person and invisible to an agent that just wrote a scratch program to `/tmp`.

---

## D049 — the config file is looked for in `~/.config` first, not in the platform's config directory

**Status:** decided (2026-07-30, review round after M15). Supersedes the path list in [`08-state-and-config.md`](08-state-and-config.md).

**Why:** `dirs::config_dir()` returns `~/Library/Application Support` on macOS. That is right for an application bundle and wrong for a command-line tool: `git`, `ripgrep`, `starship`, `nvim` and everything else a terminal user has configured live in `~/.config`, this project's own README and blueprint both say `~/.config/lazydap/config.toml`, and a user who follows those instructions on a Mac would have written a file lazydap never reads. A config that is silently ignored is worse than one that does not exist — nothing is wrong until the pinned adapter quietly is not used.

**The order**, first that **exists** winning:

1. `LAZYDAP_CONFIG_PATH` — and if it names a file that is not there, that is an error, not a fall-through. The user said where it was.
2. `$XDG_CONFIG_HOME/lazydap/config.toml`
3. `~/.config/lazydap/config.toml`
4. `dirs::config_dir()/lazydap/config.toml` — last, so a file an earlier build wrote to `~/Library/Application Support` keeps working rather than being orphaned by this change.

When none exists, the **first** candidate is what `lazydap doctor` prints as the place to create one. The list is deduplicated with its order preserved, because on Linux all three usually name the same directory and a `doctor` that printed it three times would look broken.

**Consequence:** the macOS location is read but never recommended. If both exist, XDG and `~/.config` win — deliberately: the one a user hand-wrote from the docs beats the one a library chose for them.

---

## D050 — the client resolves the adapter binary and sends it; protocol goes to v4

**Status:** decided (2026-07-30, review round after M15). **Protocol v3 → v4.**

**Why:** adapter discovery reads the user's config file and `PATH`. Both describe the machine *as the person typing the command sees it*. The daemon sees neither — it may have been started days earlier, from another directory, with another environment — so `LAZYDAP_CONFIG_PATH=/tmp/pinned.toml lazydap launch ./app` read the pin in the client, and then the daemon resolved the adapter again against its own default config path and fell through to `PATH`. The pin was obeyed by the process that could not act on it and ignored by the process that could, silently. This is the same failure D047 names for `launch.json` and D024 for the project root, arrived at from the other direction.

**What changes:** `LaunchRequest` carries `adapter_command: Option<PathBuf>`, resolved client-side by `adapter::discover_with` against the config the client already loaded. The daemon uses it when present and falls back to its own lookup when absent. Discovery failing is now reported *before* a daemon is spawned, which is also a better error.

**Why a version bump for an optional field.** The field is additive and `serde` ignores unknown ones, so an older daemon would accept the request and quietly ignore the pin — which is precisely the bug being fixed, reintroduced by the compatibility story. Both ends already refuse a version they do not recognise, so bumping turns "silently launches the wrong adapter" into a `VersionMismatch` that `lazydap shutdown` clears and auto-spawn replaces. The cost is one daemon restart; the alternative is a debugger obeying a configuration nobody can see. `Shutdown` stays version-exempt, so the escape hatch still crosses the boundary.

**Not changed:** the daemon's own discovery is kept rather than deleted. It is the fallback for a client that sends nothing, and it is what `discover` still means for any future caller inside the daemon.

---

## Open decisions

These need user input.

*(O01–O04 were answered on 2026-07-30 and became D024, D025, D026 and D027.)*

None outstanding. One question is parked for M15: whether to publish crates to crates.io. Default is no — `publish = false` stays, matching mxr.

---

## Decisions to revisit at v0.1 → v0.2 boundary

- D013 (codelldb-only) → **debugpy landed at M18**; js-debug + delve still open.
- D007 (single-session enforcement) → multi-session lift.
- D023 (AI external) → re-evaluate. May want to ship a thin `lazydap-mcp` server crate as an officially-maintained client.

---

## D051 — lazydap does not publish to crates.io

**Status:** decided (2026-07-31, user).

**Why:** The crates are organizational seams, not library APIs — the same stance mxr
records. Users install from GitHub release binaries (the `product-release.yml` artifacts)
or `cargo install --git`. `publish = false` stays on all seven crates and the release
workflow has no publish job.

**Revisit when:** someone asks to depend on `lazydap-protocol` or `lazydap-dap` as a
library. That request is the signal the seams have become APIs.


---

## D052 — the `DebugAdapter` trait lives in the daemon's adapter module, not in `lazydap-core`

**Status:** decided (2026-07-31, M18). **Completes D029.**

**Why:** D029 deferred the trait until a second adapter existed, on the grounds that a trait with one implementor hides where the seam is rather than marking it. debugpy is that second adapter, so the trait is now written — and the question D029 left open is *where*.

Not in `lazydap-core`. Every method on it speaks DAP: the `adapterID` for `initialize`, the adapter's `launch` arguments, the `reason` string on a `stopped` event. `lazydap-core` is depended on by every other crate, so putting the trait there would carry the DAP vocabulary into all of them — undoing the single thing this boundary exists to do (`ARCHITECTURE.md`, anti-pattern 4), and doing it in the name of the abstraction that was supposed to enforce it.

So it lives in `crates/daemon/src/adapter/`, and the module boundary keeps doing the enforcing: `lazydap_dap` is imported nowhere else in the daemon, checked by `scripts/check_architecture_boundaries.sh`. Non-negotiable #5 — "the daemon depends on the `DebugAdapter` trait, not raw DAP messages" — is now literally true of `handlers::session`, which calls `adapter::launch` and never names an adapter.

**Shape:** object-safe and synchronous. Starting an adapter is described as a `Spawn` value (`Tcp { command }` or `Stdio { program, args }`) rather than performed by the trait, so no method is `async` and nothing has to box a future — which is what lets `for_kind` return a `&'static dyn DebugAdapter` with no allocation and no `async-trait` dependency. It also makes the difference between the two adapters assertable in a unit test instead of only observable by running a process.

**What is *not* in the trait:** everything after the launch. Stepping, stacks, scopes, variables, evaluation and breakpoints are specified precisely enough that both adapters answer them identically, and all of it stays in the one `AdapterHandle`. The trait is four required methods and two defaulted ones; if it grows, that is evidence of a real divergence, not of a missing abstraction.

---

## D053 — DAP transports are stdio as well as TCP, and reverse requests are refused rather than fatal

**Status:** decided (2026-07-31, M18).

**Why:** codelldb listens on a TCP port and announces it on stderr; debugpy speaks DAP over its own stdin and stdout and is not a binary at all (`python3 -m debugpy.adapter`). The framing is identical either way — `Content-Length` headers and a JSON body — so `DapReader`/`DapWriter` now hold boxed `AsyncRead`/`AsyncWrite` instead of TCP halves, and `DapTransport` offers `spawn_tcp` and `spawn_stdio`. Boxed rather than generic so that no type holding a transport grows a parameter it does nothing with; the cost is one virtual call per read of a stream already crossing a process boundary.

**Reverse requests.** A message with `type: "request"` arriving *from* the adapter used to fall through `read_incoming`'s match into `TransportError::Dap`, which the pump reads as the adapter dying — so a question would have killed the session. There are two in the wild, `runInTerminal` and `startDebugging`, and lazydap advertises neither. Every launch it builds is also configured not to provoke them: codelldb gets `terminal: "console"`, debugpy gets `console: "internalConsole"` and `subProcess: false`.

An adapter that asks anyway is now answered with `success: false` (`DapWriter::refuse`) in both the handshake and the pump. Silence is the worse failure: the adapter waits for a reply that never comes, the debuggee never starts, and the session dies at a timeout naming the wrong thing. A refusal it can read leaves it free to fall back or to fail in its own words.

---

## D054 — lazydap launches Python with `justMyCode: false`

**Status:** decided (2026-07-31, M18).

**Why:** debugpy defaults `justMyCode` to `true`, which hides library and standard-library frames from the stack and steps over them. That default is written for a human debugging their own application in an editor. lazydap's first-class caller is an agent asked why a program failed, and that failure is as likely to be in a dependency as in the project — a stack that silently omits where the program actually is makes it unfindable, and nothing in the output says frames were removed.

**The cost, stated plainly:** the stack at a stop-on-entry pause includes debugpy's own `runpy` frames, because that is genuinely where the interpreter is. That is noise; a stack that lies is worse.

**Related, and deliberately not decided here:** lazydap sends no `setExceptionBreakpoints` filters, so an uncaught Python exception is *not* a pause — the program exits non-zero with its traceback on stderr, exactly as it would unattended. codelldb's segfault case does pause, because a signal is something the debugger sees whether or not anybody asked. Making Python match would mean choosing exception filters for every caller, which is a bigger decision than M18 gets to make on its own. `crates/daemon/tests/wait_debugpy.rs` asserts the current behaviour so that changing it has to be deliberate.

---

## D055 — `continue` on a program that is already running is not sent to the adapter

**Status:** decided (2026-07-31, M18).

**Why:** found by running the agent loop against debugpy, not by reading code. `lazydap launch` without `--stop-on-entry` returns while the program runs; the natural next command is `continue --wait` to reach the first breakpoint, and it is what `launches run` does for any `launch.json` configuration that does not set `stopOnEntry`. codelldb acknowledges such a `continue` and nothing happens. debugpy does not answer it at all — there is no paused thread to resume — so the acknowledgement timeout fires, and `AdapterHandle::execute` correctly reads an unacknowledged execution request as a wedged adapter and kills the session (D021, D022).

**What changes:** when the session is already `Running`, `continue` is not sent. What the caller wants is the next stable state, which `--wait` is already subscribed for. On codelldb the observable outcome is unchanged, minus one request that could only ever have been a no-op.

**The decision and the state transition are one locked operation** (`Session::claim_run`), because sampling the state and then writing it leaves two windows for the pump to record a stop in between, and each corrupts a different thing. A stop landing *before* the sample made an already-running program look paused, so the `continue` went out and resumed the program past the very stop the caller was about to be told about. A stop landing *after* it was overwritten by the unconditional `Running` that followed, leaving `--wait` returning a paused blob while the session claimed to be running — and every later `stack`, `scopes` and `eval` refused, because those need a stable state. Same shape as the compare-and-set `restore_state` already uses, and for the same reason. The claim is taken *before* the subscription rather than after: a stop arriving in between is not lost, because `Wait::begin` reads the undelivered backlog as well as subscribing.

**Residual, stated rather than hidden:** a stop that lands before the handler runs at all is indistinguishable from a session that was simply paused when the caller typed `continue` — which is the ordinary, correct case. Such a `continue` resumes, and the wait may report the stop it did not cause. Telling those apart means deciding whether `--wait` should ever report a stop from before its own request, which is a wider question than D055.

**Not changed:** `step` on a running program. It has no equivalent reading — "step" cannot mean "wait for whatever happens next" — and giving it one would be inventing behaviour rather than removing a redundant message. It remains a way to reach an adapter timeout, on both adapters.
---

## D056 — watches are project state with session-scoped values; protocol goes to v5

**Status:** decided (2026-07-31, with M16).

**Why the split.** A watch is two things with two different lifetimes, and keeping them
apart is the whole design. The **expression** is the project's: it lives in
`.lazydap/state.toml` beside the breakpoints, it exists before any session and it outlives
the daemon and the machine. The **value** belongs to one stop — the moment the program
moves, it describes somewhere the program has been. Persisting the second would be
persisting a lie: a file saying `pos = 4` read back tomorrow claims it still is.

So `Watch` is stored and `WatchValue` never is, and the TUI drops every value on
`Continued`, `SessionEnded` and `DaemonGone` while keeping every expression. It is the same
division `Breakpoint` makes against `AdapterBreakpoint` (D043), arrived at from the same
direction.

**Why the daemon does not evaluate.** `lazydap watch list` returns expressions, not values.
The daemon stores; whoever wants a number asks for one with `Request::Eval`, which is what
the TUI does at every stop and what `lazydap eval` already exposes. A `watch list` that
evaluated would need a paused session to answer at all, which would make the one command
that reads project state fail exactly when there is no session — the state it is for.

**Why the protocol bumps.** `Request` is an externally-tagged `serde` enum with no
fallback, so a variant an older daemon does not know does not fail *softly*: the whole
`IpcMessage` fails to deserialise, and the daemon never reaches the `version` field it
would have refused on. Verified against the real codec — the frame
`{"version":4,"id":7,"payload":{"Request":"WatchList"}}` produces `unknown variant
'WatchList', expected one of 'Ping', ...`, and `serve_client` answers `BadRequest` on id
`0` and closes. The client filters replies by request id, so it discards that frame and
reports "the daemon closed the connection before answering" with exit 3. The bump turns
that into the `VersionMismatch` `ensure_daemon_running` already clears by restarting the
daemon — the same reasoning as D032, D043 and D050, and the reason a "purely additive"
variant is not additive here. `Shutdown` stays frozen and version-exempt regardless.

**`Event::WatchUpdated` is project scope only.** `BreakpointUpdated` needs an
`Option<SessionId>` because an adapter can hold an opinion about a breakpoint. Nothing
installs a watch — there is no DAP request for one — so no session can have an opinion, and
the event carries a `WatchId` and nothing else. What it means is D043's lesson applied
before the bug rather than after it: "the list is not what you last read; read it again".
An add and a removal arrive identically, and only the list tells them apart.

**Consequences:** `WatchReport` has no `applied_to_session`, because there is nothing for a
session to have been told. `lazydap watch` uses real subcommands rather than `break`'s
flags: `break`'s add case carries a location and four modifiers and reads better without a
verb, whereas a watch is an expression and nothing else.

---

## D057 — the REPL evaluates in `watch` context, and `/` reaches the adapter

**Status:** decided (2026-07-31, with M17).

**Why:** M17's own task file calls the REPL "the natural UX for raw adapter commands", which
is true and is not the same as making that the default. D034 already established what
`repl` context means to codelldb: the string goes to LLDB's *command* interpreter, so `x`
on a program with an `int x = 5` runs `memory read` and fails on a missing address
(quirk 7). A REPL pane whose most obvious possible input fails is not a REPL.

So the pane sends `EvalContext::Watch` by default — the same context `lazydap eval` sends,
for the same reason, so the pane and the subcommand cannot disagree about what typing an
expression means. Adapter commands keep their place behind a `/` prefix: `/bt` is LLDB's
`bt`. One character, and unambiguous, because no expression begins with a division.

**Which frame.** The one the stack pane has selected, falling back to `None` — "the top
frame", which the daemon resolves by fetching it fresh — whenever the stack on screen
belongs to a stop the program has left. That keeps the REPL, the scopes pane and the
watches pane all talking about the same function, which is also why selecting a caller
frame re-evaluates the watches against it.

**History is per-session.** It lives in the pane and dies with the process. The phase-E
sketch left this open; persisting it is a config option for after v0.1, and a debugger that
wrote every expression you tried into a file in your repository is a surprise nobody asked
for.

**Consequences:** while the cursor is in the REPL, `q` is a `q` and `c` is a `c` — the pane
claims every key that could be part of an expression. The keys that move the *program* are
all function keys, none of which can appear in an expression, so those still work while
typing. `Esc` clears a half-typed line and then leaves the pane, because `q` cannot be the
way out and a user who tabbed in should not have to already know about `Tab`.

---

## D058 — an input context swallows every chord, and a paste is never a command

**Status:** decided (2026-07-31, review round after M16/M17).

**Why:** two ways the TUI could be made to run a debug command by somebody who was only
typing, both found by review of the panes M16 and M17 added.

**Modifiers were not part of a binding.** The reducer matched on `KeyCode::Char('c')` and
nothing else, so every character binding fired on its control form too: `Ctrl-C` — the most
reflexive key on a terminal, and what a person presses to mean "stop" — sent a `Continue`
and resumed the debuggee. A binding is now a *plain* key: no control, no alt, no super.
`Shift` is deliberately still allowed, because `G` arrives with it.

**Chords inside a text field are consumed there, not passed on.** The first version let
`Ctrl-D` and `Ctrl-U` fall through from the REPL to scroll the source pane, which meant an
allowlist decided which chords were safe to leak — and the allowlist is exactly what let
`Ctrl-C` through. An input context now claims every `Char`, chorded or not. `Ctrl-C` clears
the line, in both the REPL and the add-watch prompt, because that is the meaning a shell
gives it and nothing in lazydap interrupts a debuggee from the keyboard. Any other chord is
consumed and ignored: a key that does nothing in a text field is better than one that
reaches past it.

**A paste is not the keystrokes it resembles.** Without bracketed paste the terminal
delivers pasted text as ordinary key events, so pasting `counter\nc` into the add-watch
prompt submitted `counter` on the newline and then handed the `c` to the global bindings,
which continued the program. Bracketed paste is now enabled for the life of the TUI and
disabled on the way out — leaving it on would have the user's shell receiving `\x1b[200~`
around everything they paste afterwards. A terminal that refuses it is not a reason to fail
to start; it only means pastes arrive as keystrokes, as they always did.

**Newlines in a paste are stripped, not obeyed.** Both places a paste can land hold a
single expression, so a multi-line paste is either an accident or a wrapped line. Joining
it onto one line is recoverable and visible before `<CR>`; submitting the first line and
evaluating the remainder is neither. `<CR>` stays the only thing that submits. A paste
arriving when nothing is taking text is dropped, because there is nothing sensible for it
to mean and guessing is how this class of bug starts.

---

## D059 — a read of a paused program is fenced against it resuming underneath

**Status:** decided (2026-07-31, review round after M16/M17).

**Why:** `paused_session` is a check, not a hold. Nothing in the daemon owns a session's
state for the length of a request, and the inspection handlers that need a frame do two
awaits: resolve the frame, then ask the adapter the real question. Another client calling
`continue` in the gap — a second terminal, or the TUI's own F5 — leaves the second request
arriving at a *running* program. What comes back is either values from wherever it has got
to, or nothing at all until the adapter's own timeout fires ten seconds later. Neither reads
as "you asked about a program that is no longer stopped", which is what happened.

Each session now counts the writes to its state. A handler samples that counter beside its
pause check and re-verifies it immediately before the request it actually wanted to make;
a mismatch is `SessionNotPaused`. This is D040's discipline — number the thing, drop what
has been overtaken — applied daemon-side rather than in the TUI's reducer.

**Why a counter rather than re-reading the state.** A program that resumed and stopped again
is *paused*, so re-reading would say yes. It is a different stop: every frame id resolved a
moment earlier addresses nothing in it, and answering would be the right shape of reply
about the wrong moment. Counting writes catches that; comparing states does not.

**Consequences:** applied to `eval` and `scopes`, the two handlers that resolve a frame
before asking their real question. `stack_trace` and `variables` take one step and have no
gap of their own. M16's watches made this reachable in ordinary use rather than only under
contention: a stop fires one evaluation per expression, and they queue behind each other.

## D060 — the Homebrew formula ships the release binary, and `install.sh` does not trust `releases/latest`

**Status:** decided (2026-07-31, M21).

**Why a binary formula.** `brew install` could build lazydap from source, and Homebrew would
happily drive `cargo install` for it. That makes every user install a Rust toolchain and
spend minutes recompiling something the release workflow already compiled on a native runner
and checksummed. The formula points at the release tarball for the three targets we build —
macOS arm64 and x86_64, Linux x86_64 — and Homebrew verifies the same SHA-256 the workflow
published. Anything outside those three still builds from source, which is the honest answer
rather than a formula that pretends to cover platforms with no build behind them.

The formula states `version` explicitly instead of letting Homebrew scan it out of the URL.
`brew audit --strict` calls that redundant and it is, for a release like `0.1.0`. It stops
being redundant at the first prerelease: `lazydap-0.2.0-rc1-aarch64-apple-darwin.tar.gz` is
not a filename to hand a version parser and hope. A cosmetic audit warning in a
single-project tap is a smaller cost than a formula that installs the wrong version once.

**Why `install.sh` resolves "latest" through the release list.** The obvious move is the
`releases/latest` redirect, which is what mxr's installer does. It is wrong here for two
independent reasons, and it was wrong on the day this was written: this repository also
publishes `chapter-*` releases for the learn-by-LLM book, and product releases below 1.0 go
out as prereleases, which that redirect skips. Asked on 2026-07-31 it answered
`chapter-08` — a book chapter, not a debugger. The installer reads
`/repos/{owner}/{repo}/releases` and takes the newest tag beginning `v` instead. It costs an
API call against an unauthenticated rate limit, and a failure there names the fix: pass a
version.

**Two notions of prerelease, treated differently.** GitHub's prerelease *flag* is ignored
when resolving `latest`: every `v0.*` release sets it deliberately, because a 0.x release is
not a stability promise, so honouring it would leave `latest` finding nothing at all until
v1.0. A semver prerelease *suffix* is skipped: somebody who named no version wants the
newest release meant for them, not `v0.2.0-rc1`. Tags are `vX.Y.Z` or `vX.Y.Z-suffix`, so a
hyphen is the whole test.

**What the checksum proves, and what it does not.** The installer parses the `.sha256`
manifest itself rather than handing it to `shasum -c`. `-c` lets the manifest choose which
file gets checked, which makes the manifest — the untrusted half — the one deciding whether
anything is verified at all; a manifest naming some other file passes while the archive is
never hashed. So: exactly one entry, a 64-hex digest, and a filename that must equal the
archive about to be installed; then hash the download directly and compare strings. Downloads
are restricted to `https://` and `file://`, because over plain http the same attacker serves
both the archive and the digest vouching for it.

None of that is authenticity. The archive and its digest share an origin, and whoever
controls that origin can serve a matching pair. Requiring https keeps a network attacker out
of the origin; closing the rest needs a signature over the release, which is recorded as a
follow-up in the M21 task file rather than pretended at here.

**Consequences:** the tap (`planetaryescape/homebrew-lazydap`) is a second repository, so
the release workflow's `homebrew` job needs a `HOMEBREW_TAP_TOKEN` secret it cannot create
for itself. Without the secret the job renders the formula, logs that it is skipping the
push, and succeeds — forks and rehearsals are not broken releases. The rendered formula is
printed to the job log either way, which is also how the tap gets its first copy.

Because the tap is shared state outside this repository, that job carries a global
concurrency group rather than the workflow's per-ref one, and refuses to push when the tap
already serves a newer version. Two releases in flight would otherwise race, and the loser
could quietly put the older formula back. For the same reason the Homebrew line is appended
to the release notes by that job *after* the push succeeds, rather than written into the
notes the publish job builds: a release whose tap update skipped never mentions `brew`, so
the notes cannot advertise an install command that would hand somebody the wrong version.

## D061 — a debuggee is identified by what the adapter ran, not by what we asked for

**Status:** accepted (M22)

D045's reaper kills a debuggee whose adapter died without stopping it. Before killing
anything it checks identity: the pid is still running, and its `ps` command line is still the
program the session launched. That check exists because a daemon outlives many programs, and
killing whatever holds a recycled pid would be far worse than the leak it prevents.

The check compared against **the path lazydap passed to `launch`**. That is right for
codelldb, which execs the binary it is given, and for debugpy, which runs the script as an
argument to an interpreter. It is wrong for delve: `mode: "debug"` *compiles* the `.go`
source and runs the resulting binary, so the process in the table is a file lazydap never
named. The identity check found a command line it did not recognise, correctly concluded
"this is somebody else", and declined to kill lazydap's own debuggee.

Found by M22's adapter-kill test leaking two Go debuggees, which is how D045 itself was
found — by counting orphans.

**Decision:** the DAP `process` event already carries `name`, the program the adapter says it
actually started. Take its word for it, and fall back to the launched path only when it gives
none. One rule for every adapter, rather than a delve-shaped exception.

`name` is only used when it is an **absolute path**. The specification describes it as
something to show a user, so an adapter is entitled to put a label there — and a reaper
matching a label like `node` against `ps` output would kill whatever agreed with it. A
relative or bare name falls back to what was launched, which is exactly as good as the
behaviour before this entry.

**Consequences:** codelldb is unaffected — it sends no `process` event at all (quirk 9), so
its pid still comes from scraped console text and carries no name, which is correct because
its debuggee *is* the program that was launched. debugpy sends the event with the script
path, which is what the check already used. Only delve changes behaviour, and only because
only delve runs something other than what it was handed.

The reaper is still best-effort, and still refuses more often than it kills. A Go program
whose adapter dies while it is *running* is now reaped; one whose path contains a space still
is not, for the reason the module has always given.

## D062 — starting a TCP adapter is a recipe the adapter supplies

**Status:** accepted (M22)

`DapTransport::spawn_tcp` took an adapter path and hard-coded everything else: `--port 0`,
`RUST_LOG=debug`, read stderr, look for `Listening on `. All four of those are facts about
codelldb, sitting in the transport crate, which is the one place that is supposed to know
nothing about any particular adapter.

delve disagrees with codelldb about every one of them. It is `dlv dap --listen=127.0.0.1:0`,
it needs no environment, and it announces `DAP server listening at: 127.0.0.1:54421` on
**stdout**.

**Decision:** `Spawn::Tcp` carries a `TcpSpawn` — program, arguments, environment, which
stream the announcement is on, and the marker text before the address. The adapter module
supplies it; the transport does as it is told.

The alternative was a second `spawn_tcp_delve`, which would have put a second adapter's name
in the transport crate and left the two startups free to drift apart in different functions.

**Consequences:** the codelldb-specific knowledge moved from `crates/dap` into
`crates/daemon/src/adapter/codelldb.rs`, beside the rest of it, and `RUST_LOG=debug` now has
its explanation next to the thing it explains. The three M2–M4 walkthrough examples build
their transport through `adapter::for_kind(...).spawn(...)` rather than restating the recipe,
so they cannot drift from it either.

The stream that is *not* carrying the announcement is drained into the log rather than left
unread, which it had to be anyway: a child whose pipe fills up blocks writing to it, and an
adapter blocked in a log call answers no requests.

## D063 — the protocol goes to v6 for the `Delve` adapter variant

**Status:** accepted (M22, review of D061)

Adding `AdapterKind::Delve` changes the wire without adding a request or a field, which is
the subtle half of a breaking change and the one M22 first shipped without catching. A
`LaunchRequest` carrying `adapter: "delve"` is written by a build that has the variant and
cannot be deserialised by one that does not: `AdapterKind` is an externally-tagged enum with
no fallback, so an unknown variant fails the whole envelope.

Left at v5, the failure surfaced in the worst place. A v5 daemon left running from before
this branch passes the version handshake — its version *is* 5 — and only then, on the first
Go launch, fails to decode the request and drops the connection. The client reports that as
"the daemon closed the connection", not as the `VersionMismatch` that `lazydap shutdown`
clears and the auto-spawn path resolves on its own. codelldb and debugpy launches stayed
decodable by a v5 daemon, which is exactly why it was easy to miss.

**Decision:** bump `LAZYDAP_PROTOCOL_VERSION` to 6, same reasoning as D043/D056 — move the
failure back to the handshake, where it is recognised and repaired. The full rationale lives
on the constant itself (`crates/protocol/src/types.rs`), where every bump since v2 is
recorded.

**Consequences:** none for the frozen `Shutdown` escape hatch, which carries the *daemon's*
version rather than ours and is tested against a literal v1 frame — so a v5 daemon still
answers a v6 client's shutdown and restarts as v6. That cross-version path is what makes a
variant-only bump safe to ship: the stale daemon is replaced, not merely rejected.

---

## D064 — codelldb's `pause` is reported as `pause`, extending D033 to the other SIGSTOP

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** D033 fixed one half of a mistake and left the other half in place. codelldb stops a
program by sending it `SIGSTOP`, and LLDB classifies a signal stop as an exception — so
*both* of the ways lazydap asks a program to stop on purpose report `reason: "exception"`.
D033 renamed the entry stop and nothing else, so a `lazydap pause --wait` answered:

```json
{"state":"paused","reason":"exception","raw_reason":null,"frame":{"name":"__ulock_wait",...}}
```

Reproduced twelve times out of twelve. The wire carries
`{"reason":"exception","description":"signal SIGSTOP","allThreadsStopped":true}`, and lazydap
dropped the `description` as well — so nothing in the JSON told a pause from a real
exception. `PauseReason::Pause` existed in `lazydap-core` and was unreachable with the
primary adapter.

An agent that asks a program to stop and is told it crashed will go and diagnose the crash.
That is the same expensive wrong answer D033 was written about, and non-negotiable #3
settles it the same way: `reason` is lazydap's vocabulary.

**Decision:** the session records the execution request the program has not answered yet
(`state.rs`'s `Outstanding`), and `normalise_stop` reads a `StopContext` rather than a bare
`stop_on_entry` flag. A SIGSTOP-signature stop is renamed to `pause` when a pause is
outstanding and to `entry` when a launch asked for one, and the adapter's own word goes in
`raw_reason` either way. Everything else passes through.

The marker is written *before* the request goes out: an adapter can emit the `stopped` event
before it acknowledges the request that caused it, and the pump reads the marker as the stop
arrives.

**Consequences:** `normalise_stop` is now called from the pump as well as the handshake.
D033's consequence paragraph asserted the opposite — "by the time the pump is running, the
adapter's reasons are its own" — and a comment in `pump.rs` repeated it. That was the bug:
an adapter's reasons stop being its own the moment lazydap asked it for something. Both have
been corrected.

The guard stays as narrow as D033's. A real exception during a pause does not carry
`SIGSTOP` and passes through; a `SIGSTOP` nobody asked for passes through; a `pause` sent to
an adapter that reports it properly needs none of this.

---

## D065 — a thread the adapter did not name stays unnamed

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** `lazydap threads` on a *running* program answered, with exit 0:

```json
{"threads":[{"id":0,"name":"thread 0"}]}
```

The wire says `{"threads":[{"id":0,"name":""}]}`. codelldb's answer is a placeholder, and an
obviously useless one — which is the useful part of it. `translate.rs` turned the empty name
into `format!("thread {}", thread.id)`, which reads like a real answer about a real thread.
Nothing downstream could tell the two apart, because by then there was nothing to tell apart.

lazydap must not invent data an adapter did not provide. The rule is the one D033 already
implies and this is the first case where the honest answer is *absence* rather than a
rename.

**Decision:** `ThreadInfo::name` is `Option<String>`, absent from the JSON when the adapter
named nothing. The table renderer prints an empty cell — honest, where a fabricated name is
not.

**Alternatives considered:** rejecting `threads` while the program runs. `handlers/inspect.rs`
deliberately allows it, and that stays: which threads exist is a fair question at any time,
and the answer being adapter-dependent is a fact about the adapter, not a reason to refuse.
Refusing would also have removed the one signal that says "this answer is a placeholder".

**Consequences:** a client formatting `thread.name` unconditionally has to handle the key
being absent — which is the point, and why this is part of the v7 bump rather than a silent
change.

---

## D066 — a step is reported against the thread it was aimed at

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** codelldb answers a `next` aimed at one thread by naming another. Captured off the
wire with a four-worker C program stopped at one breakpoint:

```text
next(threadId=34353117) -> stopped {"reason":"step","threadId":34353117}   moved: [117]
next(threadId=34353118) -> stopped {"reason":"step","threadId":34353117}   moved: [118]
```

The second is the bug: the reported thread is the *previous* step's target, and it did not
move. Ten times out of ten, the thread the caller asked for moved and the thread lazydap
reported did not. lazydap relayed the adapter's answer verbatim, so the blob described a
thread that had not stepped — and worse, that thread became `last_thread_id`, so the next
bare `lazydap stack` answered about the wrong thread as well.

**Decision:** the session already records what it asked for (D064's `Outstanding`), so a step
is reported against the thread it targeted, and the adapter's own answer is kept beside it in
a new `adapter_thread_id` — present only when the two disagree. That is `raw_reason`'s
discipline applied to the thread rather than to the reason.

No frame is fabricated for the reported thread: the blob's frame is fetched for whichever
thread the blob names, so reporting the requested thread means fetching *its* frame. A live
step now answers `thread_id: 34475438, adapter_thread_id: 34475437, frame: work:16`, and the
frame is the stepped one.

**Alternatives considered:** relaying the adapter's thread and adding a `requested_thread_id`
beside it. Rejected because the field a caller reads first is `thread_id`, and leaving the
wrong answer there means the fix only helps somebody who already suspects the problem.
Leaving `last_thread_id` alone and fixing only the blob — rejected for the same reason: the
poisoned `last_thread_id` is how the wrong answer spreads to the *next* command.

**Consequences:** the guard is narrow, three conditions deep. It applies only to a stop whose
reason is `step`, only when a step is outstanding, and only when the two threads differ. A
breakpoint hit on another thread while this one was stepping is the adapter telling us
something we did not ask about, and passes through untouched — verified live, where
`step --thread A` answering `reason: "breakpoint"` on thread B is reported as thread B.

---

## D067 — `variables`' filter and window are applied here when the adapter ignores them

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** `lazydap variables --start 100 --count 5` on a 2000-element array returned all 2001
entries, starting at `[0]`. So did `--filter`. All three were built into the DAP request and
sent; codelldb ignored them.

It was right to ignore them. DAP puts `filter`, `start` and `count` behind
`supportsVariablePaging`, and a client may only send them when the adapter has declared it.
codelldb does not — confirmed on the wire, its `initialize` answer has no such key. lazydap
sent them anyway and reported whatever came back, so three documented flags were silent
no-ops against the primary adapter.

**Decision:** the capability is read from `initialize` and carried on the `AdapterHandle`.
When the adapter declares paging, the arguments are sent as before. When it does not, they
are not sent at all — and are applied to the answer instead. The full array is already in
hand, which is what makes this narrowing rather than guessing.

**Consequences:** `AdapterCapabilities` gains `supports_variable_paging`, so a caller can see
which of the two happened. A `count` of `0` still means "the rest from `start`", as DAP says,
rather than an empty answer.

**Amended by D073 (2026-08-02), before shipping.** Two claims above are wrong. DAP gates only
`start` and `count` behind `supportsVariablePaging` — `filter` carries no capability and is
always sent, so withholding it suppressed correct adapter-side filtering. And the client-side
`filter` this entry described classified children by whether a name looked like `[0]`, which
is codelldb's spelling rather than anything the protocol says; there is no client-side
`filter` any more. Only `start` and `count` are applied here.

---

## D068 — an evaluation error hidden inside a value fails the command

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** codelldb answers an expression it could not evaluate with a *successful* `evaluate`
whose result is the error text:

```text
$ lazydap eval '*boxed_dyn'   -> {"value":"<error: invalid value object>"}                     exit 0
$ lazydap eval 'v_empty[0]'   -> {"value":"<read memory from 0x4 failed (0 of 4 bytes read)>"} exit 0
```

lazydap's success is the DAP envelope's, so both arrived as a `value`, no `error`, exit 0.
The documented contract says exit 0 means the value is a value; an agent branching on it
treats "could not read that memory" as a reading. This is the failure mode of the whole
batch in miniature — not a missing feature, an answer that is confidently wrong.

**Decision:** a per-adapter predicate, `DebugAdapter::is_eval_error`, defaulting to `false`
and implemented only for codelldb. A matching result fails the command through the existing
error envelope, so it exits 1 with `DapProtocolError` and the adapter's own text in
`adapter_message`. Failing properly beat adding a flag: a flag would leave every existing
caller reading the same wrong `value`, and the exit code is the thing the contract already
tells agents to branch on.

**Alternatives considered:** treating any `<...>`-wrapped value as an error. Rejected — a
false positive here turns a working `eval` into a failure, which is worse than the bug.
Python's `<__main__.Foo object at 0x10a>` and LLDB's `<incomplete type>` are legitimate
values of exactly that shape. So the brackets are necessary and not sufficient: the text
inside must also open with `error:` or say something `failed`.

**Consequences:** the heuristic reads a human-readable string, which is as brittle as it
looks, and is why it lives behind the adapter trait rather than in the shared handle — it
runs for codelldb and nothing else. A value that merely *contains* those words is left alone.
The TUI's watches pane already had a `WatchValue::Error` arm, so a failed watch now renders
as an error rather than as a value that reads like one.

**Amended by D074 (2026-08-02), before shipping.** The " failed" half of the predicate went
in despite being flagged as a risk, and is gone. `<last operation failed>` is a summary
string a real program can have, so the bracket-plus-" failed" shape is not evidence of
anything. The check is now the literal `<error:` prefix, which means the second example above
— `<read memory ... failed ...>` — is a documented gap rather than a caught error.

---

## D069 — the answer carries what the adapter already knew

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** three small losses, all the same shape — the adapter answered a question and lazydap
dropped the answer on the way out.

- **`evaluateName`.** codelldb sends `"evaluateName": "big[100]"` on every variable row.
  `DapVariable` did not model it, so it was discarded. It is the adapter's own answer to
  "what expression names this row", and given codelldb's expression limits it is the only
  reliable route from a `variables` row called `[100]` or `label` to a working `lazydap eval`
  argument.
- **`source.name`.** codelldb and delve send it; debugpy sends only `path`. lazydap passed
  the key through as-is, so an agent formatting `frame.source.name` got two languages and a
  blank — while lazydap held the path the name is *in*. `SourceRef::label` already knew the
  fallback; it is now applied one step earlier, where the shape is decided rather than where
  it is rendered.
- **`supportsVariableType`.** Never declared in `initialize`. Per DAP, `Variable.type` is only
  guaranteed to a client that asked for it. Both shipped adapters send it regardless, so
  nothing was broken — `type_name` simply rested on adapter leniency rather than on the
  contract.

**Decision:** surface all three. `Variable` gains `evaluate_name`, absent when the adapter
sends none. `source.name` is filled from the path's file name when the adapter omits it — a
derivation from data lazydap has, not an invention, which is what separates this from D065.
`InitializeArgs` declares `supportsVariableType`.

**Consequences:** `supportsVariableType` changes no observed behaviour today, which is the
argument for doing it now rather than when an adapter that honours it arrives and quietly
stops sending types.

---

## D070 — truncated `captured_output` is a prefix, not a splice

**Status:** decided (2026-08-01, from the dogfooding campaign).

**Why:** `--wait` caps `captured_output` at a megabyte (D9). The cap skipped any chunk that
would overrun it, set `output_truncated`, and **went on accepting later chunks that fit**.

Measured against a program printing 1500 kilobyte-lines, then pausing, then printing a
marker: about 500 lines vanished from the middle, and the marker — produced 800 ms later —
was concatenated directly onto a mid-line cut, with nothing indicating the join. The blob
said `output_truncated: true`, which every reader takes to mean "the tail was cut". What it
actually flagged was a splice, and an agent reasoning about the program's output would be
reading two moments in its life as one.

**Decision:** once the cap is reached the wait stops accepting output for the rest of the
run. What is kept is then a strict prefix of what the program printed, which is the only
shape the flag's ordinary reading is true of.

**Consequences:** slightly less output is retained in exchange for it meaning something.
The alternative — keeping the splice and marking the join — was rejected: a marker inside
`captured_output` is a value the program did not print, sitting in the field that says what
the program printed.

**Verified:** a live run keeps 998,173 of 1,501,530 bytes, the marker is absent, and the
retained text is a prefix of the program's own stdout. The deterministic version is
`what_survives_the_cap_is_a_prefix_of_what_the_program_printed` in `crates/daemon/src/wait.rs`.

**Completed by D072 (2026-08-02).** This entry could not yet promise a prefix. Output is also
lost *before* a wait starts, when the session's event buffer overruns between two commands,
and that path reported nothing — so a blob could be a suffix while claiming to be whole.
`output_truncated` now covers both causes and `dropped_events` says how much is missing.

---

## D071 — an outstanding request has an identity, and `pause` gets its own slot

**Status:** decided (2026-08-02, review of D064/D066).

**Why:** D064 and D066 both read a stop against "the request the program has not answered
yet", and both were given one slot to read it from. That was wrong twice over, and review
found it before it shipped.

**A `pause` deliberately does not take the execution permit** (D021) — it exists to
interrupt a run already under way, and queueing it behind that run would mean the only way
to stop a runaway program is to wait for it to stop. So a pause is routinely in flight
*beside* the step it is interrupting, and one slot cannot hold both. `step --thread A --wait`
racing `pause --wait` overwrote `Step(A)` with `Pause`, and if the step's stop landed first
the caller got both failures at once:

- D066's correction was skipped, so codelldb's wrong thread B was relayed and poisoned
  `last_thread_id` — precisely the bug D066 exists to fix;
- the marker was consumed, so the pause's own `SIGSTOP` arrived with nothing to read it
  against and came back as a genuine exception — precisely the bug D064 exists to fix.

**A rejected request left its marker installed.** `pause --thread 999` fails, `Pause` stays
up, the program keeps running, and the next genuine `SIGSTOP` — from anywhere — is renamed
to a pause nobody asked for.

**Decision:** two slots, and every marker carries an id.

- One slot for the permit-holding execution request (a step), one for the pause that
  bypasses the permit. That is the whole set: the permit admits one step at a time, and
  `interrupt` is the only other thing that moves the program.
- A stop consumes the marker it actually answers, decided *after* the stop has been read
  rather than before. A stop reported as `pause` takes the pause marker and leaves the step,
  which is still in flight — the program was stepping when it was stopped. Any other stop
  ends the run a step started and takes the step marker, leaving the pause still to be
  answered.
- Withdrawal names its marker. The error path clears the marker *it* installed, so a request
  that arrived while this one was being refused is untouched. Clearing "the marker" would
  have been the same bug one step along.
- A `continue` clears both: once the program resumes, a step still recorded is finished and
  a pause that never landed is stale. That is what bounds a marker's lifetime.

**Consequences:** the interleaving is tested directly rather than inferred — both orders of
a concurrent step and pause, a withdrawal that must not touch a newer marker, and a resume
that clears both. `Outstanding` is now a pair of slots rather than an enum, which is the
shape the concurrency actually has.

---

## D072 — `output_truncated` covers loss before the wait as well as during it

**Status:** decided (2026-08-02, review of D070).

**Why:** D070 made a truncated blob a prefix rather than a splice, and stopped there. It
could not actually promise a prefix, because the other way a blob loses output happens
before the wait exists.

`EventBuffer` holds a thousand events and drops the oldest when it fills. A debuggee that
prints more than that between two CLI invocations pushes the *beginning* of its own output
out of the buffer, and `undelivered()` reported no loss — so `continue --wait` handed back a
**suffix** with `output_truncated: false`. A blob that silently omits the beginning is the
same class of lie as one that splices the middle; D070 fixed one end of it and left the
other.

The count was already there. `EventBuffer` tracks `dropped` for the `output` command's
`dropped: N`, but a wait needs a different question answered — not "how many has this session
ever lost" but "is the blob I am about to hand back whole". Advancing `delivered` past a
dropped event, which is what stops the next wait trying to re-report something that no longer
exists, is also what made the gap invisible.

**Decision:** the buffer counts separately the events that fell off *before any wait carried
them*, and `undelivered()` returns that alongside the backlog. `output_truncated` is set by
either cause, and `StableState` gains `dropped_events` saying how many. The count resets when
a wait commits delivery, so a gap is reported once rather than by every blob thereafter.

The field's documented meaning is now "you are not seeing all of it", with the two shapes
spelled out: cap reached means what you keep is a prefix, events lost means it is a suffix.
A reader cannot act on a distinction it was never told about, so both set the flag and the
count is what separates them.

**Consequences:** a third cause — a live subscription falling behind, which already set the
flag — now contributes its count too, so `dropped_events` is the whole answer rather than
two thirds of it.

---

## D073 — `variables --filter` is the adapter's to honour, and lazydap does not fake it

**Status:** decided (2026-08-02, review of D067).

**Why:** D067 gated `filter`, `start` and `count` together behind `supportsVariablePaging`
and applied all three client-side when the adapter had not declared it. Both halves were
wrong.

**DAP gates only `start` and `count`.** `filter` is independently valid and carries no
capability, so withholding it from an adapter that would have honoured it suppressed correct
adapter-side filtering for no reason.

**The client-side fallback classified by a convention lazydap invented.** It read a child as
indexed when its name looked like `[0]` — which is how codelldb happens to spell them, not
something the protocol says. Against an adapter that spells its elements any other way it
would return the wrong rows, silently. DAP does provide counts for exactly this question,
`namedVariables` and `indexedVariables`, but they live on the *parent* variable, and a call
that has only a `variablesReference` does not have the parent. So the information genuinely
is not there at that layer.

**Decision:** `filter` is always sent and never emulated. `start` and `count` stay gated and
are still applied client-side when the adapter has not claimed them — they need no
interpretation, being a slice of the list the adapter returned.

An adapter that ignores `filter` is an adapter that was asked and declined. That is a fact
about the adapter, reportable as such, and a better answer than a guess dressed up as
support — the same principle as D065, where the honest answer to "what is this thread
called" was absence.

**Consequences:** `--filter` against codelldb returns everything, and the skill and site docs
say so rather than implying it works everywhere. `narrow` is now a slice and nothing else.

---

## D074 — the evaluation-error predicate is the narrowest defensible shape

**Status:** decided (2026-08-02, review of D068).

**Why:** D068 detected codelldb's error-in-a-successful-value by two tests: the value is
wrapped in angle brackets, *and* the text inside either opens with `error:` or contains
" failed". The second half is not evidence. `<last operation failed>` and `<error: sentinel>`
are summary strings a real program can have — an enum, a status field, a `Display` impl — and
codelldb returns them unchanged. Treating those as failures turns a working `eval` into an
error, which is a worse bug than the one being fixed: a false success costs a caller one
confusing value they can still see, while a false failure costs them a value they cannot get
at all.

**Decision:** the check is the literal `<error:` prefix and nothing else.

**Consequences:** a known and documented gap. codelldb reports an unreadable address as
`<read memory from 0x4 failed (0 of 4 bytes read)>`, which genuinely is an error and is still
reported as a value with exit 0. That trade is deliberate and asserted in the tests — both
unit and live — so it stays visible rather than being rediscovered as a bug. The skill's
schema doc tells agents that a value wrapped in angle brackets is worth a second look.

Widening this needs better evidence than a substring: an adapter that fails the request, or a
marker in the response. Not a longer list of words that sometimes mean failure.

---

## D075 — a frame id and a variables reference are lazydap's, minted per stop

**Status:** decided (2026-08-02, dogfooding round two).

**Why:** both used to be the adapter's own numbers, passed through in each direction. They
are valid only until the program moves, and an adapter is free to hand the same number out
again at the next stop for something else. A caller holding one across a `continue` therefore
had two futures, and the quiet one is much the worse:

- the number addresses nothing, and the adapter says so in its own words. `scopes --frame 0`
  gave `Invalid frame reference: 0`, which is at least true. `eval --frame 0` gave **`can't
  evaluate expressions when the process is running`** about a program that was plainly
  stopped — and an agent reading that starts polling something that is never going to move;
- the number has been **recycled**, and the adapter answers it. Somebody else's variables
  come back under exit 0, with nothing in the response to say the question was about a moment
  that has passed. That was reported as `{"variables": []}` and is the reason this is a Tier A
  finding rather than a message-quality one.

**Decision:** lazydap mints its own handles. One monotonic sequence per session, never reused,
each recorded against the stop generation it was issued at (D059's fence, which already counts
exactly this). A handle presented from an older generation is refused *before the adapter is
asked anything*, with `StaleHandle`; a number that was never a handle at all is `BadRequest`.
Both name the command that hands out a fresh one.

**Why minting rather than a check against the adapter's numbers.** A registry that merely
recorded which adapter numbers had been handed out cannot catch the recycled case: at the new
stop `1007` genuinely is a current handle, so the check passes and the caller still gets
another frame's variables. Only a number that is never reused makes "this belongs to an
earlier stop" decidable. That is the whole argument for the extra layer.

**Two codes, not one.** `StaleHandle` and `BadRequest` need different reactions — ask again
at this stop and retry, versus stop making the number up — so collapsing them would throw
away the only part a caller can act on. `--frame 0` is the second: an obvious thing to type,
never issued by anything, and nothing to do with the program having moved.

**Consequences:** `frame_id` and `variables_reference` are opaque small integers with no
relation to anything DAP said, which is what they always claimed to be. Handles are minted
under the same fence as the data they describe, so one can never be stamped with a stop its
frame did not come from — which added a fence re-check to `stack_trace` and `variables`,
where D059 had judged there was no gap. There was: between the adapter answering and the
numbers being handed out. A `variables_reference` of `0` is left alone, because it is DAP's
"this is a scalar" and not a reference. The protocol goes to v8: a v7 client would decode
these integers happily and then send them back to be read against a different table.

---

## D076 — `continue` on a running program reports what it did, which is nothing

**Status:** decided (2026-08-02, review of D055).

**Why:** D055 established that lazydap does not *send* a continue to a program already
running: codelldb ignores it and debugpy never answers, and the acknowledgement timeout that
follows kills the session. It said nothing about what to report, and what was reported was

```json
{"state":"running","thread_id":0}
```

under exit 0. Two inventions in one line. Nothing was resumed, so calling it a plain success
is a claim about an action that did not happen; and `thread_id: 0` is whatever codelldb
answers a `threads` call on a running process with, which is not a thread — the same class of
fabrication D065 removed from `ThreadInfo::name`.

**Decision:** `Response::Continued` carries `already_running`, and `thread_id` is `None` when
it is set. The field is not invented data — it is the daemon reporting its own decision.

**Why not an error.** "Continue" has a second reading — *get me to the next stop* — and with
`--wait` that is the documented ordinary sequence for a launch without `--stop-on-entry`
(D055). Erroring would break it. So the wait path is unchanged and only the fire-and-forget
answer had to stop lying. `pause` is the opposite case and is refused (D081).

---

## D077 — a message explains a refusal; a verified breakpoint has nothing to explain

**Status:** decided (2026-08-02, dogfooding round two).

**Why:** `launch` answered with a breakpoint that contradicted itself:

```json
{"id":6,"line":37,"verified":true,"message":"Resolved locations: 0"}
```

and the next `continue --wait` corrected it by event to `Resolved locations: 1`. `verified` is
the trustworthy half — a breakpoint on a comment line reports `false` in the same response —
so the stale count was making a working breakpoint look broken to anyone who read the fields
together.

**Decision:** drop `message` when `verified` is true, wherever an `AdapterBreakpoint` is built
from something an adapter said. What is left is the case the field is actually read for: an
unverified breakpoint, where the message is the reason.

**Why not wait for the adapter to settle.** The other option was to hold the launch open until
the `breakpoint` events arrived. That pays for a cosmetic field with launch latency on every
session, forever, and there is no upper bound on how long an adapter takes to make up its
mind. Dropping the message costs nothing real: where a breakpoint ended up is `line`, whether
it took is `verified`, and `Resolved locations: 1` was never actionable either.

**Consequences:** D048's symlink retry is unaffected — it reads the message of an *unverified*
breakpoint, which is exactly what is kept.

---

## D078 — a stop carries the frame responsible for it, and that frame's locals

**Status:** decided (2026-08-02, dogfooding round two).

**Why:** two independent sources found the same friction. Reading a local was always two
commands (`scopes`, then `variables --reference N`) for the single most common thing anybody
does after a program stops. And a crash blob's `frame` is usually not the frame anybody wants:
a real segfault reported `_platform_strcmp$VARIANT$Base`, which has a `source_reference` and
no path at all, and naming the responsible `lookup_key` at `config.c:40` turned a
two-command diagnosis into a five-command one.

**Decision:** one mechanism, in the wait's existing "fetch the top frame" step.

- **`user_frame`** — the nearest frame below `frame` that has a source path, present only when
  `frame` has none. **`frame` is never overwritten**: it is where the program stopped and
  stays that, whatever it turns out to be. Read it as `user_frame` first, `frame` second.
- **`locals`** — the locals of whichever of those two a person would look at, with
  `frame_id` naming which, so the choice is never something a reader has to infer. A crash
  inside `strcmp` has no locals worth reading in `strcmp`; the ones that explain it are in the
  caller that passed the null.

**Always on, because it was measured.** `user_frame` costs a longer answer to a `stackTrace`
that was already being made — 24 frames instead of 1, one round trip either way. `locals`
costs two more round trips. Measured over 15 stops on the same fixture: median `elapsed_ms`
57 with the context and 57 without, means 68.5 and 67.5. The cost is about a millisecond
against a wait dominated by the program itself, so a flag would have bought nothing and left
the common path needing two commands anyway.

**Bounded and honest.** The frame search stops at 24 frames, so a thousand-deep recursion
costs no more than a shallow one. Locals are capped at 100 with `truncated` saying so, the
same pattern as `output_truncated`. Every part is best-effort and absent when the adapter
would not answer — never an empty list, which would claim a frame has no locals when the
truth is that nobody could find out (D065's rule).

---

## D079 — `0` means no limit, for every flag that takes a count

**Status:** decided (2026-08-02, dogfooding round two).

**Why:** `--timeout 0` means "wait forever" and is documented. `stack --levels 0` meant
"nothing", and answered `{"frames":[]}` under exit 0 — a plausible-looking empty stack for a
paused program. DAP itself says `levels: 0` means all frames, so the passthrough was wrong
twice.

**Decision:** `0` is spelled "no limit" on `--levels`, `--count` and `--max`. Applied in the
daemon rather than the CLI so every client gets the same reading.

---

## D080 — a variables answer is capped by default and says when that bit

**Status:** decided (2026-08-02, dogfooding round two).

**Why:** now that `--start/--count` actually work (D073), nothing else bounded a response. A
`Vec` of 2000 expands to 2001 rows in one answer, and an agent that asked what a container
held had spent most of its context on one variable with nothing in the response to say so.

**Decision:** a default of 200 rows, `--max` to change it, `--max 0` to lift it, and
`truncated` on the response — so `Response::Variables` becomes a struct rather than a bare
`Vec`, which had nowhere to put the flag. The TUI passes `--max 0` explicitly: the cap
protects an agent's context, and a pane scrolls.

**Values are never shortened.** Only the list is. A 5000-character string is one row, and a
row reading `"abcd…"` would be a claim about the *data* rather than about the list — a caller
cannot tell a truncation marker from a value that really does end in an ellipsis. Truncating
a container is recoverable with `--start`; truncating a value silently changes what the
program was observed to contain, which is the one thing this must not become.

---

## D081 — `pause` on a stopped program is refused, not answered with the stop it found

**Status:** decided (2026-08-02, dogfooding round two).

**Why:** `pause --wait` on an already-paused session handed back the stop the program was
*already* sitting at, wearing a fresh `elapsed_ms` — a blob indistinguishable from one the
request had caused. An agent reading it concluded its pause had worked.

**Decision:** refuse it, `BadRequest`, naming the state and pointing at `lazydap status`.

**Why an error here and not for `continue` (D076).** The asymmetry is real. "Continue" has a
second reading — get me to the next stop — and it is the documented ordinary sequence after a
launch without `--stop-on-entry`, so it waits and reports honestly about what it did.
"Pause" has no second reading: the program is stopped, that is already the state the caller
wanted, there is nothing to interrupt and no future stop to wait for. The only alternatives
were an error or a fabricated one.
