# 14 — Roadmap

## Phases at a glance

| Phase | What | Milestones | Calendar |
|---|---|---|---|
| **A** — see the protocol | Raw DAP plumbing, no UI, no daemon | M0–M4 | 1–2 weeks |
| **B** — daemon + protocol | lazydap IPC, CLI subcommands, agent skill | M5–M7 | 2–3 weeks |
| **C** — TUI | ratatui shell, Elm-ified, IPC-wired | M8–M11 | 2–4 weeks |
| **D** — useful features | Stack, scopes, breakpoint UI, config | M12–M15 → **v0.1** | 4–6 weeks |
| **E** — beyond v0.1 | Watches, REPL, second adapter | M16–M18 | ongoing |

## Build philosophy

Skateboard → bike → car. Each milestone is a **runnable program that does something end-to-end** — not "step 3 of 12 toward something useful." Stop after any milestone and there's a working artifact. Reward loop short.

**Two unknowns max in flight at any time.** If a milestone needs three new concepts at once, split it. M0 has exactly one new concept (spawn a child process and read its stdout). That's the right size.

## Phase A — see the protocol (M0–M4)

**Goal:** understand DAP by talking to it. No daemon, no UI, no abstraction. One small binary per milestone.

- **[M0](../implementation/tasks/M00-hello-adapter.md)** — Spawn `codelldb`. Read raw stdout. Print. Exit.
- **[M1](../implementation/tasks/M01-read-one-message.md)** — Parse `Content-Length` framing, decode one JSON message, pretty-print.
- **[M2](../implementation/tasks/M02-initialize-handshake.md)** — Send `initialize`, parse response, print capabilities.
- **[M3](../implementation/tasks/M03-launch-and-observe.md)** — Send `launch` for a hello-world C binary, stream events for 5s.
- **[M4](../implementation/tasks/M04-pause-on-breakpoint.md)** — Add `setBreakpoints` + `configurationDone`. Observe `stopped` event.

**Win after Phase A:** you've seen real DAP traffic, set a breakpoint, hit it. You understand what the daemon needs to do.

## Phase B — daemon + protocol (M5–M7)

**Goal:** wrap the DAP plumbing in a daemon, expose it through a JSON-over-Unix-socket protocol, build CLI subcommands. Test with an agent skill.

- **[M5](../implementation/tasks/M05-ipc-protocol-daemon.md)** — Define lazydap protocol types. Daemon binary that holds a session and accepts one IPC client. Single subcommand: `lazydap launch ./bin`.
- **[M6](../implementation/tasks/M06-cli-subcommands.md)** — Full CLI surface: `break`, `continue`, `step`, `stack`, `scopes`, `eval`, `status`. All talking to daemon via IPC. `--wait` semantics implemented.
- **[M7](../implementation/tasks/M07-skill-agent-verification.md)** — Build `lazydap.skill` ZIP. Test end-to-end with Claude Code: agent reads skill, drives a debug session, reports findings.

**Win after Phase B:** you have a working CLI debugger and a working agent debugger. No TUI yet. The CLI alone is useful and shippable.

## Phase C — TUI (M8–M11)

**Goal:** add the TUI as a second client. Keep it strictly a client of the same protocol.

- **[M8](../implementation/tasks/M08-hello-ratatui.md)** — Hello ratatui. No DAP. Just learn the render loop.
- **[M9](../implementation/tasks/M09-show-a-file.md)** — Show a hardcoded source file in ratatui. Still no DAP.
- **[M10](../implementation/tasks/M10-elm-ify.md)** — Refactor M9 into Model/Msg/update/view. **Don't skip this.** The Elm shape is what keeps the rest of the project from collapsing.
- **[M11](../implementation/tasks/M11-wire-ipc-into-tui.md)** — TUI subscribes to daemon events over IPC. Keypresses send IPC requests. Source pane shows current line.

**Win after Phase C:** lazydap v0.1-prerelease. TUI works. Same protocol as the CLI.

## Phase D — useful features → v0.1 (M12–M15)

**Goal:** ship something people want to install.

- **[M12](../implementation/tasks/M12-stack-pane.md)** — Stack pane in TUI. `<CR>` jumps to frame.
- **[M13](../implementation/tasks/M13-scopes-pane.md)** — Scopes pane with expand-on-`<CR>`.
- **[M14](../implementation/tasks/M14-toggle-breakpoint.md)** — Press `b` in TUI to toggle breakpoint. Sign in gutter.
- **[M15](../implementation/tasks/M15-config-file.md)** — Config file, `.vscode/launch.json` import. Tag v0.1. `cargo install lazydap`.

