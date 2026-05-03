---
chapter: 21
session_id: M7-1
title: Skill ZIP and agent verification
phase: B
estimated_time_minutes: 90
artifact: An LLM agent successfully drives lazydap end-to-end via the skill
status: stub
related_milestone: docs/implementation/tasks/M07-skill-agent-verification.md
---

# Chapter 21 — Skill ZIP and agent verification

> **Status:** stub. Not yet taught live; will be filled in when the corresponding teaching session runs. Below is the planned scope; the predict-pauses, compiler conversations, and pain-anchor framing get added during the live session per the chapter template.

> Session ID: `M7-1` · Phase B · ~90 min · [Underlying milestone](../implementation/tasks/M07-skill-agent-verification.md)

## What you'll learn

Hand-write SKILL.md plus auto-generate references/commands.md from clap. ZIP. Test conversation. How the agent skill differs from the human CLI (it's the same surface; the skill is just docs), what 'agent-native' means in practice.

## What you'll build

An LLM agent successfully drives lazydap end-to-end via the skill

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

The companion teaching-notes file at `docs/teaching/notes/21-skill-zip-agent-verification.md` will be created during the live session.

## See also

- ← [Chapter 20-persistent-breakpoints](20-persistent-breakpoints.md)
- → [Chapter 22-hello-ratatui](22-hello-ratatui.md)
- [Underlying milestone](../implementation/tasks/M07-skill-agent-verification.md)
- [Book README / TOC](README.md)
