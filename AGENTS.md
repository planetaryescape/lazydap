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
- [`docs/implementation/tasks/M00-...`](docs/implementation/tasks/) through `M24-...` — one MD file per milestone

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

`--wait` accepts `--timeout=N` (seconds, default 30, `0` = infinite). The `LAZYDAP_TIMEOUT` env var sets the default; a value that is not a number of seconds is a usage error (exit 2) rather than an ignored one.

The response includes everything that happened during execution:

- `state`: `"paused" | "exited" | "terminated" | "timeout" | "adapter_died"`
- `reason`: why it stopped (breakpoint, step, exception, exit code, ...)
- `frame`: top frame source/line/column when paused
- `captured_output`: array of `{category, output}` from the program's stdout/stderr during the run
- `breakpoint_updates`: any breakpoints whose state changed during the run
- `additional_stopped_threads` and `thread_updates`: **empty against codelldb, and against
  any adapter that reports a multi-threaded stop as one event.** Both are filled from
  messages codelldb does not send. Four threads stopping simultaneously on four distinct
  breakpoints produce exactly one `stopped` event with `allThreadsStopped: true` — and
  `additional_stopped_threads` is filled only from a *second* `Stopped`, so it stays `[]`.
  `thread_updates` comes from DAP `thread` events, of which codelldb emitted none in a full
  session. Read `all_threads_stopped` for "did everything stop", and `lazydap threads` for
  which threads exist. debugpy and delve do send per-thread events, so the fields carry
  something there.

Don't poll `lazydap status` in a loop. Use `--wait`.

A session that ends on its own — the program exited, or the adapter terminated it — needs no
closing `lazydap disconnect`: the daemon disconnects and reaps the adapter itself as soon as
the session ends, and the next `launch` reuses the slot. Disconnect when *you* want to stop
early, not as cleanup after a `--wait` that came back `exited`.

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

In table mode, errors print human text to stderr. Exit code is the canonical signal, and `error` agrees with it: every `UsageError` exits 2, every `DaemonUnreachable` exits 3, every `AdapterNotFound` exits 4.

Writing to a closed pipe is not an error. `lazydap break --list --format jsonl | head -1` ends with exit 0 — the reader went away on purpose.

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
7. **Tests cross real boundaries.** There is no `FakeAdapter`; `AdapterHandle::detached()` (`#[cfg(test)]`) stands in for one where the thing under test is session bookkeeping. The canonical tests run real codelldb, debugpy and delve.
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

### Release shorthand

If the user says `ship it`, run the full release flow:

1. Finalise `CHANGELOG.md`: the top section gets today's date and no unreleased wording —
   the release workflow's guard refuses to publish a section matching
   `unreleased|not (yet )?tagged|until the tag is cut`, on purpose.
2. Bump the workspace version in the root `Cargo.toml` if the current version's tag already
   exists. Never overwrite a tag or a GitHub release; releases are immutable — a bad one is
   followed by a fixed one, not replaced.
3. Full gates on the exact commit being shipped: `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets`, `cargo test --workspace --all-targets`,
   `bash scripts/check_architecture_boundaries.sh`, `scripts/build-skill.sh` + clean
   `git diff -- skill/ lazydap.skill`, and `cd site && npm run build`. Echo the literal
   test count; an empty gate readout is a failed gate.
4. Commit, push `main`, wait for CI green on that push before tagging.
5. `git tag -a v{version}` (annotated; plain `git tag` fails wanting a message) and push the
   tag. The workflow re-runs gates, builds macOS arm64/x86_64 + Linux x86_64, and publishes
   a GitHub release with tarballs, SHA-256 sums, and `lazydap.skill` attached. `v0.*`
   publishes as a prerelease. Rehearse first with `workflow_dispatch` if wanted — it runs
   gates and builds and deliberately stops before publishing.
6. Verify every install channel in throwaway locations, never against the real install:
   - **Release tarball**, following the release notes verbatim into a scratch `HOME`:
     `shasum -a 256 -c` → `tar -xzf` → `install` → `lazydap version --format json` must
     report the new version.
   - **Fresh clone**: `git clone` to a scratch dir, then
     `cargo install --path crates/daemon --root "$(mktemp -d)"`, run `bin/lazydap version`.
   - **Git install**: `cargo install --git https://github.com/planetaryescape/lazydap
     --tag v{version} --locked --root "$(mktemp -d)" lazydap-daemon`, run
     `bin/lazydap version`.
   - **install.sh**, with no version argument so it resolves the newest release for itself:
     `LAZYDAP_INSTALL_DIR="$(mktemp -d)" ./install.sh`, then run `lazydap version` out of
     that directory. It must report the new version, which also proves the release assets
     and their `.sha256` files are both attached and agree.
   - **Homebrew**: `brew update && brew install planetaryescape/lazydap/lazydap`, run
     `lazydap version`, then `HOMEBREW_NO_AUTOREMOVE=1 brew uninstall lazydap`. Keep that
     variable — a plain `brew uninstall` sweeps unrelated orphaned formulae off the
     machine on its way out. If the workflow's `homebrew` job logged
     `HOMEBREW_TAP_TOKEN not set; skipping tap update`, the tap still carries the previous
     version and this channel has not shipped.
