# 00 — Overview

## What lazydap is

lazydap is a scriptable, terminal-first debugger. It wraps any [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/) (DAP) adapter — codelldb, debugpy, js-debug, delve, etc. — behind a single CLI binary that exposes every operation as a subcommand returning JSON.

It's three things in one shape:

1. A **CLI** you can drive from any language.
2. A **TUI** that's a thin client over that CLI.
3. An **agent skill** so AI tools can debug code via shell commands instead of bespoke MCP servers.

The CLI is the product. The TUI and skill are clients. So is anything anyone builds on top of the protocol — Electron apps, web dashboards, vim plugins, MCP servers, language bindings.

## The thesis

Debugging is the last terminal-first developer task without a `lazy*`-quality interface, and it's the one AI agents care about most when they hit runtime questions. Existing options are:

- **MCP-tied** (every recent project) — works in Claude Code or Cursor, doesn't help shell scripts, CI, or tools that don't run an MCP host.
- **VS Code-tied** (VS Code Debug Agent, GitHub Copilot Spaces) — requires an editor to be running.
- **DAP-tied** (any direct DAP wrapper) — useful if you already speak DAP, miserable if you don't.
- **Print-debug-loop** (Cursor Debug Mode, Replit Agent) — instruments code with logs, doesn't actually step.

None expose debugging as plain shell subcommands an agent can invoke via Bash. lazydap fills that.

(See [`11-state-of-the-art.md`](11-state-of-the-art.md) for the full competitive analysis.)

## Who lazydap is for

In rough priority order:

1. **Terminal-first developers** who edit in vim/nvim/Helix and don't want to context-switch to VS Code or CLion to step through code.
2. **AI agents** (Claude Code, Cursor, Copilot, custom agents) that need to inspect runtime state but can't or shouldn't depend on MCP transports.
3. **CI / scripts** that want to assert "this binary, given this input, exits at line X with variable Y in this state."
4. **Builders** who want to wrap a debugger in their own UX — Electron app, web tool, custom DSL.

lazydap is **not** for: people who already love their IDE debugger and are happy. Use what works for you.

### The segment that actually needs this (strategic anchor, 2026-07-31)

The list above is *who by interface*. This is *who by need*, and it is the sharper cut — it
decides roadmap priority and positioning.

Most developers ship most days without a debugger, and their agents can too: a print
statement and a rerun genuinely cover a lot of work. lazydap would be *useful* to all of
them; it is *aimed* at the minority for whom that loop breaks down and the work turns painful
without a debugger:

- **Manual memory management** — C, C++, Zig, unsafe Rust. Use-after-free and corruption,
  where the crash site is not the bug site and the crash destroys its own evidence.
- **The FFI boundary** — Python or Node calling a native extension. The segfault lives
  *beneath* the interpreter; a print statement cannot cross the language boundary. lazydap's
  codelldb + debugpy pairing serves exactly this seam.
- **Races and timing** — bugs that adding a print statement makes disappear, because the
  print changes the scheduling. Breakpoints and watchpoints perturb less.
- **Release-only bugs** — where a printf costs a full rebuild and a breakpoint does not.
- **Crash forensics** — the process is dying; the state must be read at the moment of death,
  not logged before it.

The strategic consequence: when a feature serves the checklist but not this segment, the
segment wins. `attach` (debugging a live, misbehaving native process) serves it more than a
fourth GC'd-language adapter does, and that is why `attach` precedes js-debug on the roadmap.
The competitive audit ([`docs/research/2026-07-31-agent-debugger-landscape.md`](../research/2026-07-31-agent-debugger-landscape.md))
found no one in the convergence cluster has `attach` at all.

### Why the segment is the beachhead and not the ceiling (2026-07-31)

The segment above is *who wins unconditionally*. It is not the whole market, and the reason
is friction, not need. Most developers skip the debugger not because a print statement tells
them more, but because the debugger costs ceremony a print does not: a launch config,
breakpoints set by hand, the commands remembered, the context-switch out of the editor. print
costs one line where you already are.

