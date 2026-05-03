---
chapter: 16
session_id: M5-5
title: Wire lazydap launch end-to-end
phase: B
estimated_time_minutes: 90
artifact: lazydap launch ./hello works end-to-end
status: stub
related_milestone: docs/implementation/tasks/M05-ipc-protocol-daemon.md
---

# Chapter 16 — Wire lazydap launch end-to-end

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M5-5` · Phase B · ~90 min · [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)

## What you'll learn

First real subcommand. CLI side (crates/daemon/src/cli/launch.rs) → IPC client → daemon handler → DAP transport from Phase A → response back. The moment the architecture from blueprint becomes real code.

## What you'll build

lazydap launch ./hello works end-to-end

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

The companion teaching-notes file at `docs/teaching/notes/16-wire-launch-end-to-end.md` will be created during the live session.

## See also

- ← [Chapter 15-auto-spawning-daemon](15-auto-spawning-daemon.md)
- → [Chapter 17-stepping-commands](17-stepping-commands.md)
- [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)
- [Book README / TOC](README.md)