7. One real debug session against the released tarball's binary: `break`, `launch`,
   `continue --wait` to a breakpoint, `disconnect`, `shutdown`, zero strays
   (`pgrep -x codelldb`, `pgrep -x lazydap`, and `pgrep -f` on the fixture path).
8. Redeploy the docs site if `site/` changed since the last deploy:
   `cd site && vercel deploy --prod --scope planetaryescape`, then confirm
   `https://lazydap.sh` serves the new content.
9. Refresh the user's local install and skill: `cargo install --path crates/daemon --force`
   and re-extract `lazydap.skill` into `~/.claude/skills/lazydap/` (which is
   `~/.dotfiles/.agents/skills/lazydap/` — same directory through the symlink chain).
10. Report: tag, release URL, per-channel verification output, and anything skipped.

Source of truth: the steps above and
[`.github/workflows/product-release.yml`](.github/workflows/product-release.yml), which is
what actually runs from the tag. The protocol version does not need bumping for a
release — only for wire-shape changes (see D032/D043/D050/D056 for what counts).

## What you (the agent) should NOT do

- Don't add features without a milestone or task file describing them.
- Don't introduce a framework (axum, actix, anyhow-everywhere, etc.) without explicit user approval. The dependency budget is small.
- Don't write Rust that's "clever." Read the code; if a future-you reading this in 6 months would have to think, simplify.
- Don't write tests that mock things lazydap actually owns (the daemon, the store, the adapter trait). Mock external systems only.
- Don't bypass `lazydap.skill`'s CLI surface to call internal APIs. If the agent UX is wrong, fix the CLI.
- Don't add AI features into the core. AI is an external client — same as the TUI, same as everything else.

## Current state (verified 2026-08-18)

**Prerelease — every `v0.*` tag ships as one — but the CLI above is real.** The agent loop
documented in this file works end to end against C, Python and Go programs. What exists today:

