---
chapter: 13
session_id: M5-2
title: Length-prefixed JSON codec
phase: B
estimated_time_minutes: 90
artifact: Encode and decode a Ping/Pong roundtrip via length-prefixed bytes
status: stub
related_milestone: docs/implementation/tasks/M05-ipc-protocol-daemon.md
---

# Chapter 13 — Length-prefixed JSON codec

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M5-2` · Phase B · ~90 min · [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)

## What you'll learn

read_message and write_message in crates/protocol/src/codec.rs. read_exact vs read, big-endian vs little-endian, the cancellation-safety footgun in async reads.

## What you'll build

Encode and decode a Ping/Pong roundtrip via length-prefixed bytes

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

The companion teaching-notes file at `docs/teaching/notes/13-length-prefixed-json-codec.md` will be created during the live session.

## See also

- ← [Chapter 12-protocol-crate-ipc-envelope](12-protocol-crate-ipc-envelope.md)
- → [Chapter 14-unix-sockets-accept-loop](14-unix-sockets-accept-loop.md)
- [Underlying milestone](../implementation/tasks/M05-ipc-protocol-daemon.md)
- [Book README / TOC](README.md)
