# 05 — Phase E: beyond v0.1

**Goal:** turn lazydap from "useful for the niche it covers" into "useful daily."

## Milestones

- **[M16 — Watches](tasks/M16-watches.md)** — watches pane. `a` to add expression. Re-evaluated on each pause.
- **[M17 — REPL pane](tasks/M17-repl-pane.md)** — bottom split. Type expressions, see results, history.
- **[M18 — Second adapter](tasks/M18-second-adapter.md)** — debugpy. Debug Python. Multi-language unlock.
- **[M19 — TUI reconnect](tasks/M19-tui-reconnect.md)** — a daemon that goes away leaves the TUI reconnecting, not frozen. Added during Phase D; filed here because it landed after M15.
- **[M20 — Documentation website](tasks/M20-docs-site.md)** — `site/`: Astro + Starlight, pages generated from the binary with a drift-failing check.
- **[M21 — Packaging](tasks/M21-packaging.md)** — `install.sh` and a Homebrew tap, the two channels the ship-it checklist recorded as missing.
- **[M22 — Third adapter](tasks/M22-delve-adapter.md)** — delve. Debug Go.
- **[M23 — Fourth adapter](tasks/M23-jsdebug-adapter.md)** — js-debug for Node. **Blocked at its own scope gate:** the parent session debugs nothing, so it needs a child-session milestone first.
- **[M24 — Attach to a running process](tasks/M24-attach.md)** — chosen over M23 on 2026-07-31. The next milestone.

## What you'll have at the end

- Watches across sessions (persisted in `.lazydap/state.toml`).
- REPL with history.
- Three working adapters (codelldb + debugpy + delve).
- `lazydap launch` auto-detects the adapter from the program's extension, with `--adapter` to override.

## Phase-level concepts

### The second adapter is the real test

When you go from one adapter to two, you discover what was hardcoded that shouldn't have been. Every place lazydap assumed "codelldb" or "C/C++/Rust" gets exercised.

This is a feature. M18 will reveal architectural mistakes that v0.1 hid. Fix them as you discover them.

### REPL design constraints

The REPL is bound by what `evaluate` returns from the adapter. codelldb's `expressions: "simple"` mode is forgiving; `expressions: "native"` lets users write raw LLDB. We expose both via a config toggle.

debugpy's eval is full Python in the paused process. Different vocabulary, similar UX.

### Watches semantics

Watches re-evaluate on each pause. The result is cached until the next pause. If `evaluate` errors (variable out of scope, expression invalid), the watch shows the error, not stale data.

## Risks specific to Phase E

- **Adapter quirks pile up.** debugpy will surface things codelldb hid. (See [`/docs/blueprint/03-adapters.md`](../blueprint/03-adapters.md).)
- **Watches across sessions.** Persisted by expression text, not by adapter ID. If the user switches projects, watches still show — and likely error. Make the UX clear: errored watches are dimmed.
- **REPL history.** Cross-session? Per-session? Default per-session; configurable to persist.

## Phase E is done when

- All M16–M24 boxes ticked.
- codelldb, debugpy and delve all work end-to-end.
- Watches survive session boundaries.
- REPL is usable for the things `lazydap eval` is usable for.

After Phase E, the project exits structured-milestone mode. Future work tracked as issues / addenda; the architecture is stable; new contributors can find their footing.
