---
chapter: 12
session_id: M5-1
title: The protocol crate and IpcMessage envelope
phase: B
estimated_time_minutes: 90
artifact: IpcMessage round-trips through bytes
status: stub
related_milestone: docs/implementation/tasks/M05-ipc-protocol-daemon.md
---

# Chapter 12 — The protocol crate and IpcMessage envelope

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M5-1` · Phase B · ~90 min · [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)

## What you'll learn

Create crates/protocol with IpcMessage, IpcPayload, Request, Response types (Ping/Pong only for now). Enum-as-message-type pattern, serde tagging strategies, why a dedicated protocol crate.

## What you'll build

IpcMessage round-trips through bytes

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

The companion teaching-notes file at `docs/teaching/notes/12-protocol-crate-ipc-envelope.md` will be created during the live session.

## See also

- ← [Chapter 11-first-real-breakpoint](11-first-real-breakpoint.md)
- → [Chapter 13-length-prefixed-json-codec](13-length-prefixed-json-codec.md)
- [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)
- [Book README / TOC](README.md)
