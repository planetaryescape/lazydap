---
chapter: 7
session_id: M2-2
title: The DAP transport struct and atomic seq
phase: A
estimated_time_minutes: 90
artifact: Send initialize request, parse response, get the adapter's Capabilities
status: stub
related_milestone: docs/implementation/tasks/M02-initialize-handshake.md
---

# Chapter 07 — The DAP transport struct and atomic seq

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M2-2` · Phase A · ~90 min · [Underlying milestone](../implementation/tasks/M02-initialize-handshake.md)

## What you'll learn

DapTransport struct with request<T, R>() method. Generics in method signatures. The AtomicI32 family and why we need it for sequence numbers. Error type design with thiserror.

## What you'll build

Send initialize request, parse response, get the adapter's Capabilities

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

The companion teaching-notes file at `docs/teaching/notes/07-dap-transport-and-seq.md` will be created during the live session.

## See also

- ← [Chapter 06-serde-typed-protocols](06-serde-typed-protocols.md)
- → [Chapter 08-event-streaming](08-event-streaming.md)
- [Underlying milestone](../implementation/tasks/M02-initialize-handshake.md)
- [Book README / TOC](README.md)
