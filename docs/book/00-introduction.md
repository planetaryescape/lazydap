# Chapter 00 — Introduction

> Read this first. ~10 minutes.

## What this book is, in one paragraph

A guided, multi-session walk through learning Rust *and* the Debug Adapter Protocol while building a real, useful debugger called **lazydap**. Each chapter is one teaching session: one new concept, one runnable artifact at the end. Read it solo (like a textbook with predict-pauses) or open it in an LLM-aware coding agent and have it taught to you live (the agent runs the chapter as a script, calibrating to your specific predictions). By chapter 35, you'll have published a real Rust crate to crates.io. By chapter 39, lazydap supports two languages.

## What this book is not

- **Not a Rust reference.** The official [Rust Book](https://doc.rust-lang.org/book/) and [Rust by Example](https://doc.rust-lang.org/rust-by-example/) cover the language exhaustively. Read those alongside; they're free, well-maintained, and complementary.
- **Not a DAP specification.** The official [DAP spec](https://microsoft.github.io/debug-adapter-protocol/) is the source of truth. The book touches what's needed to make lazydap work.
- **Not a tutorial.** Tutorials assume nothing about your prior knowledge and explain everything. This book assumes you've shipped real software in *something*, TypeScript, Python, Java, Go, anything, and uses that as your starting model. The chapters surface what you already know, then teach what's different.
- **Not interactive in the chat-bot sense.** "LLM-as-teacher" mode is a coding agent following the chapter as a script, not a chat about Rust.

## How learning works here

Three pedagogical ideas, paid for in pain:

### 1. Surface your model before teaching

Every chapter opens with a **🤔 Q:** prompt asking you to predict or describe how something works *in a language you already know*. You answer in your head (solo) or out loud (live). Then the chapter calibrates.

This isn't a quiz. It's a diagnostic. Your prediction tells the chapter (or agent) where you actually are, so it can teach what's *new* and skip what's familiar. If you've already mostly-known something, the chapter extends your model. If you've half-known it, the chapter flags where the analogy breaks.

> **Why it matters:** the hardest learners to teach are the ones who *think* they understand but actually don't. Surfacing the model up-front pulls the misalignment into view before you build code on top of it.

### 2. Predict before run

Every code block has a **🔮 Predict** prompt before you run it: "what will this print?" or "what will the compiler say?" Answer in your head, then expand the `<details>` block to calibrate.

Wrong predictions are the most teachable moments. They tell you exactly which part of your mental model is buggy. Right predictions move you forward fast.

If you skip the predicts, the chapter degrades into a flat tutorial. The pedagogy depends on you genuinely not knowing the answer when you guess.

### 3. The compiler is the teacher

Rust's compiler is famous for its error messages. They suggest fixes. They point at exact spans. They link to the book. Many chapters deliberately have you write code-that-doesn't-quite-work, hit a compiler error, read it together, and then fix.

Pre-empting compiler errors by writing "correct" code from the start would rob you of the conversation between you and the compiler that actually builds your mental model. Don't fight the chapter when it tells you to remove a `mut` and run `cargo check`. The error is the curriculum.

## The cumulative project: lazydap

This book is a long-arc project, not isolated examples. Each chapter's artifact is *cumulative*:

- **Chapters 01–03** (Phase 0, foundations): you have a Rust workspace with one binary that prints arguments. Tests pass; CI is green.
- **Chapters 04–11** (Phase A, protocol): lazydap can spawn a real debugger backend, send DAP messages, set breakpoints, observe stops.
- **Chapters 12–21** (Phase B, daemon): lazydap is a CLI you can drive from a shell. Its commands return JSON. Agents can use it.
- **Chapters 22–28** (Phase C, TUI): a terminal UI exists, talks to the daemon, shows current source line on pause.
- **Chapters 29–35** (Phase D, features → v0.1): scope tree, breakpoints in TUI, config files, **published v0.1.0** to crates.io.
- **Chapters 36–39** (Phase E, beyond): watches, REPL, second adapter (debugpy → Python support).

By the time you finish chapter 35, lazydap is shipped. Real users can install it. You did that.

The frame to hold: **the goal isn't to ship lazydap. The goal is to build a Rust engineer.** The published crate is a side effect. (A delicious one.)

## Pace and slowness

This book is slow on purpose. Senior-engineer learners have strong intrinsic motivation but high opportunity cost. The failure mode is "concepts piling up toward a future arrival point that never quite arrives." So:

- **Each chapter caps at one new concept.** Two new concepts in one session loads cognitive capacity past what schema-building can handle.
- **Each chapter ends with a runnable artifact.** Stop after any chapter and there's a working thing. Skateboard → scooter → bike → motorbike → car (Henrik Kniberg's MVP framing).
- **Each chapter ends with a teach-back.** If you can't articulate the concept in your own words, the chapter didn't land. Re-read before moving on.

Sessions that don't land aren't failures; they're data. The book says "go back" because going back is faster than fighting forward on a broken foundation.

## On C

You will see references to **C pain points** throughout the book: `malloc/free`, `char*` ambiguity, NULL pointer dereferences, `fork`/`exec`, dangling pointers, `errno` checking. This is deliberate.

Many of Rust's hardest features (ownership, lifetimes, `Result`, `Option`, exhaustive `match`) exist *to fix specific pains in C*. If you've felt those pains, the Rust solutions land deeper. If you haven't, the Rust solutions feel like trivia.

If you're learning C in parallel (the original learner of this book was), the cross-pollination compounds. Every C frustration becomes a sticky teaching moment for the Rust feature that fixes it. If you don't know C: the book leans on TypeScript and Python more, and treats C pains as "scenarios you'd hit in an unsafe systems language". Useful framing even if you don't run into them yourself.

## On predict-before-run for senior engineers

Senior engineers can fake confidence. You've probably done it; everyone has. Predict-before-run is a forcing function for honesty: when you write down (or think out loud) "I think this will print X", you commit. When the code prints Y, you can't pretend you knew. The misalignment is now visible, in your own head.

That visibility is what makes the learning stick. Trust the process; it gets less awkward by chapter 04.

## On the LLM-as-teacher mode

If you're reading this in an agent harness (Claude Code, Cursor, etc.), the agent will:

- Read the chapter and its companion teaching-notes file before starting
- Ask the chapter's **🤔 Q:** and **🔮 Predict:** questions to *you* in real time, not just inline
- Wait for your answer
- Calibrate (using the chapter's `<details>` blocks as a menu of common responses)
- Run the actual code in your environment, not a stale screenshot
- Read the actual compiler output with you, not a cached one
- Close with the chapter's teach-back questions

The agent's contract: **the chapter is the curriculum.** It does not freestyle, skip ahead, or invent new concepts. If something genuinely new comes up, the agent flags it and stays in the chapter's lane.

If the chapter is wrong (a library version drifts, an OS quirk lands, your environment differs), the agent works around it AND files a follow-up to update the chapter. The book improves over time.

## Picking up where you left off

Each chapter's "Before you start" section lists *exact verification commands* you can run to confirm the previous chapter's artifact still works. Run them every time you re-open the book. If anything fails, go back to the chapter that introduced the broken thing.

If you're forking this repo to learn from someone else: `git log --oneline` shows you the cumulative commit history. Check out the commit at the end of any chapter to start from that point.

## Where to go next

[**Chapter 01: Cargo workspaces →**](01-cargo-workspaces.md)

If you'd like to skim the whole arc first: [Table of contents](README.md).

If you want the project background before diving in: [`docs/blueprint/00-overview.md`](../blueprint/00-overview.md).

If you're an agent: [`AGENTS.md`](../../AGENTS.md) (teaching-mode contract).
