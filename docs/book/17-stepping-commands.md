---
chapter: 17
session_id: M6-1
title: Stepping commands
phase: B
estimated_time_minutes: 90
artifact: lazydap continue / step / pause work (no --wait yet)
status: stub
related_milestone: docs/implementation/tasks/M06-cli-subcommands.md
---

# Chapter 17 — Stepping commands

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M6-1` · Phase B · ~90 min · [Underlying milestone](../implementation/tasks/M06-cli-subcommands.md)

## What you'll learn

continue, step, step-into, step-out, pause. Fire-and-forget versions. How clap subcommands compose, the IPC dispatch pattern, why stepping commands are fire-and-forget by default.

## What you'll build

lazydap continue / step / pause work (no --wait yet)

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

The companion teaching-notes file at `docs/teaching/notes/17-stepping-commands.md` will be created during the live session.

## See also

- ← [Chapter 16-wire-launch-end-to-end](16-wire-launch-end-to-end.md)
- → [Chapter 18-wait-design](18-wait-design.md)
- [Underlying milestone](../implementation/tasks/M06-cli-subcommands.md)
- [Book README / TOC](README.md)
