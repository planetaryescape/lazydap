# Teaching session breakdown

Per-milestone session cuts for teaching mode. The underlying milestones live in [`docs/implementation/tasks/`](../implementation/tasks/) — those stay clean and ship-mode-ready. This doc is the parallel teaching plan.

Cognitive load discipline: one new concept per session. Some milestones are 1 session; dense ones are several. Each session produces three artifacts: a teach-back captured in the Obsidian session note (private), a public **book chapter** at `docs/book/<NN>-<title>.md`, and a teacher's-eye **teaching-notes** file at `docs/teaching/notes/<NN>-<title>.md`. See `~/.dotfiles/.skills/teaching/SKILL.md` rules 14 and 15.

Quick map from session ID → book chapter:

| Session | Chapter | Status |
|---|---|---|
| WS-1 | [`docs/book/01-cargo-workspaces.md`](../book/01-cargo-workspaces.md) | ✅ taught |
| WS-2 | [`docs/book/02-tokio-main-clap.md`](../book/02-tokio-main-clap.md) | ✅ taught |
| WS-3 | [`docs/book/03-conventions-as-code.md`](../book/03-conventions-as-code.md) | ✅ taught |
| M0-1 | [`docs/book/04-hello-adapter.md`](../book/04-hello-adapter.md) | ✅ taught |
| M1-1 | [`docs/book/05-read-one-message.md`](../book/05-read-one-message.md) | ✅ taught |
| M2-1 | [`docs/book/06-serde-typed-protocols.md`](../book/06-serde-typed-protocols.md) | ✅ taught |
| M2-2 | [`docs/book/07-dap-transport-and-seq.md`](../book/07-dap-transport-and-seq.md) | ✅ taught |
| M3-1 onwards | `docs/book/08-*` and onwards | stub — fill during the live session |

## Workspace setup → 3 sessions

[`docs/implementation/00-workspace-setup.md`](../implementation/00-workspace-setup.md)

| Session | Concept | What we do |
|---|---|---|
| **WS-1** | Cargo workspaces | `cargo init`, convert root `Cargo.toml` to `[workspace]`, create `crates/core` with workspace inheritance. Concept focus: what `[workspace]` and `version.workspace = true` actually do, why we have both root and crate manifests, the dependency-graph implications. |
| **WS-2** | `#[tokio::main]` + clap binary | Add `crates/daemon` with a binary that just parses args and prints. Concept focus: `#[tokio::main]` macro expansion, what async fn main means, clap derive macro basics. |
| **WS-3** | CI as code + license + conventions | `.github/workflows/ci.yml`, license files, `rustfmt.toml`, `clippy.toml`, `rust-toolchain.toml`, `.gitignore`, first commit. Concept focus: GitHub Actions YAML structure, why each conventions file exists. |

Anchor: mxr's `Cargo.toml` workspace shape — read it side by side.

---

## M0 — Hello, adapter → 1 session

[`docs/implementation/tasks/M00-hello-adapter.md`](../implementation/tasks/M00-hello-adapter.md)

| Session | Concept | What we do |
|---|---|---|
| **M0-1** | Spawning child processes in tokio | The whole milestone. Concept focus: `tokio::process::Command`, `Stdio::piped()`, `kill_on_drop`, the codelldb TCP-only quirk. |

