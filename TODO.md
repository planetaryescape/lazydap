# TODO

Living list of what's next. Detailed per-milestone files in [`docs/implementation/tasks/`](docs/implementation/tasks/).

> **This is the shipping repo — ship mode** (see [`/AGENTS.md`](AGENTS.md)). Teaching happens in
> `lazydap-learn`; the teaching material still in this tree is read-only reference. The work loop:
> pick the first unchecked milestone below, read its task file fully, build it, get
> `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
> `cargo check --workspace --all-targets`, and `cargo test --workspace --all-targets` green, tick the
> box here, add a completion note to the task file.

## Now

- **Next milestone: [M3 — Launch and observe](docs/implementation/tasks/M03-launch-and-observe.md)**
- Open decisions O01–O04 resolved 2026-07-30 (root detection order, read-only doctor, adapter
  discovery priority, sibling-ZIP skill distribution) — being recorded as D-entries in
  [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) with the M5 work

### Repo state notes (for cold-start agent)

Repo: [github.com/planetaryescape/lazydap](https://github.com/planetaryescape/lazydap), public.
The `chapter-*` tags/releases and [.github/workflows/release.yml](.github/workflows/release.yml)
are teaching-era machinery: tags mark the *start state* of book chapters. Leave them alone; new
chapter tags are cut from `lazydap-learn`, not here. Product releases (v0.1+) will get their own
workflow at M15.

## Workspace setup (prerequisite to M0)

- [x] [Workspace setup](docs/implementation/00-workspace-setup.md) — Cargo workspace, daemon binary stub, CI, conventions
  - Completed 2026-05-01 across 3 teaching sessions (`WS-1`, `WS-2`, `WS-3`). Initial commit: `6a06e68`.

## Phase A — see the protocol (M0–M4)

- [x] [M0 — Hello, adapter](docs/implementation/tasks/M00-hello-adapter.md) — completed 2026-05-02 (session `M0-1`). Public chapter: [`docs/book/04-hello-adapter.md`](docs/book/04-hello-adapter.md). Two follow-up issues filed: [docs/issues/0001](docs/issues/0001-codelldb-symlink-install-broken.md), [docs/issues/0002](docs/issues/0002-codelldb-version-drift-rust-log.md). New reference: [docs/reference/codelldb-quirks.md](docs/reference/codelldb-quirks.md).
- [x] [M1 — Read one message](docs/implementation/tasks/M01-read-one-message.md) — completed 2026-05-03 (session `M1-1`). Public chapter: [`docs/book/05-read-one-message.md`](docs/book/05-read-one-message.md). Side win: `verify-before-publishing` framework propagated to teaching/bookgen skills + global CLAUDE.md after live version-drift hang surfaced the principle.
- [x] [M2 — Initialize handshake](docs/implementation/tasks/M02-initialize-handshake.md) — completed across two sessions: `M2-1` (typed structs, 2026-05-03) and `M2-2` (transport + atomic seq + thiserror, 2026-05-04). Public chapters: [`docs/book/06-serde-typed-protocols.md`](docs/book/06-serde-typed-protocols.md), [`docs/book/07-dap-transport-and-seq.md`](docs/book/07-dap-transport-and-seq.md). End-state: `cargo run --example m2_initialize` round-trips a typed initialize against real codelldb.
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
- Cargo "locator": resolve `cargo build` artifacts into codelldb launch configs without a launch.json (Zed-style)

## Open decisions awaiting input

Tracked in [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) under "Open" status.
O01–O04 answered 2026-07-30; remaining open question for M15: whether to publish crates to crates.io
(default: no — `publish = false` stays, matching mxr's stance).
