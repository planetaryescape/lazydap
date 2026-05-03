---
chapter: 28
session_id: M11-3
title: Source pane shows current line
phase: C
estimated_time_minutes: 90
artifact: TUI shows where the debugger paused — lazydap v0.1-prerelease
status: stub
related_milestone: docs/implementation/tasks/M11-wire-ipc-into-tui.md
---

# Chapter 28 — Source pane shows current line

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M11-3` · Phase C · ~90 min · [Underlying milestone](../implementation/tasks/M11-wire-ipc-into-tui.md)

## What you'll learn

On Stopped event, fetch top frame, set current_line, render the arrow marker. The daemon-event-to-UI-state pipeline end-to-end.

## What you'll build

TUI shows where the debugger paused — lazydap v0.1-prerelease

## Before you start

Run the previous chapter's artifact to confirm the baseline. Exact verification commands land when this chapter is drafted.

## Outline (placeholder — filled during the live session)

1. *Surface the model* — a 🤔 Q: prompt anchored against the reader's prior-language model.
2. *Concept slices* — broken down per the one-concept-per-session rule. Each slice has a 🔮 Predict pause before the code lands.
3. *Compiler conversation* — at least one deliberate-error walkthrough where the compiler points at the bug.
4. *Try it yourself* — one analogous task the reader writes alone (gradual release: I do → we do → you do).
5. *What you can run now* — the artifact, demonstrated.
6. *Teach-back* — 📣 questions to confirm the concept landed.
7. *Pain anchors covered* — table summarising what real-world pain each new construct addresses.

The companion teaching-notes file at `docs/teaching/notes/28-source-pane-current-line.md` will be created during the live session.

## See also

- ← [Chapter 27-stepping-wired](27-stepping-wired.md)
- → [Chapter 29-stack-pane](29-stack-pane.md)
- [Underlying milestone](../implementation/tasks/M11-wire-ipc-into-tui.md)
- [Book README / TOC](README.md)
