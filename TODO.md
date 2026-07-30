# TODO

Living list of what's next. Detailed per-milestone files in [`docs/implementation/tasks/`](docs/implementation/tasks/).

> **This is the shipping repo — ship mode** (see [`/AGENTS.md`](AGENTS.md)). Teaching happens in
> `lazydap-learn`; the teaching material still in this tree is read-only reference. The work loop:
> pick the first unchecked milestone below, read its task file fully, build it, get
> `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
> `cargo check --workspace --all-targets`, and `cargo test --workspace --all-targets` green, tick the
> box here, add a completion note to the task file.

## Now

- **Every milestone through v0.1 is done. The next move is a decision, not a milestone: cut
  `v0.1.0`, or hold.** M15 landed the config file, `launch.json` import, `lazydap launches`, and
  quirk 8's fix; the `[0.1.0]` CHANGELOG section is dated and finalised, which is precisely what
  `.github/workflows/product-release.yml` refuses to publish without. Everything below is what a
  tag needs, and none of it is code:
  1. **Decide the crates.io question** (see "Open decisions" in
     [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md)). Default is no —
     `publish = false` stays on all seven crates, and the workflow has no publish job. Deciding it
     is a D-entry, not a change of plan.
  2. **Confirm the CHANGELOG date.** It reads `## [0.1.0] — 2026-07-30`. If the tag is cut on a
     different day, change that line in the same commit that precedes the tag.
  3. **Tag and push:** `git tag v0.1.0 && git push origin v0.1.0`. The workflow re-runs every gate
     against the tagged commit, checks the tag matches the workspace version, builds macOS
     arm64/x86_64 and Linux x86_64, and publishes a GitHub Release with tarballs, SHA-256 sums and
     `lazydap.skill` attached, with notes taken from the CHANGELOG section. A `v0.*` tag goes out
     as a prerelease.
  4. **Rehearse first if you want to**: `workflow_dispatch` runs the gates and the builds and stops
     before publishing, deliberately.
  5. **The demo GIF** is still missing (M15's step 4). It is not a release blocker; the README
     works without it.
- **Wave 7 candidates, once the tag is out:** M16 (watches), M17 (REPL pane), then M18 (debugpy) which is the first real test of the adapter seam (D029).
  Smaller items worth folding in: conditional breakpoints from the TUI, and the config schema
  fields the blueprint documents that nothing reads yet.
- New decisions from Phase D's TUI lane: **D040** (the reducer numbers its own requests and drops
  answers that have been overtaken), **D041** (`Cmd::Batch`), **D042** (the TUI reconnects by
  calling back into the CLI rather than learning to spawn), **D043** (a breakpoint change is either
  an adapter's opinion or the project's, and the event says which — **protocol v2 → v3**), **D044**
  (a reconnecting TUI never gives up, and every attempt is identified).
- A review round after M12–M14/M19 found eight defects, all fixed; the per-milestone task files
  carry a "Review round" section describing each. A ninth, found by counting processes rather
  than by reading code, was a **product** bug: a debuggee outlived its debugger whenever the
  adapter died uncleanly (**D045**). Check for orphans with `pgrep -f target/debug/c-fixtures`,
  not only `pgrep -x codelldb` — that blind spot is how 46 of them survived five waves of
  review. The theme was staleness applied to answers but
  not to what stays on screen while one is outstanding, and session-scoped facts treated as
  project-global.
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
chapter tags are cut from `lazydap-learn`, not here. Product releases (v0.1+) have their own
workflow, [.github/workflows/product-release.yml](.github/workflows/product-release.yml), which
runs on `v*` tags and has never fired.

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

- [x] [M12 — Stack pane](docs/implementation/tasks/M12-stack-pane.md) — completed 2026-07-30. `crates/tui/src/panes/stack.rs`. Tab cycles source → stack → scopes, `j`/`k` move the selection in the focused pane, `<CR>` jumps the source pane to the frame *and* fetches that frame's scopes. `Cmd::Batch` (D041) and reducer-allocated request ids (D040) landed here.
- [x] [M13 — Scopes pane with expansion](docs/implementation/tasks/M13-scopes-pane.md) — completed 2026-07-30. `crates/tui/src/panes/scopes.rs`. Lazily-expanded tree; `<CR>` fetches a row's children once and toggles it thereafter. Replies are matched to the node that asked by request id, and a handle already open above a row is refused rather than followed into a cycle.
- [x] [M14 — Toggle breakpoint from TUI](docs/implementation/tasks/M14-toggle-breakpoint.md) — completed 2026-07-30. `b` adds or removes through the same `BreakpointAdd`/`BreakpointRemove` the CLI sends; `●`/`◯`/`⊘` in a gutter column of its own, on the line the adapter actually used. No daemon-side handler was needed and `crates/store` was untouched — both already did the job.
- [x] [M19 — TUI reconnect](docs/implementation/tasks/M19-tui-reconnect.md) — completed 2026-07-30. `lazydap shutdown` from another terminal now leaves the TUI reconnecting rather than frozen: reducer-owned backoff, a daemon started through the CLI's own `ensure_daemon_running` behind an `EnsureDaemon` callback (D042), and a screen made true again by the `Subscribe` snapshot rather than reconstructed.
- [x] [M15 — Config file + launch.json import](docs/implementation/tasks/M15-config-file.md) — completed 2026-07-30, plus a six-finding review round (D049 config-path order, D050 client-resolved adapter → **protocol v4**, cppdbg's `environment`/`stopAtEntry`, shell-string `args`, a broken config no longer bricking `status`/`shutdown`/`doctor`, and comments blanked rather than deleted in the JSONC scanner). `~/.config/lazydap/config.toml` (adapter pin, `--wait` default), `.vscode/launch.json` import with a hand-rolled JSONC scanner (D046) and `${...}` expansion that warns rather than substitutes, `lazydap launches list`/`run` resolved client-side (D047), and quirk 8's real fix — an unbound breakpoint re-sent under the path the adapter names (D048). Release artifacts had been pre-staged by W5b; the `[0.1.0]` CHANGELOG section is now finalised, which is what lets the release workflow publish. **Phase D complete.**

## Beyond v0.1 (M16–M18+)

- [x] [M20 — Documentation website](docs/implementation/tasks/M20-docs-site.md) — completed 2026-07-30. `site/`: Astro + Starlight, 39 pages (22 generated from the binary with a drift-failing CI check), wire examples serialised from the real protocol types, `llms.txt`, verified transcripts throughout. `cd site && npm ci && npm run build`. Deployment + domain still to decide (SITE_URL placeholder is `lazydap.sh`); `public/og.png` is the one missing asset.
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
O01–O04 answered 2026-07-30. One question remains, and it now blocks nothing but the tag: whether
to publish the crates to crates.io (default: no — `publish = false` stays, matching mxr's stance).
Decide it before tagging, because crates.io versions are immutable and names are permanent.
