# Learn Rust and DAP by building lazydap

A book that teaches you Rust and the Debug Adapter Protocol while you build a real terminal-first debugger. Read it solo, or open it in an LLM-aware coding agent and have it taught to you live.

## Two ways to read this book

### 🚶 Solo

Open chapter 01, work through it under your own steam. Each chapter:

- Opens by stating the **artifact** you'll have at the end (a thing you can run, not a concept).
- Surfaces your existing mental model with a **🤔 Q:** prompt before teaching anything new.
- Has **🔮 Predict** prompts before every code run. Answer in your head, then expand the `<details>` block to calibrate.
- Lets compiler errors land deliberately so you read them with the agent, instead of avoiding them.
- Closes with **📣 Teach-back** questions and a literal *run the artifact* moment.

Skip the predicts and this is just a tutorial. Do them and it's a teaching book.

### 🤖 LLM-as-teacher

Clone this repo, open it in an agent harness (Claude Code, Cursor, etc.), and ask:

> Start the next chapter

The agent will pick up where you left off, run the chapter live and responsively (calibrating to your specific predictions, running the real code, hitting the real compiler errors), and close with the same teach-back. The chapter is the canonical curriculum. The agent's job is to add live responsiveness inside that script, not to invent its own way.

See [`AGENTS.md`](../../AGENTS.md) for the teaching-mode contract.

## Who this is for

You're a senior software engineer (or close to it) with strong skills in something *other* than Rust: TypeScript, Python, Java, Go, whatever. You've read code in a systems language but never shipped one. You want to learn Rust for real, on a real project, without slogging through 700 pages of language-feature reference. You learn fastest when you write code and hit compiler errors, not when you read about them.

You're also someone who will benefit from learning DAP (the Debug Adapter Protocol) along the way: because you write tools that touch debuggers, or because debuggers fascinate you, or because you want to see how every IDE in the world makes "set a breakpoint" actually work.

## What lazydap is

A scriptable, terminal-first debugger. CLI core, JSON-over-Unix-socket protocol, multiple frontends (TUI, agent skill, anything anyone wants to build). The book builds it from zero.

For the full project vision, see [`docs/blueprint/00-overview.md`](../blueprint/00-overview.md). For the underlying engineering tasks (ship-mode, no teaching), see [`docs/implementation/`](../implementation/).

## Table of contents

### Phase 0 — Foundations (workspace, conventions)

| Ch. | Title | Concept | Artifact |
|---|---|---|---|
| 01 | [Cargo workspaces](01-cargo-workspaces.md) | Workspace structure + version inheritance | `cargo metadata` works at the root |
| 02 | [Async main and clap](02-tokio-main-clap.md) | `#[tokio::main]` + clap derive | `cargo run -p lazydap-daemon -- --message hi --count 3` |
| 03 | [Convention as code](03-conventions-as-code.md) | Toolchain pin, formatter, linter, CI | The four CI checks pass locally |

### Phase A — See the protocol (M0 – M4)

| Ch. | Title | Concept | Artifact |
|---|---|---|---|
| 04 | [Hello, adapter](04-hello-adapter.md) | `tokio::process::Command`, `Stdio::piped`, `kill_on_drop` | Spawn codelldb, print first stderr chunk |
| 05 | [Read one message](05-read-one-message.md) | `read_line` / `read_exact` / `BufReader`, `Content-Length` framing, `lines()` move-out footgun | Parse a real DAP `initialize` response |
| 06 | Serde + typed protocols | Derive macros, JSON ↔ Rust mapping | Typed `DapRequest`/`DapResponse` round-trips |
| 07 | The transport struct + atomic seq | Generics, `AtomicI32`, error types | Send `initialize`, parse response |
| 08 | Event streaming + tagged enums | Rust enums vs TS unions, pattern matching | Read events as they come |
| 09 | The DAP launch dance | Asymmetric send-vs-listen pattern | Launch a hello-world program, observe events |
| 10 | The full handshake order | DAP capability initialisation | All five startup messages in correct order |
| 11 | First real breakpoint | Stopped events, `continue` request | Stop on a known line, resume, exit |

### Phase B — Daemon + protocol (M5 – M7)

| Ch. | Title | Concept | Artifact |
|---|---|---|---|
| 12 | The protocol crate | Enum-as-message-type, serde tagging | `IpcMessage` round-trips through bytes |
| 13 | Length-prefixed JSON codec | Big-endian length headers, `read_exact` | `read_message` / `write_message` work |
| 14 | Unix sockets + accept loop | `UnixListener`, task-per-client | Daemon binds; client connects |
| 15 | Auto-spawning daemon | PID-file + flock + re-exec | Client probes; if no daemon, spawns one |
| 16 | Wire `lazydap launch` end-to-end | First real subcommand, full path | `lazydap launch ./hello` works |
| 17 | Stepping commands | Fire-and-forget IPC | `lazydap continue` returns immediately |
| 18 | The `--wait` design | `tokio::select!`, broadcast, coalescing | `lazydap continue --wait` returns one JSON blob |
| 19 | Inspection commands | `variables_reference`, lazy expansion | `lazydap stack`, `scopes`, `eval` work |
| 20 | Persistent breakpoints | TOML state, debounced writes | Breakpoints survive daemon restart |
| 21 | Skill + agent verification | Skill ZIP packaging, agent-native CLI surface | Claude Code drives lazydap end-to-end |

