# TODO

Living list of what's next. Detailed per-milestone files in [`docs/implementation/tasks/`](docs/implementation/tasks/).

## Current teaching session

> **Project is in teaching mode** (see [`/AGENTS.md`](AGENTS.md) for the protocol). Sessions are smaller than milestones — each session covers one new concept.

**Next session: `M0-1` — Hello, adapter.** First real milestone.

- Plan: [`docs/teaching/sessions.md`](docs/teaching/sessions.md) — search for `M0-1`
- Underlying milestone: [`docs/implementation/tasks/M00-hello-adapter.md`](docs/implementation/tasks/M00-hello-adapter.md)
- Last session: `WS-3` — Convention as code (2026-05-01). Obsidian: `Lazydap Session 2026-05-01 WS-3.md`. Atomic concept: `Rust Project Conventions.md`.
- Obsidian hub: `Lazydap Teaching Sessions.md` (vault root) — log goes here

WS-3 deliverable: lazydap is conventions-complete. `rust-toolchain.toml`, `rustfmt.toml`, `clippy.toml`, dual licenses, `.github/workflows/ci.yml` (fmt/clippy/check/test) all in place. First commit landed: `6a06e68 chore: initial workspace scaffold`.

**All WS-* sessions complete. Next is M0.** Pre-session todo: install codelldb if not already present (do this in-session, not before).

When all WS-* sessions are done, check the workspace setup box below.

If the user says "drop teaching mode," skip the teaching column and pick milestones directly from the lists below.

## Workspace setup (prerequisite to M0)

- [x] [Workspace setup](docs/implementation/00-workspace-setup.md) — Cargo workspace, daemon binary stub, CI, conventions
  - Completed 2026-05-01 across 3 teaching sessions (`WS-1`, `WS-2`, `WS-3`). Initial commit: `6a06e68`.

## Now

- Decisions to confirm with user (see `docs/blueprint/15-decision-log.md` for in-flight items)
- Begin teaching session `WS-1`

## Phase A — see the protocol (M0–M4)

- [ ] [M0 — Hello, adapter](docs/implementation/tasks/M00-hello-adapter.md)
- [ ] [M1 — Read one message](docs/implementation/tasks/M01-read-one-message.md)
- [ ] [M2 — Initialize handshake](docs/implementation/tasks/M02-initialize-handshake.md)
- [ ] [M3 — Launch and observe](docs/implementation/tasks/M03-launch-and-observe.md)
- [ ] [M4 — Pause on breakpoint](docs/implementation/tasks/M04-pause-on-breakpoint.md)

## Phase B — daemon + protocol (M5–M7)

- [ ] [M5 — IPC protocol + daemon binary](docs/implementation/tasks/M05-ipc-protocol-daemon.md)
- [ ] [M6 — CLI subcommands talk to daemon](docs/implementation/tasks/M06-cli-subcommands.md)
- [ ] [M7 — Skill + agent verification](docs/implementation/tasks/M07-skill-agent-verification.md)

## Phase C — TUI (M8–M11)

- [ ] [M8 — Hello ratatui](docs/implementation/tasks/M08-hello-ratatui.md)
- [ ] [M9 — Show a file](docs/implementation/tasks/M09-show-a-file.md)
- [ ] [M10 — Elm-ify the loop](docs/implementation/tasks/M10-elm-ify.md)
- [ ] [M11 — Wire IPC into TUI](docs/implementation/tasks/M11-wire-ipc-into-tui.md)

## Phase D — useful features (M12–M15) → v0.1

- [ ] [M12 — Stack pane](docs/implementation/tasks/M12-stack-pane.md)
- [ ] [M13 — Scopes pane with expansion](docs/implementation/tasks/M13-scopes-pane.md)
- [ ] [M14 — Toggle breakpoint from TUI](docs/implementation/tasks/M14-toggle-breakpoint.md)
- [ ] [M15 — Config file + launch.json import](docs/implementation/tasks/M15-config-file.md) → **tag v0.1**

## Beyond v0.1 (M16–M18+)

- [ ] [M16 — Watches](docs/implementation/tasks/M16-watches.md)
- [ ] [M17 — REPL pane](docs/implementation/tasks/M17-repl-pane.md)
- [ ] [M18 — Second adapter (debugpy)](docs/implementation/tasks/M18-second-adapter.md)

## Known follow-ups (post-v0.1, no milestone yet)

- Multi-session support (currently single-session-per-daemon enforced; protocol uses session IDs from M5 to keep this option open)
- `js-debug` adapter for Node/TS
- `delve` adapter for Go
- Conditional breakpoints (UI + protocol)
- Restart / disconnect-and-relaunch
- Theming + mouse support
- HTTP bridge (separate crate, optional binary)
- AI advisor extension points (see [`docs/blueprint/12-ai-future.md`](docs/blueprint/12-ai-future.md))

## Open decisions awaiting input

Tracked in [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) under "Open" status.
