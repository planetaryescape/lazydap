---
chapter: 27
session_id: M11-2
title: Stepping commands wired
phase: C
estimated_time_minutes: 90
artifact: F5/F10/F11 keys step the program in the TUI
status: stub
related_milestone: docs/implementation/tasks/M11-wire-ipc-into-tui.md
---

# Chapter 27 — Stepping commands wired

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M11-2` · Phase C · ~90 min · [Underlying milestone](../implementation/tasks/M11-wire-ipc-into-tui.md)

## What you'll learn

Extend update to handle F5/F10/F11 producing Cmd::SendIpc(Request::Continue/Step). How new keybindings become two-line additions to the match (the discipline payoff of M10).

## What you'll build

F5/F10/F11 keys step the program in the TUI

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

The companion teaching-notes file at `docs/teaching/notes/27-stepping-wired.md` will be created during the live session.

## See also

- ← [Chapter 26-ipc-client-subscribe](26-ipc-client-subscribe.md)
- → [Chapter 28-source-pane-current-line](28-source-pane-current-line.md)
- [Underlying milestone](../implementation/tasks/M11-wire-ipc-into-tui.md)
- [Book README / TOC](README.md)
