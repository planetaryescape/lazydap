# AGENTS.md — guidance for AI agents

Read this when you (an AI agent — Claude, Cursor, Copilot, etc.) are asked to work on lazydap or use lazydap to debug code. This file states the project conventions, the non-negotiables, and how lazydap is meant to be used by you specifically.

## ⚠️ This is the shipping repository — teaching mode is OFF

**Do not teach. Do not slow down for pedagogy. Build.**

lazydap exists in two parallel repositories. This is the shipping one:

| | This repo (`lazydap`) | Learning repo (`lazydap-learn`) |
|---|---|---|
| Path | `~/code/planetaryescape/lazydap` | `~/code/planetaryescape/lazydap-learn` |
| Goal | **a shippable debugger** | **the user's understanding of Rust** |
| Pace | as fast as correctness allows | one new concept per session |
| Who drives | coding agents, largely autonomous | the user, with an agent teaching |
| Canonical docs | `docs/implementation/tasks/` | `docs/teaching/` + `docs/book/` |
| Session logs | none | Obsidian vault |

If you were asked to teach, explain slowly, ask the user to predict output, or run a
session from `docs/teaching/sessions.md` — **you are in the wrong repository.** Say so and
point at `~/code/planetaryescape/lazydap-learn`.

### How to work here

1. Read [`/TODO.md`](TODO.md) — the first unchecked milestone is next.
2. Read that milestone's file in [`docs/implementation/tasks/`](docs/implementation/tasks/) fully. It is self-contained: what / why / how / success criteria / files / verify / depends on.
3. Confirm its listed dependencies are complete. Don't skip ahead.
4. Build it. Run `cargo test --workspace` and `cargo clippy --workspace --all-targets` — both must pass before you claim done.
5. Check the box in `/TODO.md`, add a completion note at the bottom of the task file (date, deviations, follow-ups discovered).
6. If a milestone reveals work needing its own milestone, add `MNN-name.md` to `docs/implementation/tasks/` and index it in `/TODO.md` + the phase doc.

Ask when a decision genuinely isn't made — don't fabricate architecture. Everything else,
just do. The **non-negotiables** further down this file still apply in full; they are the
one thing shipping speed does not get to trade away.

### The teaching material that's still in this repo

`docs/teaching/`, `docs/book/`, `docs/chain/`, `.skills/teaching/`, and `.bookgen/` are
retained here as **reference, not instruction**. The chapters are often the clearest
written explanation of why a given piece is shaped the way it is — read them when you need
that. Never run them as a session, and don't maintain them here; `lazydap-learn` owns them.

The vendored copy at [`.skills/teaching/`](.skills/teaching/SKILL.md) is a frozen **v1.0.0**
snapshot (2026-05-02) — deliberately not kept current here. `lazydap-learn` tracks the live
version. Don't run bookgen's updater against this repo; it would re-vendor teaching
machinery this repo has no use for.

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

**This directory is intentionally clean of teaching content** — which is precisely why this
repo can ship fast. It works as-is for an agent, with no pedagogical overlay to strip out.
It is the canonical task list here.

Structure:

- [`docs/implementation/README.md`](docs/implementation/README.md) — index of phases
- [`docs/implementation/00-workspace-setup.md`](docs/implementation/00-workspace-setup.md) — prerequisite to M0
- [`docs/implementation/01-phase-A.md`](docs/implementation/01-phase-A.md) through `05-phase-E.md` — phase docs (groups of milestones)
- [`docs/implementation/tasks/M00-...`](docs/implementation/tasks/) through `M18-...` — one MD file per milestone

**How agents work with tasks:**

1. **Pick the next task.** Look at [`/TODO.md`](TODO.md) for current state. The first unchecked milestone is the next one to work on. (Or pick whichever the user names explicitly.)
2. **Read the task file.** Each milestone file (`docs/implementation/tasks/MNN-*.md`) is self-contained: what / why / how / success criteria / files / verify / depends on. Read it fully before starting.
3. **Confirm dependencies.** The task file lists what previous milestones must be complete. Don't skip ahead.
4. **Do the work.** End to end, in one pass where you can. `cargo test --workspace` and `cargo clippy --workspace --all-targets` green before you call it done.
5. **Mark the task done.** Check the box in `/TODO.md`. Add a brief completion note at the bottom of the task file (date completed, any deviations from the plan, any follow-ups discovered).
6. **Add new tasks.** If a milestone reveals work that needs its own milestone, create a new `MNN-name.md` file in `docs/implementation/tasks/` with the same template. Add it to `/TODO.md` and to the relevant phase doc.

**The implementation directory is the project's working memory.** Treat it that way: write to it, read from it, keep it current.

### `docs/teaching/` and `docs/book/` — archive in this repo

Read-only here. `docs/teaching/` slices milestones into learning sessions and
`docs/book/` holds the written chapters. Both are **owned by `lazydap-learn`** — edit them
there, not here. In this repo they're useful for one thing: when you need to know why a
piece of already-written code is shaped the way it is, the chapter covering that milestone
usually explains it better than the blueprint does.

Do not run a session from `docs/teaching/sessions.md`. Do not update it when you complete a
milestone — update `/TODO.md` and the task file instead.

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

## Current state (verified 2026-07-30)

**Pre-alpha.** Most of the CLI documented above is still the *target design*. Five subcommands
are real; assume nothing else is. What exists today:

- **Cargo workspace**, edition 2024, `rust-version = "1.85"`, five crates: `lazydap-core`, `lazydap-protocol`, `lazydap-config`, `lazydap-dap`, `lazydap-daemon`.
- **One binary: `lazydap`** (built from `crates/daemon`). `cargo install --path crates/daemon` installs it.
- **Working subcommands:** `launch`, `status`, `disconnect`, `shutdown`, `daemon`. Both `--format table` and `--format json`, auto-detected from the tty. Everything else in this file — `break`, `continue`, `stack`, `scopes`, `eval`, `--wait` — **does not exist yet** (M6).
- **A real daemon:** per-project instance, auto-spawns on first use, Unix socket with length-delimited JSON, one debug session at a time (D007), a per-session read pump, and events buffered per session.
- **Working DAP plumbing** in `lazydap-dap`: framed read/write, typed `initialize`, and a splittable transport.
- **Five runnable examples:** `cargo run --example m0_hello_adapter`, `m1_read_one_message`, `m2_initialize`, `m3_launch_and_observe`, `m4_pause_on_breakpoint`.
- All four gates pass, plus `bash scripts/check_architecture_boundaries.sh`.
- **Milestones complete:** workspace setup, M0–M5. **Next up: M6 — CLI subcommands talk to daemon.**

If a user asks you to debug something lazydap cannot do yet, say which subcommands exist and
point at the roadmap. Don't pretend the rest of the CLI is there.

**This paragraph will go stale.** [`/TODO.md`](TODO.md) is the live source of truth — trust
its checkboxes over this list, and correct this section when you notice it has drifted.

## Glossary (so we don't talk past each other)

- **Adapter** — an external DAP server process (codelldb, debugpy, ...). Speaks DAP. Owns the actual debuggee process.
- **Session** — one active debug session, owned by the daemon, mediated to one adapter.
- **`--wait`** — block until next stable state of the debuggee. The bridge between async DAP events and synchronous shell invocation.
- **Stable state** — paused, exited, or terminated. Querying scopes/stack is only safe in stable states.
- **DAP** — [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/). The thing adapters speak. lazydap users never see it.
- **lazydap protocol** — JSON-over-Unix-socket. The thing clients speak. What lazydap users build on.
