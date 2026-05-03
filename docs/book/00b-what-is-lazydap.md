---
chapter: "0b"
title: What is lazydap
status: complete
estimated_time_minutes: 8
---

# Chapter 00b — What is lazydap

> Required reading. ~8 minutes. The technical context everyone needs before chapter 01.

## What you'll know by the end

- What DAP is, and what DAP isn't
- The debugger layer cake: the half-dozen layers between you typing "set a breakpoint" and the kernel actually pausing the debuggee
- Where lazydap sits in that stack, and how the "wrapper" framing applies to it

## What is DAP?

**DAP** is the **Debug Adapter Protocol**. It's the JSON-over-stdio (or sometimes TCP) language that IDE plugins speak when they want to drive a debugger. Microsoft created it for VS Code and then opened it up; now every major editor that supports debugging speaks DAP.

The crucial framing: **DAP is communication, not debugging.** It's how an IDE asks a debugger "set a breakpoint at line 42" and how the debugger replies "ok, here's what happened when you hit it." The debugging itself happens way below DAP, in something called the layer cake.

## The debugger layer cake

This is the sketch you'll come back to throughout the book.

```
┌──────────────────────────────────────────────────────┐
│  YOU (or your TUI, your IDE, your agent)             │
└────────────────────┬─────────────────────────────────┘
                     │ "set a breakpoint at line 42"
                     ▼
┌──────────────────────────────────────────────────────┐
│  Frontend: lazydap / VS Code / nvim-dap              │   ← what we're building
└────────────────────┬─────────────────────────────────┘
                     │ DAP (JSON messages, communication only)
                     ▼
┌──────────────────────────────────────────────────────┐
│  Adapter: codelldb / debugpy / dlv-dap / lldb-dap    │   ← speaks DAP on top, native debugger underneath
└────────────────────┬─────────────────────────────────┘
                     │ Library calls (C++ API, Python API, etc.)
                     ▼
┌──────────────────────────────────────────────────────┐
│  Real debugger: LLDB · GDB · sys.settrace · V8       │   ← THIS is what knows how to debug
└────────────────────┬─────────────────────────────────┘
                     │ OS debugging API
                     ▼
┌──────────────────────────────────────────────────────┐
│  ptrace (Linux) · Mach exceptions (macOS) · Win32    │   ← syscalls into the kernel
└────────────────────┬─────────────────────────────────┘
                     │
                     ▼
┌──────────────────────────────────────────────────────┐
│  Your debuggee process, paused, inspected, resumed   │
└──────────────────────────────────────────────────────┘
```

Layer by layer, briefly:

- **Frontend**: the UI a human (or agent) talks to. Translates intent into DAP messages. lazydap is one of these. Doesn't know what `ptrace` is, doesn't know what DWARF is. **Pure UX.**
- **DAP**: just JSON. `{"command": "setBreakpoints", "arguments": ...}`. **No code is executing on the debuggee at this layer.** It's a wire format. Same shape as LSP — `Content-Length: N\r\n\r\n` framed.
- **Adapter**: a bridge process. Speaks DAP on its stdio (or TCP). Internally calls a real debugger library to do the work. codelldb embeds LLDB; debugpy uses Python's `sys.settrace`; js-debug talks to V8's Inspector Protocol. **The adapter is where DAP meets the actual debugger.**
- **Real debugger**: this is where debugging actually happens. LLDB knows DWARF symbols (line 42 maps to address `0x4011a8`). GDB knows the same for its targets. They use OS-level APIs (`ptrace` on Linux, Mach exceptions on macOS, Windows Debug API on Windows) to inspect another process.
- **Kernel**: ultimately stops the debuggee, delivers signals, mediates memory access between the two address spaces.

For the long version with each layer explained and a worked example of "what happens when you set a breakpoint" (the part most explanations skip), read [`docs/reference/how-debuggers-actually-work.md`](../reference/how-debuggers-actually-work.md). It's the most important reference doc in this project.

The short version, in one sentence: **lazydap is the top layer. Everything below it already exists and we're not rewriting it.**

## Where lazydap sits

So lazydap is a **frontend**. We:

- Send DAP messages to an adapter (chapter 04 introduces codelldb, our first one)
- Receive DAP responses and events back
- Translate user intent (CLI subcommands, TUI keybindings) into DAP requests
- Render adapter responses as human-readable output (table for TTYs, JSON for non-TTYs)

We do **not**:

- Implement breakpoints — the adapter does
- Read DWARF debug symbols — LLDB does
- Touch the debuggee process directly — the kernel does, mediated through the adapter
- Re-implement DAP — we just speak it

This is the right slice to work at. The layer cake below us is mature. Anyone trying to write a debugger from scratch in 2026 is doing it for academic reasons, not practical ones.

## "But isn't lazydap just a wrapper?"

Yes. Every usable debugger surface is a wrapper.

- VS Code's debugger wraps DAP.
- CLion wraps DAP and JetBrains' platform debuggers.
- Xcode's debug pane wraps LLDB directly.
- gdbgui wraps GDB.
- nvim-dap wraps DAP.

The question isn't whether lazydap is a wrapper. **The question is what the wrapper is for.** lazydap's design constraints (every action exposed as a CLI subcommand; JSON output by default in non-TTY contexts; stable schemas; the TUI as a regular consumer of the same protocol) exist because the wrapper has a specific job: **let humans, agents, and scripts all use the same surface.** Not "expose DAP," but "expose debugging."

If you want the long-form version of this argument with concrete comparisons (lazygit on git, kubectl on the K8s API, httpie on curl), [`docs/articles/yes-its-a-wrapper.md`](../articles/yes-its-a-wrapper.md) is a good read.

## What to take from this chapter

The required thing is the **layer cake**. Keep it in your head. When chapter 04 has you spawning codelldb and reading bytes off its stderr, you'll think: "right, lazydap (frontend) is talking to codelldb (adapter), which will eventually talk to LLDB (real debugger), which talks to ptrace (kernel API)." That mental picture is what makes the design choices comprehensible.

Optional but useful: the wrapper framing. When someone asks "isn't this just a wrapper on DAP?" — yes, and here's why the question misses the point.

If you'd like the personal backstory behind the project (why I built this, what was the C journey doing, why Rust specifically), [Chapter 00c](00c-why-i-built-this.md) has it. If you don't care, skip straight to [Chapter 01](01-cargo-workspaces.md). The book works either way.

## See also

- [`docs/reference/how-debuggers-actually-work.md`](../reference/how-debuggers-actually-work.md): the full layer-cake walkthrough with a worked example of how a breakpoint fires
- [`docs/articles/yes-its-a-wrapper.md`](../articles/yes-its-a-wrapper.md): the "wrapper" rebuttal, plumbing-and-porcelain framing
- [`docs/articles/the-cli-is-the-product.md`](../articles/the-cli-is-the-product.md): why every action is a CLI subcommand, why the TUI is a client of the same protocol
- [`docs/articles/agent-driven-debugging.md`](../articles/agent-driven-debugging.md): the state of agent-driven debugging in early 2026, and the niche lazydap fills

Next up: [Chapter 00c — Why I built this](00c-why-i-built-this.md) (optional backstory) or [Chapter 01 — Cargo workspaces](01-cargo-workspaces.md) (where the book starts building).