- **Cargo workspace**, edition 2024, `rust-version = "1.85"`, seven crates: `lazydap-core`, `lazydap-protocol`, `lazydap-config`, `lazydap-dap`, `lazydap-store`, `lazydap-tui`, `lazydap-daemon`.
- **One binary: `lazydap`** (built from `crates/daemon`). `cargo install --path crates/daemon` installs it.
- **`lazydap doctor` passes when lazydap can debug something**, not when this machine has every adapter (`D093`). A missing adapter is reported as `missing` with an install hint and does not fail the run; the config file, the state file and the daemon still have to be sound. `doctor --check-state` reads `.lazydap/state.toml` in the CLI process and starts no daemon, which is how you diagnose a state file that stops one from starting.
- **Working subcommands:** `launch`, `launches` (`list`/`run`), `status`, `disconnect`, `shutdown`, `daemon`, `tui`, `continue`, `step` (alias `next`), `step-in`, `step-out`, `pause`, `break` (add/list/remove/toggle), `watch` (`add`/`list`/`remove`), `stack`, `scopes`, `variables`, `eval`, `threads`, `output`, `doctor`, `version`, `logs`, `completions`. `--wait` and `--timeout` on everything that moves the program.
- **A TUI.** Bare `lazydap` on a terminal opens it (`lazydap tui` is the explicit spelling); anywhere else — a pipe, a CI job — it prints help instead. It is a **client**, with no path to the daemon's internals: it connects over the same socket, subscribes to events, and F5/`c`, F10/`n`, F11 and shift-F11 send the requests behind `continue`, `step`, `step-in` and `step-out`. `j`/`k`/`<C-d>`/`<C-u>`/`gg`/`G` move the view; `q` leaves without ending the session. Five panes: source, stack, scopes, watches and a REPL, with `Tab` cycling the focus between them, `<CR>` jumping to a frame or expanding a variable, `b` toggling a breakpoint on the cursor line, and `a`/`dd` adding and removing watches — all through the same requests the CLI sends. It reconnects on its own when the daemon goes away, starting one if there is none. **Note the REPL takes the keyboard:** with the cursor in it `q` is a `q`, so `Esc` leaves the pane before `q` will quit.
- **All five formats:** `table`, `json`, `jsonl`, `csv`, `ids`, auto-detected from the tty.
- **A real daemon:** per-project instance, auto-spawns on first use, Unix socket with length-delimited JSON, one debug session at a time (D007), a per-session read pump, and events buffered per session.
- **Persistent breakpoints** in `.lazydap/state.toml` (D006), applied during each launch's configuration phase and surviving both the session and the daemon.
- **Persistent watch expressions**, in the same file and with the same discipline (D056). `lazydap watch add/list/remove` sets them without a session; the TUI's watches pane re-evaluates every one of them at each stop, and again when you select another frame. Only the *expressions* are stored — a value belongs to one stop, and a file claiming `pos = 4` tomorrow would be lying.
- **The agent skill**, `lazydap.skill` at the repository root, built by `scripts/build-skill.sh` from `skill/`.
- **Three adapters:** codelldb for C, C++ and Rust, debugpy for Python, delve for Go. The program's extension picks one — `.py` debugpy, `.go` delve, anything else codelldb — and `lazydap launch --adapter <name>` overrides it. All three live in `crates/daemon/src/adapter/` behind the `DebugAdapter` trait (D052), and each has its own quirks file under [`docs/reference/`](docs/reference/).
- **Two adapter normalisations you should know about:** `--stop-on-entry` reports `reason: "entry"` with the adapter's `"exception"` kept in `raw_reason` (D033), and `eval` defaults to the `watch` context because `repl` runs an LLDB *command* (D034). Both are in [`docs/reference/codelldb-quirks.md`](docs/reference/codelldb-quirks.md).
- **`Subscribe` and live event streaming.** A subscribed connection is pushed event frames as they happen, filtered to the kinds it asked for, interleaved with its own replies. It is answered with a `Response::Status` snapshot taken at the moment the stream starts, and replays nothing (D038).
- **Launch configurations, read from two files.** `lazydap launches list` merges `.lazydap/state.toml`'s `[[launch_configs]]` with `.vscode/launch.json` (JSONC: comments and trailing commas), expands `${workspaceFolder}` and friends, and marks each one runnable or not with the reason. `lazydap launches run <name>` sends the same `Launch` request `lazydap launch` does — resolution is client-side, because both files are found by walking up from *your* working directory, not the daemon's (D047).
- **A user config file** at `~/.config/lazydap/config.toml`, `$XDG_CONFIG_HOME/lazydap/config.toml`, or `LAZYDAP_CONFIG_PATH` — first that exists wins, platform config dir searched last (D049). Two settings are consumed: `[adapter.<name>] command` — `codelldb`, `debugpy` or `delve`, D026's first discovery tier, ahead of `PATH` — and `[general] wait_timeout_seconds`. Everything else in the blueprint's schema parses and is ignored — deliberately, rather than being modelled as fields nothing reads.
- **Breakpoints bind under symlinked paths.** A file whose breakpoints the adapter declines while naming a location it could have used is re-sent under that name, once, when nothing in it bound and the suggestion resolves to the same file (D048, quirk 8). This is what makes a debuggee under `/tmp` on macOS work.
- **Not yet:** `attach` (M24, next), `until`, `source`, `restart`, conditional breakpoints from the TUI, js-debug for Node (M23, blocked at its own scope gate), the rest of the config schema.
- All four gates pass, plus `bash scripts/check_architecture_boundaries.sh`.
- **Milestones complete:** workspace setup and M0–M22. Phases A, B, C and D are done. M23 and M24 are the two open ones. Released: `v0.1.0` on 2026-07-31, then `v0.2.0`–`v0.2.8` on 2026-08-18 and `v0.2.9` on 2026-08-19 out of a defect campaign; every one of them went out through `.github/workflows/product-release.yml`.

Note the protocol is at **v10** (D101: `Request::Doctor` lost `check_adapters`
and `check_state` — both checks have run in the client since D093 — so it now goes
on the wire as `"Doctor"`, a shape neither an older nor a newer peer can decode.
v9 was D086: `action` on a breakpoint report gained
`updated` and `unchanged`, because setting a location that already has a
breakpoint now *edits* it — keeping its id, and clearing the modifiers the new
command left out — rather than returning the old one untouched. v8 was
D075–D081: a `frame_id` and a
`variables_reference` are now lazydap's own handles, minted per stop and never
reused, so one from an earlier stop is refused with `StaleHandle` instead of
colliding with a number the adapter has since recycled; `Response::Continued`
gained `already_running`; `Response::Variables` became a struct with a
`truncated` flag; the `--wait` blob gained `user_frame` and `locals`; and
`ErrorCode` gained `StaleHandle`. v7 was D065–D069: `ThreadInfo::name` became optional,
`Event::Stopped` and the `--wait` blob gained `adapter_thread_id`, `AdapterCapabilities`
gained `supports_variable_paging`, and a variable gained `evaluate_name` — none of them a
new request, so a v6 daemon decodes what a v7 client sends and then answers `threads` in a
shape this build cannot read; v6 was D061, a third `AdapterKind`; v5 was D056, the watch
requests and `Event::WatchUpdated`; v4 was D050, `LaunchRequest` carrying the adapter binary
the *client* resolved; v3 was D043, `BreakpointUpdated` distinguishing an adapter's opinion
from a change to the project's list). A daemon left running from an older build is
refused with `VersionMismatch`; `lazydap shutdown` clears it and the next command starts a
current one — and the TUI now does that for itself.

The full per-command JSON is `skill/references/output-schemas.md`, which is **hand-written
and therefore the thing most likely to have drifted** — check it against
`crates/protocol/src/types.rs` when it matters. Last swept against the binary 2026-08-18. `commands.md` next to it is generated from
the real `Cli` type and does not drift.

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
