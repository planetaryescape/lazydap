---
chapter: 10
session_id: M4-1
title: The full handshake order
phase: A
estimated_time_minutes: 90
artifact: All five DAP startup messages exchanged in correct order
status: stub
related_milestone: docs/implementation/tasks/M04-pause-on-breakpoint.md
---

# Chapter 10 — The full handshake order

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M4-1` · Phase A · ~90 min · [Underlying milestone](../implementation/tasks/M04-pause-on-breakpoint.md)

## What you'll learn

initialize → launch (don't await) → wait for initialized event → setBreakpoints → configurationDone → events flow. Why this order matters; what happens if you skip configurationDone.

## What you'll build

All five DAP startup messages exchanged in correct order

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

The companion teaching-notes file at `docs/teaching/notes/10-full-handshake-order.md` will be created during the live session.

## See also

- ← [Chapter 09-dap-launch-dance](09-dap-launch-dance.md)
- → [Chapter 11-first-real-breakpoint](11-first-real-breakpoint.md)
- [Underlying milestone](../implementation/tasks/M04-pause-on-breakpoint.md)
- [Book README / TOC](README.md)
