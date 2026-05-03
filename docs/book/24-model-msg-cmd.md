---
chapter: 24
session_id: M10-1
title: Define Model, Msg, and Cmd
phase: C
estimated_time_minutes: 90
artifact: Refactored TUI state into the pure Model/Msg/Cmd shape (no behaviour change yet)
status: stub
related_milestone: docs/implementation/tasks/M10-elm-ify.md
---

# Chapter 24 — Define Model, Msg, and Cmd

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M10-1` · Phase C · ~90 min · [Underlying milestone](../implementation/tasks/M10-elm-ify.md)

## What you'll learn

TEA's three-types pattern (Elm). Refactor M9's state into AppState. Define enum Msg, enum Cmd. Write the empty update(state, msg) skeleton. Why mutation is restricted to update; anchor to React's useReducer.

## What you'll build

Refactored TUI state into the pure Model/Msg/Cmd shape (no behaviour change yet)

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

The companion teaching-notes file at `docs/teaching/notes/24-model-msg-cmd.md` will be created during the live session.

## See also

- ← [Chapter 23-source-pane-scrolling](23-source-pane-scrolling.md)
- → [Chapter 25-wire-main-loop](25-wire-main-loop.md)
- [Underlying milestone](../implementation/tasks/M10-elm-ify.md)
- [Book README / TOC](README.md)
