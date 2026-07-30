# TODO

Living list of what's next. Detailed per-milestone files in [`docs/implementation/tasks/`](docs/implementation/tasks/).

> **This is the shipping repo — ship mode** (see [`/AGENTS.md`](AGENTS.md)). Teaching happens in
> `lazydap-learn`; the teaching material still in this tree is read-only reference. The work loop:
> pick the first unchecked milestone below, read its task file fully, build it, get
> `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
> `cargo check --workspace --all-targets`, and `cargo test --workspace --all-targets` green, tick the
> box here, add a completion note to the task file.

## Now

- **Next milestone: [M12 — Stack pane](docs/implementation/tasks/M12-stack-pane.md)** — Phase C is
  done. The TUI is real: it connects to the daemon as a client, subscribes to events, shows the
  source with a marker that follows the debuggee, and drives the program with F5/F10/F11/S-F11
  through the same requests the CLI sends. Phase D adds the panes that make it useful.
- Open decisions O01–O04 resolved 2026-07-30 and recorded as D024–D027 in
  [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md), alongside D028 (codec),
  D029 (adapter seam), D030 (`SessionId` form), D031–D036 from M6/M7 (breakpoint ids, protocol
  v2, the two codelldb normalisations, skill generation, and what `--dry-run` means per command),
  and D037–D039 from Phase C (the daemon→TUI dependency arrow, what `Subscribe` answers with, and
  how the TUI is verified)

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
- [x] [M3 — Launch and observe](docs/implementation/tasks/M03-launch-and-observe.md) — completed 2026-07-30. `cargo run --example m3_launch_and_observe` launches `examples/c-hello/build/hello` under codelldb and streams the whole event sequence to termination, capturing both debuggee lines as DAP `output` events. Transport gained `Incoming` + `send_request` + `read_incoming`.
- [x] [M4 — Pause on breakpoint](docs/implementation/tasks/M04-pause-on-breakpoint.md) — completed 2026-07-30. `cargo run --example m4_pause_on_breakpoint` sets a breakpoint on `examples/c-hello/main.c:19`, hits it (`stopped`, reason `breakpoint`), resumes, and runs to `terminated`. Phase A complete.

## Phase B — daemon + protocol (M5–M7)

- [x] [M5 — IPC protocol + daemon binary](docs/implementation/tasks/M05-ipc-protocol-daemon.md) — completed 2026-07-30. The binary is now `lazydap`. New crates `lazydap-protocol` (envelope + codec) and `lazydap-config` (paths, project-root detection); the daemon serves a Unix socket, auto-spawns, owns one session (D007), and runs a per-session read pump that resolves M3's cancellation-safety debt. Subcommands: `launch`, `status`, `disconnect`, `shutdown`, `daemon`. Boundary check wired into CI.
- [x] [M6 — CLI subcommands talk to daemon](docs/implementation/tasks/M06-cli-subcommands.md) — completed 2026-07-30. The full surface: stepping with `--wait`, `break`/`stack`/`scopes`/`variables`/`eval`/`threads`/`output`, `doctor`/`version`/`logs`/`completions`, and `table`/`json`/`jsonl`/`csv`/`ids`. New crate `lazydap-store` persists breakpoints to `.lazydap/state.toml` (D006) and applies them during each launch's configuration phase. Protocol goes to v2 (D032). Live verification against real codelldb found three bugs, including one that made `eval` unusable (D034).
- [x] [M7 — Skill + agent verification](docs/implementation/tasks/M07-skill-agent-verification.md) — completed 2026-07-30. `lazydap.skill` at the repository root, built reproducibly from `skill/` by `scripts/build-skill.sh`; `references/commands.md` is generated from the clap tree (D035) and CI fails if the committed artefacts drift. **Phase B complete.**

## Phase C — TUI (M8–M11)

- [x] [M8 — Hello ratatui](docs/implementation/tasks/M08-hello-ratatui.md) — completed 2026-07-30. New crate `lazydap-tui` (ratatui 0.30, crossterm via ratatui's re-export). Bare `lazydap` on a terminal opens it; `echo "" | lazydap` prints help, because the tty check is stdin **and** stdout. `lazydap tui` is the explicit spelling. Boundary script gains the `lazydap-tui` row (D037).
- [x] [M9 — Show a file](docs/implementation/tasks/M09-show-a-file.md) — completed 2026-07-30. Source pane with line numbers, `j`/`k`/arrows/`<C-d>`/`<C-u>`/`gg`/`G`, scrolling that keeps the cursor on screen. Not wrapped, and `<C-d>` is half the *visible* height rather than a fixed ten lines.
- [x] [M10 — Elm-ify the loop](docs/implementation/tasks/M10-elm-ify.md) — completed 2026-07-30. `state`/`msg`/`update`/`view` per D012; behaviour identical to M9, checked screen by screen in a pseudo-terminal. The loop turns async here (`select!` over input and a tick, crossterm's blocking poll on `spawn_blocking`).
- [x] [M11 — Wire IPC into TUI](docs/implementation/tasks/M11-wire-ipc-into-tui.md) — completed 2026-07-30. `Subscribe` implemented daemon-side and answered with a state snapshot (D038); the TUI connects as an ordinary client, F5/F10/F11/S-F11 send the same requests the CLI's subcommands send, and `stopped` events move the marker. **Phase C complete.**

## Phase D — useful features (M12–M15) → v0.1

- [ ] [M12 — Stack pane](docs/implementation/tasks/M12-stack-pane.md)
- [ ] [M13 — Scopes pane with expansion](docs/implementation/tasks/M13-scopes-pane.md)
- [ ] [M14 — Toggle breakpoint from TUI](docs/implementation/tasks/M14-toggle-breakpoint.md)
- [ ] [M19 — TUI reconnects when the daemon goes away](docs/implementation/tasks/M19-tui-reconnect.md) — added 2026-07-30; M11 recorded reconnection as mandatory pre-v0.1
- [ ] [M15 — Config file + launch.json import](docs/implementation/tasks/M15-config-file.md) → **tag v0.1**

## Beyond v0.1 (M16–M18+)

- [ ] [M20 — Documentation website](docs/implementation/tasks/M20-docs-site.md) — Astro/Starlight site mirroring mxr's; added 2026-07-30 on user request. Runs in parallel with the milestone track; not gated on v0.1.
- [ ] [M16 — Watches](docs/implementation/tasks/M16-watches.md)
- [ ] [M17 — REPL pane](docs/implementation/tasks/M17-repl-pane.md)
- [ ] [M18 — Second adapter (debugpy)](docs/implementation/tasks/M18-second-adapter.md)

## Known follow-ups (post-v0.1, no milestone yet)

- Multi-session support (currently single-session-per-daemon enforced; protocol uses session IDs from M5 to keep this option open)
- `js-debug` adapter for Node/TS
- `delve` adapter for Go
- Conditional breakpoints in the TUI (the CLI already ships them — `break --condition`, discovered working 2026-07-30; only the TUI can't set one)
- Restart / disconnect-and-relaunch
- Theming + mouse support
- HTTP bridge (separate crate, optional binary)
- AI advisor extension points (see [`docs/blueprint/12-ai-future.md`](docs/blueprint/12-ai-future.md))
- Cargo "locator": resolve `cargo build` artifacts into codelldb launch configs without a launch.json (Zed-style)

## Open decisions awaiting input

Tracked in [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) under "Open" status.
O01–O04 answered 2026-07-30; remaining open question for M15: whether to publish crates to crates.io
(default: no — `publish = false` stays, matching mxr's stance).