Driven by an agent, that ceremony collapses. The human action becomes *ask*, and at that
price the debugger wins the comparison it used to lose: one `continue --wait` returns strictly
more than a print — stop reason, frame, locals, and every line the program emitted — for the
same one sentence. Log points are the sharpest proof: `break --log "x={x}"` is the print
workflow itself, with none of print's cost (no edit, no rebuild, structured output, works on
a release binary). lazydap does print-debugging better *and* adds the breakpoint workflow on
top, from the same ask.

Two disciplines keep this from becoming a generic pitch:

1. **It is not free, and the honest claim is stronger for admitting it.** The remaining costs
   print does not have are latency (spawn daemon, adapter, session), tokens (the agent runs
   several commands), and **install** (the adapter is still a setup step; print needs none).
   The claim is *same effort, strictly more information*, not *no cost*. Install is the one
   friction lazydap has not yet removed — which is why distribution (`brew`, `install.sh`,
   M21) is on the critical path, not a nicety.
2. **Lead with the segment, not with friction.** "Frictionless debugging for everyone" fails
   the differentiation test — no competitor claims they make debugging hard, and Cursor
   undercuts it directly by removing the debugger entirely (log-instrumentation, even less
   friction). Friction-removal is only defensible when it is concrete: *one sentence in, one
   complete answer out, no polling, no MCP host* — which is the wait-blob contract wearing a
   friction label. So: the segment is the beachhead that converts without persuasion; the
   friction collapse is the wedge that pulls the adjacent market behind it.

## Principles

These guide every decision. Ordered by precedence — when they conflict, top wins.

### 1. Scriptability before features

Every operation goes through a single JSON-over-Unix-socket protocol. The CLI is the canonical client. We don't add features that bypass it. This is enforced by the crate-dependency graph, not just by convention. (See [`01-architecture.md`](01-architecture.md).)

### 2. Wrap, don't replace

We wrap DAP adapters. We don't write a debugger from scratch. The hard parts (DWARF parsing, ptrace, symbol resolution, language-specific quirks) live in adapters that already work. lazydap is a translation layer with good UX.

### 3. The TUI is a client, not a peer

The TUI doesn't get special features. It uses the same protocol the agent skill uses. If the TUI needs information the protocol doesn't expose, the protocol is wrong, not the TUI.

### 4. Honest about state

A debugger has live state. We don't pretend it's stateless. We don't pretend it's queryable like git. We expose explicit session lifecycle and explicit "wait for next stable state" semantics. (See [`10-async-to-sync.md`](10-async-to-sync.md).)

### 5. Local-first

The daemon runs locally. Sessions are local. Adapters are local. There's no "lazydap cloud." If someone wants to expose a session over the network, they write a bridge using the protocol. We won't ship that bridge in core.

### 6. Two unknowns max in flight at any milestone

Building a debugger is hard enough. Don't compound it with new frameworks, new languages, new architectures all at once. Each milestone introduces at most two novel concepts. (See [`14-roadmap.md`](14-roadmap.md) for how this shapes the milestone plan.)

## Scope — in and out

### In scope (v0.1)

- Single concurrent debug session per daemon (per project).
- One adapter: **codelldb** (covers C, C++, Rust).
- Stepping (continue, step over, step in, step out), breakpoints, stack/scopes/variables, evaluate, REPL.
- TOML state file (`.lazydap/state.toml`) for persistent breakpoints and watches per project.
- TOML config (`~/.config/lazydap/config.toml`) for global preferences.
- `.vscode/launch.json` import (read-only) for project-local launch configs.
- Agent skill (`lazydap.skill`) with full CLI reference.
- TUI with source pane, stack pane, scopes pane, REPL pane, basic keybindings.
- `--wait` pattern for synchronous agent invocation.
- JSON output, auto-detected from TTY status.

