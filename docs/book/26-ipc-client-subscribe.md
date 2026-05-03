---
chapter: 26
session_id: M11-1
title: IPC client and Subscribe
phase: C
estimated_time_minutes: 90
artifact: TUI receives daemon events live
status: stub
related_milestone: docs/implementation/tasks/M11-wire-ipc-into-tui.md
---

# Chapter 26 — IPC client and Subscribe

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M11-1` · Phase C · ~90 min · [Underlying milestone](../implementation/tasks/M11-wire-ipc-into-tui.md)

## What you'll learn

Build crates/tui/src/ipc_client.rs. Connect to daemon, send Subscribe, route incoming events into the input channel as Msg::DaemonEvent. How the TUI is just another client of the daemon, the broadcast subscription model.

## What you'll build

TUI receives daemon events live

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

The companion teaching-notes file at `docs/teaching/notes/26-ipc-client-subscribe.md` will be created during the live session.

## See also

- ← [Chapter 25-wire-main-loop](25-wire-main-loop.md)
- → [Chapter 27-stepping-wired](27-stepping-wired.md)
- [Underlying milestone](../implementation/tasks/M11-wire-ipc-into-tui.md)
- [Book README / TOC](README.md)
