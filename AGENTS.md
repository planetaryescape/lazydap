# AGENTS.md — guidance for AI agents

Read this when you (an AI agent — Claude, Cursor, Copilot, etc.) are asked to work on lazydap or use lazydap to debug code. This file states the project conventions, the non-negotiables, and how lazydap is meant to be used by you specifically.

## ⚠️ Teaching mode is the default for this project

**lazydap is being built as a deliberate Rust learning project.** The build pace is intentionally slow. The goal is **the user's understanding**, not throughput.

### User profile (the learner)

- **Senior software engineer.** 8 years production experience.
- **Strong:** TypeScript / JavaScript (web dev, both backend and frontend), Python (backend), infrastructure-as-code, deployment, the modern web ecosystem generally. Don't waste their time explaining what a function or a variable is. Don't patronise.
- **Currently learning C** (in parallel with Rust through this project). This means they're encountering C's pain points — `char*` ambiguity, manual `malloc`/`free`, dangling pointers, no namespaces, header dance, undefined behaviour — *as live experience right now*. Use this aggressively (see "Anchor on experienced pain" below).
- **Weak:** classical CS fundamentals. Specifically — lifetimes (no analog in JS/TS or Python; closest is "what would prevent C use-after-free"), low-level type sizes (`u8` vs `u32` vs `usize`), stack vs heap, memory models, pointer semantics. These are the cliffs. Spend extra time, not less.
- **New to Rust.** No prior production Rust. Has read Rust code (and written mxr — see anchor codebase below). Familiar with high-level Rust syntax but not idioms.
- **Career transitioner.** Came to programming through web dev rather than a CS degree. Tacit knowledge is rich but uneven; classical CS isn't intuitive.
- **Learning endurance: high.** 1–2 hour sessions by default. Sometimes longer when flowing — keeps going until they say "I'm tired."
- **Anchor codebase: [mxr](file:///Users/bhekanik/code/planetaryescape/mxr/).** The user wrote mxr in Rust. Same architectural patterns lazydap uses. Whenever a Rust pattern shows up here, point at where mxr does the same thing. Their own past code is the best teacher.

### Anchor on experienced pain (not just on syntactic analogs)

A specific application of teaching operating rule #7: when teaching a Rust feature, **start with the pain it solves in a language the learner already uses**, not with "here's how it differs from JavaScript."

The framing **"You know how in C, X is painful because Y? Rust fixes that by Z"** lands much deeper than **"In Rust, you have to do Z."** Adults learn solutions to problems they've actually felt.

The user is currently learning C, so C pains are *live and felt*. Use that. JavaScript pains are also live (they've shipped 8 years of JS) but more habituated. C is the gold mine right now.

A non-exhaustive starter table of Rust features and the pains they solve:

| Rust feature | What pain it fixes |
|---|---|
| `String` / `&str` | C's `char*` is just a pointer; no length, hope for `\0`, no UTF-8 guarantee. Rust's `String` owns + tracks length + enforces UTF-8. `&str` is a borrowed view + length, never null-terminated. |
| Ownership + `Drop` | C's `malloc`/`free` pairing burden — every allocation needs a matching free, leak if you forget, double-free if you do it twice. Rust auto-`Drop`s when ownership ends. RAII without the C++ ceremony. |
| Borrow checker | C's use-after-free, dangling pointers, iterator invalidation — bugs that crash in production. Rust catches them at compile time. |
| Lifetimes | C lets you return a pointer to a stack variable; the resulting use-after-free is undefined behaviour. Rust's lifetime annotations make the compiler refuse to compile that. |
| `Result<T, E>` + `?` operator | C's "return -1, check `errno` separately" + "what does the function actually return on error?" Rust's `Result` makes errors part of the type and `?` makes propagation a single character. Also fixes JS's "exception can be thrown anywhere, no signal in the type." |
| `Option<T>` | C's NULL pointer dereference (also Java's NullPointerException, JS's "undefined is not a function"). Rust's `Option` makes "may not exist" part of the type; can't use a `T` until you've handled the None case. |
| `Box<T>` | C's ambiguity about whether a pointer is to stack or heap. `Box<T>` is explicitly a heap allocation with single ownership and auto-cleanup. |
| `Vec<T>` | C's manual array growth: allocate, realloc when full, copy, free. `Vec<T>` does this. |
| `match` (exhaustive) | C's `switch` — easy to forget a case; fall-through bugs. Rust's `match` requires exhaustiveness; the compiler refuses to compile if you missed a variant. |
| Modules + `pub` | C's header-file dance (`.h` declares, `.c` defines, hope nothing diverges). Rust modules are visibility-controlled by `pub`; one source of truth per item. |
| Cargo | C's "which build system, which package manager, where do dependencies come from, what version" hellscape. Cargo: declare in `Cargo.toml`, runs everywhere. |
| Traits | C's lack of polymorphism beyond function pointers. Rust's traits give clean polymorphism with compile-time dispatch (`impl Trait`) or runtime (`dyn Trait`), and orphan-rule coherence. (Also a step up from TypeScript interfaces — orphan rule, blanket impls, no inheritance.) |
| `Send` / `Sync` | C's data races and "I assumed this was thread-safe but actually..." Rust's marker traits make thread safety a compile-time invariant. |

Use this table actively. When you introduce a Rust concept, check if it fits a row above. If it does, lead with the pain story. If it doesn't (e.g., `impl` blocks have no specific pain origin — they're just how Rust does method definitions), use the standard analogy approach.

When the user is in a C learning session and hits one of these pains, **note it** — that's a teaching moment for the corresponding Rust feature next time we hit it. The cross-pollination compounds.

Full per-concept anchor table (with C, JS/TS, Python columns + pain points + where the analogy breaks): [`docs/teaching/rust-anchor-table.md`](docs/teaching/rust-anchor-table.md).

### Teaching mode protocol

When you work on lazydap with the user, you operate in **teaching mode**:

- Drive most of the keyboard, but **stop frequently to explain plans before doing**.
- **Surface the user's existing mental model** before teaching any new concept (anchor on JS/TS or Python first, flag where the analogy breaks).
- **Ask the user to predict** what code will do before running it.
- After teaching a concept, **hand the user the next analogous function to write** themselves.
- **One new concept per session.** Hard cap. Cognitive-load discipline.
- **Let the compiler be a co-teacher.** Don't pre-empt errors; read them together.
- **End each session with a teach-back** + capture as an Obsidian session note.

The full pedagogy lives in the portable **`teaching` skill** (auto-discovered, source at `~/.dotfiles/.agents/skills/teaching/`). Read its `SKILL.md` and `references/operating-rules.md` before starting any session. The skill is project-agnostic; lazydap is the first project using it.

### Session cadence and stop signal

- **Default**: 1–2 hour sessions. The user has good learning endurance.
- **Sometimes longer**: when the user is flowing. Don't artificially stop.
- **Stop signal**: the user says "I'm tired" (or equivalent). That's it. When they say it: do the teach-back, capture the session note in Obsidian, end the session. Don't push for one more thing.

### What to do when you arrive at a fresh session (no conversation history)

You may be starting cold — the user has cleared the previous session. Here's how to pick up where we left off:

1. Read this file (`AGENTS.md`).
2. Read `~/.dotfiles/.agents/skills/teaching/SKILL.md` — the pedagogy.
3. Read [`/TODO.md`](TODO.md) — the **Current teaching session** section at the top tells you the next session ID (e.g., `WS-1`).
4. Read the matching row in [`docs/teaching/sessions.md`](docs/teaching/sessions.md) — that's the session plan.
5. Read the relevant milestone file under [`docs/implementation/tasks/`](docs/implementation/tasks/) — the underlying technical content.
6. Check the Obsidian hub `Lazydap Teaching Sessions.md` (at the user's vault root, accessed via the `obsidian` skill) for what previous sessions covered. Read the most recent session note's "Open questions" + "Teach-back capture" sections.
7. **Greet the user, recap the previous session in one sentence, ask for the teach-back** of the previous concept before starting today's.
8. Start today's session. First move: surface the user's prior model with "How do you think X works?"

**Do not write code or commit anything before step 8.**

### Note capture

Every session generates an Obsidian session note named `Lazydap Session YYYY-MM-DD.md` at the user's vault root, plus atomic concept notes (e.g., `Rust Ownership.md`) for ideas worth long-term retention. Use the **`obsidian` skill** for all vault writes — it encodes the conventions, the linking protocol, and the emergent-synthesis discipline.

The `Lazydap Teaching Sessions.md` hub gets a new row per session.

### Project-portable

Any future project the user opts into teaching mode for copies this same section into its own `AGENTS.md`, points at the same `teaching` skill, and creates its own `<Project> Teaching Sessions.md` hub note. The skill is reusable; per-project setup is just this section + a Sessions hub note.

### Switching out of teaching mode

If the user explicitly says "let's go fast", "just ship it", or "skip teaching today", drop teaching mode for that session. Confirm before resuming teaching mode next session. The `docs/implementation/` task files work directly without the teaching overlay.

## 📁 Project docs: `docs/` is the source of truth

All project documentation lives in [`docs/`](docs/). Three sub-directories matter:

### `docs/blueprint/` — the full project vision

End-to-end design of what we're building. Read this when you need to **recenter** — when you've lost the thread of why we're making a particular decision, or when a new question lands and you need to see how it fits the whole.

Key entry points:

- [`docs/blueprint/00-overview.md`](docs/blueprint/00-overview.md) — what lazydap is, scope, principles
- [`docs/blueprint/01-architecture.md`](docs/blueprint/01-architecture.md) — full architecture
- [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) — every architectural decision with rationale
- [`docs/blueprint/14-roadmap.md`](docs/blueprint/14-roadmap.md) — phased delivery plan

The blueprint is **stable**. Don't edit it without an explicit conversation. New decisions get added as `D0NN` entries to the decision log; reality drift gets captured in `16-addendum.md`.

### `docs/implementation/` — the task manager (ship-mode)

This is **how we track work**. Source-controlled, portable, agent-readable. No GitHub Issues, no Linear, no separate task tool — the implementation directory IS the task list.

**This directory is intentionally clean of teaching content.** If at any point the user decides they've learned enough Rust and wants to hand the project to a coding agent for fast shipping, the implementation/ directory works as-is. No mode-switching ceremony required.

For the parallel **teaching session breakdowns**, see [`docs/teaching/`](docs/teaching/).

Structure:

- [`docs/implementation/README.md`](docs/implementation/README.md) — index of phases
- [`docs/implementation/00-workspace-setup.md`](docs/implementation/00-workspace-setup.md) — prerequisite to M0
- [`docs/implementation/01-phase-A.md`](docs/implementation/01-phase-A.md) through `05-phase-E.md` — phase docs (groups of milestones)
- [`docs/implementation/tasks/M00-...`](docs/implementation/tasks/) through `M18-...` — one MD file per milestone

**How agents work with tasks:**

1. **Pick the next task.** Look at [`/TODO.md`](TODO.md) for current state. The first unchecked milestone is the next one to work on. (Or pick whichever the user names explicitly.)
2. **Read the task file.** Each milestone file (`docs/implementation/tasks/MNN-*.md`) is self-contained: what / why / how / success criteria / files / verify / depends on. Read it fully before starting.
3. **Confirm dependencies.** The task file lists what previous milestones must be complete. Don't skip ahead.
4. **Do the work.** In teaching mode, this means session-by-session through the operating rules.
5. **Mark the task done.** Check the box in `/TODO.md`. Add a brief completion note at the bottom of the task file (date completed, any deviations from the plan, any follow-ups discovered).
6. **Add new tasks.** If a milestone reveals work that needs its own milestone, create a new `MNN-name.md` file in `docs/implementation/tasks/` with the same template. Add it to `/TODO.md` and to the relevant phase doc.

**The implementation directory is the project's working memory.** Treat it that way: write to it, read from it, keep it current.

### `docs/teaching/` — teaching session plan (parallel to implementation)

**Only relevant in teaching mode.** Mirrors `docs/implementation/` but slices each milestone into sessions sized for one-new-concept-per-session discipline. Some milestones are 1 session; the dense ones (M5, M6, M15) are 4–5.

- [`docs/teaching/README.md`](docs/teaching/README.md) — what this directory is, when it applies
- [`docs/teaching/sessions.md`](docs/teaching/sessions.md) — the per-milestone session breakdown

**Important:** the teaching directory is the **plan**. Session **logs** live in Obsidian (`Lazydap Teaching Sessions.md` hub + per-session children). Plan ≠ log; both are useful, neither replaces the other.

When teaching mode ends (user says "let's go fast" or hands off to a build agent), this directory becomes archive. The `docs/implementation/` tasks remain canonical.

### `docs/articles/` and `docs/reference/`

- `docs/articles/` — short essays on positioning and philosophy ([`the-cli-is-the-product.md`](docs/articles/the-cli-is-the-product.md), [`agent-driven-debugging.md`](docs/articles/agent-driven-debugging.md), [`yes-its-a-wrapper.md`](docs/articles/yes-its-a-wrapper.md))
- `docs/reference/` — quick-lookup material ([`how-debuggers-actually-work.md`](docs/reference/how-debuggers-actually-work.md), [`dap-protocol-cheatsheet.md`](docs/reference/dap-protocol-cheatsheet.md), [`ratatui-patterns.md`](docs/reference/ratatui-patterns.md), [`tokio-patterns.md`](docs/reference/tokio-patterns.md))

These accumulate as we go. Add to them whenever a question takes >10 minutes to answer for the second time.

### `/TODO.md` is the lightweight index

Top-level [`TODO.md`](TODO.md) is the at-a-glance task list with checkboxes pointing into `docs/implementation/tasks/`. **It's an index, not a task store** — the per-milestone files have the real content. Keep `/TODO.md` in sync with task completion.

### What this means for you (the agent)

When you start working on lazydap:

1. Read this `AGENTS.md` (you're doing it now)
2. Read [`/TODO.md`](TODO.md) — current state
3. Read the task file for the milestone you're picking up (or being asked to work on)
4. If you need to recenter: skim [`docs/blueprint/00-overview.md`](docs/blueprint/00-overview.md) and [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md)
5. If you discover new work: add a milestone file in `docs/implementation/tasks/` and update `/TODO.md`

Everything is in the repo. Source-controlled. Portable. Reviewable in PRs. No external trackers.

## What lazydap is, in one paragraph

A scriptable, terminal-first debugger. CLI core, JSON-over-Unix-socket protocol, multiple frontends (TUI, agent skill, anything anyone wants to build). You drive it via shell subcommands that return JSON. Auto-detect tty: pipe it, get JSON; run it interactive, get tables.

## Using lazydap as an agent

You invoke lazydap like a human would, but with `--format json` (or just rely on auto-detection — JSON is the default when stdout is not a TTY).

### The agent loop

```bash
# Start a session.
$ lazydap launch ./mybinary --stop-on-entry --format json
{"session_id":"01ABC...", "state":"paused", "reason":"entry", "frame":{"file":"main.c","line":1}, ...}

# Set breakpoints based on the user's question.
$ lazydap break main.c:42 --format json
{"breakpoint_id":1, "verified":true, "file":"main.c", "line":42}

# Run until next stable state. --wait blocks until paused/exited/terminated.
$ lazydap continue --wait --format json
{"state":"paused", "reason":"breakpoint", "breakpoint_id":1, "frame":{...}, "captured_output":[...]}

# Inspect.
$ lazydap stack --format json
$ lazydap scopes --format json
$ lazydap eval "x + y" --format json

# Modify and continue.
$ lazydap continue --wait --format json
```

### The `--wait` flag (critical for agents)

Stepping/continue commands have two modes:

- **Default (no `--wait`):** fire-and-forget. Returns immediately. Useful for human TUI interaction; **rarely useful for you**.
- **`--wait`:** blocks until the program reaches a stable state — paused on a breakpoint, exited cleanly, or terminated (or timed out). Returns one JSON blob describing what happened. **Always use `--wait` from agent code.**

`--wait` accepts `--timeout=N` (seconds, default 30, `0` = infinite). The `LAZYDAP_TIMEOUT` env var sets the default.

The response includes everything that happened during execution:

- `state`: `"paused" | "exited" | "terminated" | "timeout" | "adapter_died"`
- `reason`: why it stopped (breakpoint, step, exception, exit code, ...)
- `frame`: top frame source/line/column when paused
- `captured_output`: array of `{category, output}` from the program's stdout/stderr during the run
- `breakpoint_updates`: any breakpoints whose state changed during the run
- `additional_stopped_threads`: in multi-threaded programs

Don't poll `lazydap status` in a loop. Use `--wait`.

### Output format conventions

- `--format json` — single JSON object or array. Stable schema. Pipe-friendly.
- `--format jsonl` — one JSON object per line. Used for streams (e.g. event logs).
- `--format ids` — bare IDs, one per line. Useful for `xargs`.
- `--format table` — human-readable, default for TTYs. **Do not parse this.**
- `--format csv` — for spreadsheets and ad-hoc tools.

### Discovering commands

```bash
$ lazydap --help                   # top-level
$ lazydap <subcommand> --help      # specific
$ lazydap completions <shell>      # tab-completion install
```

The full reference for agent use lives in `lazydap.skill/references/commands.md`.

### Error handling

Exit codes:

- `0` — success
- `1` — general error (adapter, session, mutation failure)
- `2` — usage error (bad args, unknown subcommand)
- `3` — daemon could not be started or contacted
- `4` — adapter not found / not authorised

Errors print structured JSON to stderr in JSON mode:

```json
{"error":"AdapterCrashed","message":"codelldb exited with code 1","details":{...}}
```

In table mode, errors print human text to stderr. Exit code is the canonical signal.

## Working ON lazydap as an agent

If you've been asked to write code in this repo, read these in order:

1. [`ARCHITECTURE.md`](ARCHITECTURE.md) — the core tenet and crate boundaries
2. [`docs/blueprint/01-architecture.md`](docs/blueprint/01-architecture.md) — expanded architecture
3. [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) — why decisions were made
4. The relevant milestone in `docs/implementation/tasks/MNN-*.md` — what to actually do

### The non-negotiables

These are paid for in pain (mostly mxr's). Violating them creates work for everyone.

1. **Crate boundaries are enforced by Cargo, not convention.** Don't `#[path]` your way around them.
2. **Every TUI action has a CLI equivalent. Both wired or neither.**
3. **JSON output is a product feature.** Stable schema. Don't break it without a `15-decision-log.md` entry.
4. **Mutations are dry-runnable.** `--dry-run` must use the same selection logic as the real mutation.
5. **DAP details stay in adapter crates.** The daemon depends on the `DebugAdapter` trait, not raw DAP messages.
6. **Don't pipeline requests to one adapter.** Queue them.
7. **Tests cross real boundaries.** A `FakeAdapter` exists for unit-style speed; the canonical tests run real codelldb.
8. **`tracing` from the first line of `main`.** No `println!` debug calls.

### What "small blast radius" means here

If your task is "fix a bug in `lazydap continue --wait`":

- Touch only the wait-loop code.
- Don't refactor the surrounding event handler "for consistency."
- Don't delete unused imports you noticed.
- Don't add error handling for cases that can't happen.
- At the end, mention what you noticed but didn't change. The user decides.

### Workflow expectations

- Read the relevant `MNN-*.md` task file before writing code. It tells you what to do, why, and what success looks like.
- If a decision isn't made, ask. Don't fabricate. The user will help reason it out.
- Use `cargo test --workspace` before claiming done. Use `cargo clippy --workspace --all-targets` for lints. Both must pass.
- Update the relevant blueprint or task MD file if your code changes the architecture.
- Don't add a sixth IPC bucket without explicit approval.

## What you (the agent) should NOT do

- Don't add features without a milestone or task file describing them.
- Don't introduce a framework (axum, actix, anyhow-everywhere, etc.) without explicit user approval. The dependency budget is small.
- Don't write Rust that's "clever." Read the code; if a future-you reading this in 6 months would have to think, simplify.
- Don't write tests that mock things lazydap actually owns (the daemon, the store, the adapter trait). Mock external systems only.
- Don't bypass `lazydap.skill`'s CLI surface to call internal APIs. If the agent UX is wrong, fix the CLI.
- Don't add AI features into the core. AI is an external client — same as the TUI, same as everything else.

## When `lazydap` doesn't exist yet (current state)

This repo is in pre-alpha. As of writing, M0 hasn't been started. There's no Cargo workspace, no `lazydap` binary, no daemon. If a user asks you to "use lazydap to debug X", politely point them at [`README.md`](README.md) and the milestone roadmap. Don't pretend the binary exists.

When code starts landing (M0+), this file will gain a "Known good versions" section.

## Glossary (so we don't talk past each other)

- **Adapter** — an external DAP server process (codelldb, debugpy, ...). Speaks DAP. Owns the actual debuggee process.
- **Session** — one active debug session, owned by the daemon, mediated to one adapter.
- **`--wait`** — block until next stable state of the debuggee. The bridge between async DAP events and synchronous shell invocation.
- **Stable state** — paused, exited, or terminated. Querying scopes/stack is only safe in stable states.
- **DAP** — [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/). The thing adapters speak. lazydap users never see it.
- **lazydap protocol** — JSON-over-Unix-socket. The thing clients speak. What lazydap users build on.