**Win after Phase D:** public release.

## Phase E — beyond v0.1 (M16–M18)

**Goal:** make it actually live with daily use.

- **[M16](../implementation/tasks/M16-watches.md)** — Watches.
- **[M17](../implementation/tasks/M17-repl-pane.md)** — REPL pane.
- **[M18](../implementation/tasks/M18-second-adapter.md)** — Second adapter (debugpy → Python).

## Stop points worth flagging

Each of these is a public-facing artifact you could ship and stop:

- After **M5**: working CLI debugger (single subcommand, just `launch`).
- After **M7**: agent-driven CLI debugger. Full skill. Real, shippable.
- After **M11**: TUI debugger. Genuinely lazydap.
- After **M15**: public v0.1 on crates.io.
- After **M18**: multi-language tool.

Don't skip M10. That single weekend of "rewrite into Elm shape *before* adding DAP messages" is what keeps the rest of the project from spaghetti. Adding IPC into a non-Elm-structured TUI is exactly how lazygit/lazydocker forks die in their authors' WIP folders.

## Time estimates

Calendar time, evenings + weekends:

| Phase | Time |
|---|---|
| A — see the protocol (M0–M4) | 1–2 weeks |
| B — CLI debugger + skill (M5–M7) | 2–3 weeks |
| C — TUI shell (M8–M11) | 2–4 weeks |
| D — v0.1 features (M12–M15) | 4–6 weeks |
| E — beyond v0.1 (M16–M18) | ongoing |

Total to public v0.1: **~3 months**.

Realistic, not pessimistic. Every edge case (adapter crashes, optimised-out variables, source not found, multi-threaded targets) is its own small project.

## Future vision (post-M18)

The roadmap above gets to "useful debugger." After that, the interesting questions:

### Multi-session

Designed for from M5 (every IPC message has a session ID), enforced as N=1 in v0.1. Lifting the constraint:

- Daemon manages a `HashMap<SessionId, Session>`.
- CLI subcommands gain `--session <id>` flag.
- TUI gets a session-picker pane.
- New IPC: `Session::List`, `Session::Switch`.

This is mostly daemon work; the protocol already handles it.

### More adapters

Each adapter is a new crate (`adapter-XYZ`) implementing `DebugAdapter`. Add support for:

- **delve** (Go)
- **js-debug** (Node/TS)
- **lldb-dap** (alternative C/C++/Rust adapter, ships with LLVM)
- **dap-mode** (Java)
- **netcoredbg** (.NET)

Adapter quirks (Gmail-vs-IMAP-style differences in mxr) stay in the adapter crates.

### HTTP / WebSocket bridge

A separate optional binary (`lazydap-bridge`) that translates the Unix socket protocol to HTTP/WebSocket. Lets browsers talk to lazydap. Lives outside the main binary so it's optional.

### AI advisor extension points

Two primitives, both already in the design:

1. **Streaming events API** (`Subscribe { channels: [...] }`) — anything observing a session uses this.
2. **`getStateSnapshot` command** — returns rich JSON: current frame, locals (recursive, depth-limited), recent step history (ring buffer), source slice, breakpoints, watch values. One call, one structured payload.

These two enable third parties to build:

- "Why did this break?" panels
- Stack trace summarisation
- Auto-watch suggestion
- Test generation from paused state
- Patch suggestion

We don't ship those features in core. We ship the primitives. (See [`12-ai-future.md`](12-ai-future.md).)

### MCP server (separate crate)

Once the CLI surface is stable, wrapping it as an MCP server is a weekend's work. We don't build the MCP server inside the daemon — it's a thin separate crate that shells out. This way MCP is one of N possible bridges, not the architecture.

## What's deliberately *not* on this roadmap

- "Rewrite the debugger" → use adapters, never write our own.
- "Replace VS Code" → not a goal.
- "Cloud sync of debug sessions" → not a goal.
- "Visual flowchart of execution" → out of scope; bolt on later if it makes sense.
- "Replay-style time travel" → out of scope; depends on adapter support, hard problem.

## When this roadmap changes

- After every milestone, re-read this doc. Update if reality has diverged.
- After every phase, write a `docs/blueprint/16-addendum-N.md` if the experience changed the plan.
- Major direction changes get a `15-decision-log.md` entry.
