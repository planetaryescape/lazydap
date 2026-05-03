---
chapter: 38
session_id: M18-1
title: debugpy adapter crate
phase: E
estimated_time_minutes: 90
artifact: lazydap launch foo.py works
status: stub
related_milestone: docs/implementation/tasks/M18-second-adapter.md
---

# Chapter 38 — debugpy adapter crate

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M18-1` · Phase E · ~90 min · [Underlying milestone](../implementation/tasks/M18-second-adapter.md)

## What you'll learn

crates/adapter-debugpy/. Implement DebugAdapter trait. Trait implementation patterns, where the codelldb assumptions were hidden (the lessons surface as you do this — collect them).

## What you'll build

lazydap launch foo.py works

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

The companion teaching-notes file at `docs/teaching/notes/38-debugpy-adapter.md` will be created during the live session.

## See also

- ← [Chapter 37-repl-pane](37-repl-pane.md)
- → [Chapter 39-adapter-routing](39-adapter-routing.md)
- [Underlying milestone](../implementation/tasks/M18-second-adapter.md)
- [Book README / TOC](README.md)
