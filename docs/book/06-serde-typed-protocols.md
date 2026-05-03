---
chapter: 6
session_id: M2-1
title: Serde and typed protocols
phase: A
estimated_time_minutes: 90
artifact: Round-trip a typed DapRequest through bytes
status: stub
related_milestone: docs/implementation/tasks/M02-initialize-handshake.md
---

# Chapter 06 — Serde and typed protocols

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M2-1` · Phase A · ~90 min · [Underlying milestone](../implementation/tasks/M02-initialize-handshake.md)

## What you'll learn

Serde derive macros for typed JSON. DapRequest, DapResponse, Capabilities defined as Rust types with Serialize/Deserialize. Why generic types in the request/response shape.

## What you'll build

Round-trip a typed DapRequest through bytes

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

The companion teaching-notes file at `docs/teaching/notes/06-serde-typed-protocols.md` will be created during the live session.

## See also

- ← [Chapter 05-read-one-message](05-read-one-message.md)
- → [Chapter 07-dap-transport-and-seq](07-dap-transport-and-seq.md)
- [Underlying milestone](../implementation/tasks/M02-initialize-handshake.md)
- [Book README / TOC](README.md)