### Phase C — TUI (M8 – M11)

| Ch. | Title | Concept | Artifact |
|---|---|---|---|
| 22 | Hello ratatui | Immediate-mode rendering loop | `lazydap-tui` shows centred text |
| 23 | Show a file | Layouts, scroll offset | A file viewer pane |
| 24 | Define Model / Msg / Cmd | TEA's three-types pattern (Elm) | Refactor M9 state into pure shape |
| 25 | Wire the main loop | `tokio::select!` over input + tick | Event-driven update loop |
| 26 | IPC client + Subscribe | Daemon broadcast subscription | TUI receives daemon events |
| 27 | Stepping commands wired | Keybinding → IPC dispatch | F5/F10/F11 step the program |
| 28 | Source pane shows current line | Daemon-event-to-UI pipeline | TUI shows where the debugger paused |

### Phase D — Useful features → v0.1 (M12 – M15)

| Ch. | Title | Concept | Artifact |
|---|---|---|---|
| 29 | Stack pane | Fetch-on-event pattern | Stack pane navigates frames |
| 30 | Scope tree render | Tree rendering in ratatui | Scope tree visible (no expand) |
| 31 | Lazy expand variables | Request/response correlation | `<CR>` expands a variable |
| 32 | Toggle breakpoint | Verified-vs-unverified state | `b` toggles a breakpoint |
| 33 | Config crate | XDG paths, defaults merging | Config loads from `~/.config/lazydap/` |
| 34 | `launch.json` import | Foreign format with comments + variables | `.vscode/launch.json` works |
| 35 | Release prep + ship v0.1 | Cargo publish dance, release-please | **lazydap v0.1.0 published** |

### Phase E — Beyond v0.1 (M16 – M18)

| Ch. | Title | Concept | Artifact |
|---|---|---|---|
| 36 | Watches pane + persist | Modal pattern, per-pause re-eval | Watches survive across pauses |
| 37 | REPL pane | Command history, watch vs repl context | Type expressions, see results |
| 38 | debugpy adapter crate | Trait implementation patterns | `lazydap launch foo.py` works |
| 39 | Adapter routing | Discovery chain, filetype detection | Auto-pick adapter by filetype |

## How to read it

1. Start at chapter 01. The ladder is cumulative. Chapter 04 assumes you ran chapter 03's artifact.
2. Read in order. Each chapter caps at one new concept (cognitive load discipline). Skipping ahead is allowed but you'll feel it.
3. Type the code yourself. Don't paste. The compiler conversation is the curriculum.
4. Pause at every **🔮 Predict** before expanding the `<details>`. Wrong predictions are the most teachable moments. Your mental model needs that data.
5. Demonstrate the artifact at chapter close. Run the thing. See it work. The visibility is the fuel.

## Prerequisites

- **A working Rust toolchain.** Install via [rustup](https://rustup.rs/) — the repo's `rust-toolchain.toml` will auto-select the right channel/components when you `cd` in.
- **macOS or Linux.** Windows isn't currently a target.
- **Comfort with one mainstream language.** The book uses TypeScript and Python as primary anchors, with C as a deeper pain anchor for memory-related concepts. You don't need C; you do need some mainstream language to anchor against.

## Picking up mid-book

If you skipped chapters: each chapter's "Before you start" section lists exact verification commands. Run them; if they fail, go back and follow the linked predecessor.

If you got the repo from someone else and want to start from the current state: `git log --oneline` shows the canonical commits. Check out the commit that lands the chapter you want to start from, then go forward.

## Companion artifacts

- [`docs/teaching/notes/`](../teaching/notes/): the *teacher's* working files. One per chapter. Captures common wrong predictions, sticky points, refinement ideas. Read these if you're an LLM teaching a learner; they tell you what to pre-empt.
- [Obsidian: `Lazydap Teaching Sessions.md`]: the original learner's private journal of every session that produced these chapters. Not in this repo (private knowledge base); referenced for completeness.

## See also

- [`AGENTS.md`](../../AGENTS.md): teaching-mode contract for AI agents working on lazydap
- [`docs/blueprint/`](../blueprint/): full project vision (recenter when lost)
- [`docs/implementation/`](../implementation/): ship-mode milestone tasks
- [`docs/teaching/sessions.md`](../teaching/sessions.md): the per-session syllabus that produced this book
- The portable [teaching skill](https://github.com/): the pedagogy this book is built on (`~/.dotfiles/.skills/teaching/SKILL.md` in the original author's setup)