Pre-session: install codelldb if not already present (we'll do this in-session, not before).

---

## M1 — Read one message → 1 session

[`docs/implementation/tasks/M01-read-one-message.md`](../implementation/tasks/M01-read-one-message.md)

| Session | Concept | What we do |
|---|---|---|
| **M1-1** | Async byte-stream parsing | Parse `Content-Length` header, read N bytes of body. Concept focus: `read_line` vs `read_exact`, why partial reads happen in async I/O, the `BufReader` pattern. |

Anchor: this is the smallest possible "framed protocol parser." When concepts solidify, generalises to any LSP/DAP-style stream.

---

## M2 — Initialize handshake → 2 sessions

[`docs/implementation/tasks/M02-initialize-handshake.md`](../implementation/tasks/M02-initialize-handshake.md)

| Session | Concept | What we do |
|---|---|---|
| **M2-1** | serde + typed protocols | Define `DapRequest`, `DapResponse`, `Capabilities` etc. with `Serialize`/`Deserialize`. Concept focus: derive macros, `#[serde(rename_all = "camelCase")]`, the JSON ↔ Rust mapping, why generic types in the request/response shape. |
| **M2-2** | The transport struct + atomic seq | Build `DapTransport` with `request<T, R>()` method. Concept focus: generics in method signatures (`<T: Serialize, R: DeserializeOwned>`), the `Atomic*` family and why we need it for sequence numbers, error type design with `thiserror`. |

This is dense. Splitting because typed protocols + the transport abstraction are independently meaty.

---

## M3 — Launch and observe → 2 sessions

[`docs/implementation/tasks/M03-launch-and-observe.md`](../implementation/tasks/M03-launch-and-observe.md)

| Session | Concept | What we do |
|---|---|---|
| **M3-1** | Event streaming + tagged enums | Add `Incoming` enum, `read_incoming` method, the response/event distinction. Concept focus: Rust enums vs TS unions, pattern matching, `Box<dyn Future>` if it comes up. |
| **M3-2** | The DAP launch dance | Build a hello-world C binary, send `launch`, observe events, `disconnect`. Concept focus: the asymmetry of "send launch, don't wait, listen for `initialized` event" — why DAP works this way. |

---

## M4 — Pause on breakpoint → 2 sessions

[`docs/implementation/tasks/M04-pause-on-breakpoint.md`](../implementation/tasks/M04-pause-on-breakpoint.md)

| Session | Concept | What we do |
|---|---|---|
| **M4-1** | The full handshake order | initialize → launch (don't await) → wait for initialized event → setBreakpoints → configurationDone → events flow. Concept focus: why this order matters, what happens if you skip configurationDone. |
| **M4-2** | First real breakpoint | Stop on a known line, observe the `stopped` event, send `continue`, watch the program complete. Concept focus: the conceptual moment "we have a debugger" lands here. Capture in obsidian. |

End of Phase A. Stop point worth flagging — celebrate before Phase B.

---

## M5 — IPC protocol + daemon binary → 5 sessions

[`docs/implementation/tasks/M05-ipc-protocol-daemon.md`](../implementation/tasks/M05-ipc-protocol-daemon.md)

This is the biggest milestone. Five sessions minimum.

| Session | Concept | What we do |
|---|---|---|
| **M5-1** | The protocol crate + IpcMessage envelope | Create `crates/protocol`, define `IpcMessage`, `IpcPayload`, `Request`, `Response` (just `Ping`/`Pong` for now). Concept focus: enum-as-message-type pattern, serde tagging strategies, why a dedicated protocol crate (boundary discipline). |
| **TDD-1** | Test-driven development (the dedicated meta-session) | The retroactive explanation. Up to this point smoke tests have been teacher-written and treated as deferred-load (rule 8). This session walks the existing tests for chapters 05+ together, explains `#[tokio::test]`, fixture data, the Implementation Swap Test, and behaviour-vs-implementation testing. Concept focus: tests as a design tool *and* a verification tool, lifted from the project's `tdd` skill. After this session, M5-2 onward is test-driven by default. |
| **M5-2** | Length-prefixed JSON codec | Build `read_message` / `write_message` in `crates/protocol/src/codec.rs` **test-first** (now that TDD-1 has landed). Concept focus: `read_exact` vs `read`, big-endian vs little-endian, the cancellation-safety footgun in async reads. The codec is a perfect TDD candidate — pure functions, fixture bytes in / typed message out. |
| **M5-3** | Unix sockets + accept loop | `crates/daemon/src/server.rs` with `UnixListener::bind`, accept loop, `tokio::spawn` per client. Concept focus: how Unix sockets differ from TCP, file permissions on sockets, why we spawn a task per connection. |
| **M5-4** | Auto-spawning daemon | The `ensure_daemon_running` dance: probe socket → fork daemon → poll for socket. Concept focus: re-execing the binary, detaching from parent, the PID-file + flock pattern, the "client probes before doing anything" workflow. |
| **M5-5** | Wire `lazydap launch` end-to-end | The first real subcommand. CLI side (`crates/daemon/src/cli/launch.rs`) → IPC client → daemon handler → DAP transport from Phase A → response back. Concept focus: this is the moment the architecture from blueprint becomes real code. |

Pause after M5-5. Major moment — the daemon-backed CLI exists.

---

## M6 — CLI subcommands → 4 sessions

[`docs/implementation/tasks/M06-cli-subcommands.md`](../implementation/tasks/M06-cli-subcommands.md)

| Session | Concept | What we do |
|---|---|---|
| **M6-1** | Stepping commands (no `--wait`) | `continue`, `step`, `step-into`, `step-out`, `pause`. Fire-and-forget versions. Concept focus: how clap subcommands compose, the IPC dispatch pattern, why stepping commands are fire-and-forget by default. |
| **M6-2** | The `--wait` design | The hardest design in lazydap. Build the wait loop: receive DAP events, buffer output/breakpoint/thread events, return on stopped/exited/terminated/timeout. Concept focus: `tokio::select!`, `tokio::sync::broadcast`, the coalescing window, why timeouts must be configurable. |
| **M6-3** | Inspection commands | `stack`, `scopes`, `variables`, `eval`. Concept focus: lazy variable expansion (`variables_reference`), the read-only vs mutating split, what an "inspection" implies for stable state. |
| **M6-4** | Persistent breakpoints | `break add/list/remove/toggle` + `crates/store` for `.lazydap/state.toml` reads/writes. Concept focus: TOML serialisation with serde, debounced disk writes, the BreakpointId vs adapter id distinction. |

Diagnostics commands (`status`, `logs`, `disconnect`) wrap up at the end of M6-4 if there's energy; otherwise they're cheap one-evening additions later.

---

## M7 — Skill + agent verification → 1 session

[`docs/implementation/tasks/M07-skill-agent-verification.md`](../implementation/tasks/M07-skill-agent-verification.md)

| Session | Concept | What we do |
|---|---|---|
| **M7-1** | Build the skill ZIP, verify with Claude Code | Hand-write `SKILL.md` + auto-generate `references/commands.md` from clap. ZIP. Test conversation. Concept focus: how the agent skill differs from the human CLI (it's the same surface; the skill is just docs), what "agent-native" means in practice. |

End of Phase B. Stop point — the CLI debugger ships, agents can drive it.

---

## M8 — Hello ratatui → 1 session

[`docs/implementation/tasks/M08-hello-ratatui.md`](../implementation/tasks/M08-hello-ratatui.md)

| Session | Concept | What we do |
|---|---|---|
| **M8-1** | ratatui's draw loop | Empty TUI, "lazydap" centred, `q` quits. Concept focus: the immediate-mode rendering model, why ratatui re-draws every frame, raw mode + alternate screen ritual. |

---

## M9 — Show a file → 1 session

[`docs/implementation/tasks/M09-show-a-file.md`](../implementation/tasks/M09-show-a-file.md)

| Session | Concept | What we do |
|---|---|---|
| **M9-1** | Source pane + scrolling | Render a hardcoded file with line numbers. j/k cursor movement. Concept focus: ratatui layouts, `Paragraph` vs `Block`, computing scroll offset from cursor position. |

---

## M10 — Elm-ify the loop → 2 sessions

[`docs/implementation/tasks/M10-elm-ify.md`](../implementation/tasks/M10-elm-ify.md)

The load-bearing pivot. Don't rush.

| Session | Concept | What we do |
|---|---|---|
| **M10-1** | Define Model / Msg / Cmd | Refactor M9's state into `AppState`. Define `enum Msg`, `enum Cmd`. Write the empty `update(state, msg) -> (state, cmd)` skeleton. Concept focus: TEA's three-types pattern, why mutation is restricted to `update`, anchor to React's `useReducer` from the user's prior knowledge. |
| **M10-2** | Wire the main loop | Refactor the main loop to be `update`-driven. Channels for input + tick. Pure `view` function. Concept focus: `tokio::select!` over input + tick, why the loop reads channels and dispatches via `update`, the "side effects via Cmd" pattern. |

Reference: [[The Elm Architecture (TEA)]] in Obsidian. Read it together at the start of M10-1.

---

## M11 — Wire IPC into TUI → 3 sessions

[`docs/implementation/tasks/M11-wire-ipc-into-tui.md`](../implementation/tasks/M11-wire-ipc-into-tui.md)

| Session | Concept | What we do |
|---|---|---|
| **M11-1** | IPC client + Subscribe | Build `crates/tui/src/ipc_client.rs`. Connect to daemon, send `Subscribe { channels: [Stopped, Output, ...] }`, route incoming events into the input channel as `Msg::DaemonEvent`. Concept focus: how the TUI is just another client of the daemon, the broadcast subscription model. |
| **M11-2** | Stepping commands wired | Extend `update` to handle F5/F10/F11 → produce `Cmd::SendIpc(Request::Continue/Step)`. Concept focus: how new keybindings become two-line additions to the match (the discipline payoff of M10). |
| **M11-3** | Source pane shows current line | On `Stopped` event, fetch top frame, set `current_line`, render the `→` marker. Concept focus: the daemon-event-to-UI-state pipeline end-to-end. **lazydap v0.1-prerelease lands here.** |

Stop point. Big moment.

---

## M12 — Stack pane → 1 session

[`docs/implementation/tasks/M12-stack-pane.md`](../implementation/tasks/M12-stack-pane.md)

| Session | Concept | What we do |
|---|---|---|
| **M12-1** | Stack pane + frame nav | Add `StackView`, render frame list, `<CR>` jumps source pane to selected frame. Concept focus: the IPC fetch-on-event pattern (Stopped → fetch full stack), pane focus management. |

---

## M13 — Scopes pane with expansion → 2 sessions

[`docs/implementation/tasks/M13-scopes-pane.md`](../implementation/tasks/M13-scopes-pane.md)

| Session | Concept | What we do |
|---|---|---|
| **M13-1** | Render the scope tree | Static tree (locals/arguments/globals), no expansion yet. Concept focus: tree rendering in ratatui, `ScopePath` indexing. |
| **M13-2** | Lazy expand on `<CR>` | The `variables_reference` correlation pattern. Send `Variables` request, receive children, populate the node, re-render. Concept focus: request/response correlation by request id, the "loaded vs expanded" state distinction, why we don't pre-fetch. |

---

## M14 — Toggle breakpoint → 1 session

[`docs/implementation/tasks/M14-toggle-breakpoint.md`](../implementation/tasks/M14-toggle-breakpoint.md)

| Session | Concept | What we do |
|---|---|---|
| **M14-1** | `b` toggles breakpoint | Source pane + persistent state. Concept focus: the verified vs unverified distinction (sign goes from ◯ to ●), the adapter's `setBreakpoints` "replaces all in file" semantic. |

---

## M15 — Config + launch.json + release → 3 sessions

[`docs/implementation/tasks/M15-config-file.md`](../implementation/tasks/M15-config-file.md)

| Session | Concept | What we do |
|---|---|---|
| **M15-1** | Config crate + global config.toml | `crates/config/src/lib.rs`. Read `~/.config/lazydap/config.toml`. XDG paths. Defaults. Concept focus: how `config-rs`-style merging works, why we don't use the `config` crate (one less dep), env var overrides. |
| **M15-2** | `.vscode/launch.json` import | Parse JSON-with-comments, substitute `${workspaceFolder}` and friends, surface as launch configs. Concept focus: how to handle a foreign format gracefully (warn on unknown variables, don't silently substitute empty), the JSON-with-comments cleanup pass. |
| **M15-3** | Release prep + ship v0.1 | LICENSE, CHANGELOG, README, CI publish workflow, `cargo publish` order, tag `v0.1.0`. Concept focus: the cargo-publish dependency-order dance (publish leafs first, daemon last), `release-please` setup, the GIF demo. **Public release.** |

Major stop point. Celebrate. Then Phase E.

---

## M16 — Watches → 1 session

[`docs/implementation/tasks/M16-watches.md`](../implementation/tasks/M16-watches.md)

| Session | Concept | What we do |
|---|---|---|
| **M16-1** | Watches pane + persist | Add expression, evaluated on each pause via `Eval`. Persist in state.toml. Concept focus: the modal pattern (first time we have one), per-pause re-evaluation. |

---

## M17 — REPL pane → 1 session

[`docs/implementation/tasks/M17-repl-pane.md`](../implementation/tasks/M17-repl-pane.md)

| Session | Concept | What we do |
|---|---|---|
| **M17-1** | REPL pane | Type expression, submit, history. Concept focus: command history pattern, the difference between watch-context and repl-context evaluations. |

---

## M18 — Second adapter → 2 sessions

[`docs/implementation/tasks/M18-second-adapter.md`](../implementation/tasks/M18-second-adapter.md)

| Session | Concept | What we do |
|---|---|---|
| **M18-1** | debugpy adapter crate | `crates/adapter-debugpy/`. Implement `DebugAdapter` trait. Concept focus: trait implementation patterns, where the codelldb assumptions were hidden (the lessons surface as you do this — collect them). |
| **M18-2** | Adapter routing + auto-detect | Daemon dispatches by `AdapterKind`. `lazydap launch foo.py` auto-picks debugpy. Concept focus: the adapter discovery chain, why filetype-based detection is best-effort not authoritative. |

End of Phase E. **Multi-language unlock.**

---

## Counts

| Phase | Milestones | Sessions |
|---|---|---|
| Workspace setup | 1 | 3 |
| Phase A (M0–M4) | 5 | 8 |
| Phase B (M5–M7) | 3 | 10 |
| Phase C (M8–M11) | 4 | 7 |
| Phase D (M12–M15) | 4 | 7 |
| Phase E (M16–M18) | 3 | 4 |
| **Total** | **20** | **39** |

At ~1.5 hours per session average, that's ~58 hours of teaching time across the whole project. Two evenings a week for six months. Realistic.

## When to deviate from this plan

- **A session feels light** — extend it. Combine with the next one if there's energy and the concepts are related.
- **A session feels heavy** — split it. Add a row to the table. The plan is meant to evolve.
- **A new concept comes up that wasn't planned** — capture it. Make it its own session if substantive; fold into an existing session if minor.
- **Reality diverges from the plan** — update the plan. Don't quietly skip ahead.

## See also

- [`README.md`](README.md) — what this directory is
- [`/AGENTS.md`](../../AGENTS.md) — teaching mode setup for this project
- [`docs/implementation/`](../implementation/) — the underlying milestone tasks (ship-mode-ready)
- [`/docs/blueprint/`](../blueprint/) — the full project vision (recenter when lost)
- Obsidian: `Lazydap Teaching Sessions.md` — where actual sessions get logged
- Obsidian: `Teaching Senior Engineers.md` — the pedagogy synthesis