### Out of scope for v0.1, planned post-v0.1

- Multi-session per daemon (semantics designed in from M5; UI/CLI lifts the constraint later).
- Additional adapters: debugpy, js-debug, delve.
- Conditional breakpoints, log points, exception filters.
- Restart / disconnect-and-relaunch.
- Mouse support, theming.
- HTTP bridge / WebSocket transport (separate optional binary).
- AI advisor integration points (see [`12-ai-future.md`](12-ai-future.md)).

### Out of scope, period

- Writing our own debugger. Use existing adapters.
- Cross-machine debug sessions. lazydap is local-first.
- Full GDB/LLDB UI parity. We do the 90% case fast, not the 100% case.

## Comparison to mxr

This project explicitly inherits architecture from [mxr](https://github.com/planetaryescape/mxr) — a daemon-backed CLI email client by the same author. The patterns are the same:

| mxr | lazydap |
|---|---|
| Single binary, subcommand-first | Same |
| Auto-spawning daemon, Unix socket IPC | Same |
| Length-delimited JSON protocol | Same |
| Strict crate boundaries enforced by Cargo | Same |
| CLI-first culture, every TUI action has CLI equivalent | Same |
| `--format json|table|csv|ids`, auto-detected | Same |
| `--dry-run` + `--yes` for mutations | Same |
| `tracing` from start | Same |
| `.skill` ZIP with `SKILL.md` + `references/commands.md` | Same |

Domain-specific differences:

| mxr | lazydap |
|---|---|
| SQLite as canonical state | TOML files (state is small + scriptable) |
| Tantivy search (BM25 over messages) | None |
| Email accounts, sync providers | DAP adapters (codelldb, debugpy, ...) |
| Sync loops + delta cursors | Push-based DAP events; daemon broadcasts |
| Mostly poll-based | Mostly push-based; `--wait` bridges to sync |

Things mxr learned the hard way that lazydap inherits free:

- "Test with real adapters, not mocks" — see [`docs/articles/the-cli-is-the-product.md`](../articles/the-cli-is-the-product.md) (TODO).
- "Crate boundaries enforced by Cargo, not convention" — violations always come back.
- "The daemon serves reusable truth and workflows, not screen payloads" — keeps the protocol stable across clients.

## What success looks like

After [M11](../implementation/tasks/M11-wire-ipc-into-tui.md):

- A user can `lazydap launch ./mybinary`, set breakpoints from the TUI, step through, inspect variables, and quit cleanly.
- An agent (Claude Code, Cursor, etc.) can do the same via shell commands.
- A third-party can write a 100-line Python script that runs a debug session end-to-end.

After [M15](../implementation/tasks/M15-config-file.md) (v0.1):

- `cargo install lazydap` works.
- A `.vscode/launch.json` works out of the box for projects that have one.
- Public release with a real README, GIF demo, install guide.

After [M18](../implementation/tasks/M18-second-adapter.md):

- Multi-language: Python via debugpy works. Then Go, then Node/TS.

## What success doesn't look like

- "Replaces VS Code." It doesn't. Don't try.
- "Best debugger ever." It's a CLI debugger that happens to also have a TUI and an agent skill. Be useful, not maximal.
- "Built-in AI." AI features are external clients of the protocol. We ship the protocol; others ship the AI. (See [`12-ai-future.md`](12-ai-future.md).)

## Further reading

- [`01-architecture.md`](01-architecture.md) — full architecture
- [`14-roadmap.md`](14-roadmap.md) — phased delivery
- [`15-decision-log.md`](15-decision-log.md) — every design decision and why
- [`11-state-of-the-art.md`](11-state-of-the-art.md) — what exists today, where the gap is
- [`docs/articles/yes-its-a-wrapper.md`](../articles/yes-its-a-wrapper.md) — the "isn't this just a wrapper on DAP?" question, answered
