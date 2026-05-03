---
chapter: 35
session_id: M15-3
title: Release prep and ship v0.1
phase: D
estimated_time_minutes: 90
artifact: lazydap v0.1.0 published to crates.io
status: stub
related_milestone: docs/implementation/tasks/M15-config-file.md
---

# Chapter 35 — Release prep and ship v0.1

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M15-3` · Phase D · ~90 min · [Underlying milestone](../implementation/tasks/M15-config-file.md)

## What you'll learn

LICENSE, CHANGELOG, README, CI publish workflow, cargo publish order, tag v0.1.0. The cargo-publish dependency-order dance (publish leafs first, daemon last), release-please setup.

## What you'll build

lazydap v0.1.0 published to crates.io

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

The companion teaching-notes file at `docs/teaching/notes/35-release-prep-ship-v01.md` will be created during the live session.

## See also

- ← [Chapter 34-launch-json-import](34-launch-json-import.md)
- → [Chapter 36-watches-pane](36-watches-pane.md)
- [Underlying milestone](../implementation/tasks/M15-config-file.md)
- [Book README / TOC](README.md)
