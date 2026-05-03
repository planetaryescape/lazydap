---
chapter: 15
session_id: M5-4
title: Auto-spawning the daemon
phase: B
estimated_time_minutes: 90
artifact: First CLI invocation auto-spawns the daemon if it isn't running
status: stub
related_milestone: docs/implementation/tasks/M05-ipc-protocol-daemon.md
---

# Chapter 15 — Auto-spawning the daemon

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M5-4` · Phase B · ~90 min · [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)

## What you'll learn

The ensure_daemon_running dance: probe socket → fork daemon → poll for socket. Re-execing the binary, detaching from parent, the PID-file + flock pattern, the 'client probes before doing anything' workflow.

## What you'll build

First CLI invocation auto-spawns the daemon if it isn't running

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

The companion teaching-notes file at `docs/teaching/notes/15-auto-spawning-daemon.md` will be created during the live session.

## See also

- ← [Chapter 14-unix-sockets-accept-loop](14-unix-sockets-accept-loop.md)
- → [Chapter 16-wire-launch-end-to-end](16-wire-launch-end-to-end.md)
- [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)
- [Book README / TOC](README.md)
