# Implementation

Phased delivery of lazydap. Each phase is a directory's worth of milestones; each milestone is a self-contained MD in [`tasks/`](tasks/).

## Phases

- **[00 — Workspace setup](00-workspace-setup.md)** — Cargo workspace skeleton, CI, conventions. Done before M0.
- **[01 — Phase A: see the protocol](01-phase-A.md)** — Raw DAP plumbing. M0–M4.
- **[02 — Phase B: daemon + protocol](02-phase-B.md)** — IPC core, CLI subcommands, agent skill. M5–M7.
- **[03 — Phase C: TUI](03-phase-C.md)** — ratatui shell, Elm-ified, IPC-wired. M8–M11.
- **[04 — Phase D: useful features → v0.1](04-phase-D.md)** — Stack pane, scopes, breakpoint UI, config. M12–M15.
- **[05 — Phase E: beyond v0.1](05-phase-E.md)** — Watches, REPL, second adapter. M16–M18.

## How to use these docs

Each phase doc:

- Sets the phase goal in one paragraph
- Lists the milestones it contains, with one-line summaries
- Notes phase-level dependencies and shared context
- Calls out cross-cutting risks and decisions

Each milestone (`tasks/MNN-*.md`) is self-contained:

- **What** — concrete outcome
- **Why** — how it advances lazydap
- **How** — implementation steps
- **Success criteria** — how to know you're done
- **Files** — what gets created or touched
- **Verify** — how to test it
- **Depends on** — which prior milestones must be complete

You should be able to read just one milestone file and start work. The phase docs exist to give context; the milestone files are the actual instructions.

## Working through a milestone

1. Open the milestone file (e.g. [`tasks/M00-hello-adapter.md`](tasks/M00-hello-adapter.md)).
2. Confirm dependencies are done.
3. Read the "How" section start to finish.
4. Write code. Use the success criteria as your acceptance test.
5. Update [`/TODO.md`](../../TODO.md) — check the box.
6. Commit. Move to next.

## When a milestone reveals the plan was wrong

Don't quietly skip ahead. Update the milestone file (or split it). If the change is architectural, add an entry to [`/docs/blueprint/16-addendum.md`](../blueprint/16-addendum.md).

## Teaching mode parallel

If the project is in teaching mode (see [`/AGENTS.md`](../../AGENTS.md)), there's a parallel session-level plan at [`docs/teaching/sessions.md`](../teaching/sessions.md). It slices each milestone into sessions for cognitive-load discipline. The milestones in this directory stay clean and ship-mode-ready regardless — the teaching directory is an overlay, not a replacement.

If you're a coding agent operating in non-teaching mode, ignore the teaching directory entirely. Pick milestones from here, do them, mark done.
