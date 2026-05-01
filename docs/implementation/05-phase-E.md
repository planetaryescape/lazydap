# 05 — Phase E: beyond v0.1

**Goal:** turn lazydap from "useful for the niche it covers" into "useful daily."

## Milestones

- **[M16 — Watches](tasks/M16-watches.md)** — watches pane. `a` to add expression. Re-evaluated on each pause.
- **[M17 — REPL pane](tasks/M17-repl-pane.md)** — bottom split. Type expressions, see results, history.
- **[M18 — Second adapter](tasks/M18-second-adapter.md)** — debugpy. Debug Python. Multi-language unlock.

## What you'll have at the end

- Watches across sessions (persisted in `.lazydap/state.toml`).
- REPL with history.
- Two working adapters (codelldb + debugpy).
- `lazydap launch` auto-detects adapter from program type when reasonable.

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

- All M16–M18 boxes ticked.
- Both codelldb and debugpy work end-to-end.
- Watches survive session boundaries.
- REPL is usable for the things `lazydap eval` is usable for.

After Phase E, the project exits structured-milestone mode. Future work tracked as issues / addenda; the architecture is stable; new contributors can find their footing.
