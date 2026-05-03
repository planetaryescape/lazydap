---
chapter: 25
session_id: M10-2
title: Wire the main loop
phase: C
estimated_time_minutes: 90
artifact: Event-driven update loop powering the TUI
status: stub
related_milestone: docs/implementation/tasks/M10-elm-ify.md
---

# Chapter 25 — Wire the main loop

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M10-2` · Phase C · ~90 min · [Underlying milestone](../implementation/tasks/M10-elm-ify.md)

## What you'll learn

Refactor the main loop to be update-driven. Channels for input + tick. Pure view function. tokio::select! over input + tick, why the loop reads channels and dispatches via update, the 'side effects via Cmd' pattern.

## What you'll build

Event-driven update loop powering the TUI

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

The companion teaching-notes file at `docs/teaching/notes/25-wire-main-loop.md` will be created during the live session.

## See also

- ← [Chapter 24-model-msg-cmd](24-model-msg-cmd.md)
- → [Chapter 26-ipc-client-subscribe](26-ipc-client-subscribe.md)
- [Underlying milestone](../implementation/tasks/M10-elm-ify.md)
- [Book README / TOC](README.md)
