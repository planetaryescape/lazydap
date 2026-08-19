# TODO

Living list of what's next. Detailed per-milestone files in [`docs/implementation/tasks/`](docs/implementation/tasks/).

> **This is the shipping repo — ship mode** (see [`/AGENTS.md`](AGENTS.md)). Teaching happens in
> `lazydap-learn`; the teaching material still in this tree is read-only reference. The work loop:
> pick the first unchecked milestone below, read its task file fully, build it, get
> `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`,
> `cargo check --workspace --all-targets`, and `cargo test --workspace --all-targets` green, tick the
> box here, add a completion note to the task file.

## Now

- **Next milestone: [M24 — Attach to a running process](docs/implementation/tasks/M24-attach.md).**
  The segment's most-needed feature and no competitor has it; chosen over js-debug on
  2026-07-31. [M23 — js-debug](docs/implementation/tasks/M23-jsdebug-adapter.md) is blocked at
  its own scope gate behind it: the parent session debugs nothing, and a single-level
  child-session milestone has to come first.
- **v0.1.0 shipped 2026-07-31; v0.2.0 through v0.2.8 shipped 2026-08-18, v0.2.9 on 2026-08-19.** All eleven went out
  through [.github/workflows/product-release.yml](.github/workflows/product-release.yml) as
  prereleases, with macOS arm64/x86_64 and Linux x86_64 tarballs, SHA-256 sums and
  `lazydap.skill` attached. `install.sh` and the Homebrew tap (M21) are both live; the docs
  site is deployed to <https://lazydap.sh> (M20). crates.io is answered and is **no** — D051,
  `publish = false` stays on all seven crates.
- **The 2026-08-18 defect campaign.** An audit of the whole surface found defects in every part
  of it; they were fixed in parallel packages and released the same day, one release per area:
  startup, the store and project-root detection (v0.2.0); breakpoints (v0.2.1); `--wait` and the
  execution queue (v0.2.2); the CLI surface, `doctor` and the paths errors (v0.2.3); adapter
  lifecycle (v0.2.4); the TUI (v0.2.5); the adapter-id mapping race that Linux exposed once CI
  ran the real adapters (v0.2.6); this docs sweep (v0.2.7); and the follow-ups it left (v0.2.8, D100; v0.2.9, D101–D102). The protocol went **v8 → v9** on
  the way (D086) and **v9 → v10** in the follow-ups (D101), and decisions **D084–D102** record the rules that changed; the CHANGELOG sections say what a
  user sees. The theme: answers that were confidently wrong rather than absent — a `--wait`
  reporting `timeout` for a program that had stopped, a `break --condition` silently dropping
  the condition, `--no-terminate` killing the program and saying it had not, and one leaked
  adapter process per session that ended on its own.
- **Follow-ups the campaign left**, all closed. v0.2.8 took four — the unframeable subscriber
  event, `completions` panicking on `EPIPE`, `Request::Doctor`'s dead daemon-side flags, and
  `install.sh`'s missing `GITHUB_TOKEN` passthrough — and the round after it took the last
  three: `Request::Doctor`'s now-unread fields came off the wire (protocol **v9 → v10**,
  D101), `state.unknown` is three-way merged like everything else in the file (D102),
  and the demo GIF M15's step 4 asked for is recorded from [docs/demo/demo.tape](docs/demo/demo.tape).

### Repo state notes (for cold-start agent)

Repo: [github.com/planetaryescape/lazydap](https://github.com/planetaryescape/lazydap), public.
The `chapter-*` tags/releases and [.github/workflows/release.yml](.github/workflows/release.yml)
are teaching-era machinery: tags mark the *start state* of book chapters. Leave them alone; new
chapter tags are cut from `lazydap-learn`, not here. Product releases (v0.1+) have their own
workflow, [.github/workflows/product-release.yml](.github/workflows/product-release.yml), which
runs on `v*` tags and has published every release from `v0.1.0` on. The full release flow is
the "Release shorthand" section of [`/AGENTS.md`](AGENTS.md).

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
- [x] [M16 — Watches](docs/implementation/tasks/M16-watches.md) — completed 2026-07-31. Watch expressions are project state in `.lazydap/state.toml` beside the breakpoints; their values belong to one stop and are never persisted or carried across one (D056). A watches pane, `a` to add through a prompt and `dd` to remove, `lazydap watch add/list/remove` with `--dry-run` and `--format ids`, and an `Event::WatchUpdated` so a `watch add` in another terminal reaches an open TUI. Every round of evaluation is numbered, because stopping and then selecting a caller frame puts two in flight. **Protocol v4 → v5** — a new `Request` variant is not additive in either direction.
- [x] [M17 — REPL pane](docs/implementation/tasks/M17-repl-pane.md) — completed 2026-07-31. `Tab` to the REPL, type an expression, `<CR>` sends the same `Eval` the CLI's `eval` sends, in `watch` context for D034's reason; `/bt` reaches LLDB's command interpreter instead (D057). History per-session on `<C-p>`/`<C-n>`, answers matched to the line that asked by entry id rather than position. While the cursor is in there `q` is a `q` — and fixing what that exposed, the TUI's own logs now go to the instance log file rather than across the panes.
- [x] [M22 — Third adapter: delve](docs/implementation/tasks/M22-delve-adapter.md) — Go via dlv dap; done 2026-07-31. `outputMode: "remote"` is mandatory or all debuggee output is lost; the D045 reaper needed D061 to recognise a compiled Go debuggee; protocol bumped to v6 (D063).
- [ ] [M24 — Attach to a running process](docs/implementation/tasks/M24-attach.md) — the segment's most-needed feature, no competitor has it; chosen over js-debug 2026-07-31
- [ ] [M23 — Fourth adapter: js-debug](docs/implementation/tasks/M23-jsdebug-adapter.md) — Node; **blocked at its own scope gate**. Spiked 2026-07-31 against js-debug 1.117.0: the parent session debugs nothing — 0 stops, 0 output, breakpoints stay provisional — everything happens in a `startDebugging` child. Needs a single-level child-session milestone first; deferred behind M24 (attach) per the segment thesis.
- [x] [M21 — Packaging: install.sh and Homebrew](docs/implementation/tasks/M21-packaging.md) — added 2026-07-31 after the fresh-install audit; the two channels the ship-it checklist records as missing. done 2026-07-31; tap repository and `HOMEBREW_TAP_TOKEN` both live, initial v0.1.0 formula push happens at merge
- [x] [M18 — Second adapter (debugpy)](docs/implementation/tasks/M18-second-adapter.md) — Python debugged end to end; the adapter seam is a trait (D052)

## Known follow-ups (post-v0.1, no milestone yet)

*(debugpy landed at M18, delve at M22. The campaign's own follow-ups are under "Now".)*

- Multi-session support (currently single-session-per-daemon enforced; protocol uses session IDs from M5 to keep this option open)
- `js-debug` adapter for Node/TS — has a milestone (M23) and is blocked at its scope gate
- Conditional breakpoints in the TUI (the CLI already ships them — `break --condition`, discovered working 2026-07-30; only the TUI can't set one)
- Restart / disconnect-and-relaunch
- Theming + mouse support
- HTTP bridge (separate crate, optional binary)
- AI advisor extension points (see [`docs/blueprint/12-ai-future.md`](docs/blueprint/12-ai-future.md))
- Auto-context in the `--wait` blob: an inline source snippet with a current-line marker. The locals half of this shipped at v0.2.0 — the blob carries `locals` and `user_frame` (D078)
- Double-continue semantics review: queueing a second `continue` silently carries past a breakpoint; a clean rejection may be safer against agent double-fire (bake-off finding; touches D021)
- Install-hint error prose — *done for `lazydap doctor` at D093, which names the install command per adapter. Deliberately not on the launch path: somebody whose launch just failed is not being told how to install a debugger they have.*
- Daemon-restart race test: stop then immediate relaunch raced to a transport EOF in the rival CLI; assert lazydap's path
- Cargo "locator": resolve `cargo build` artifacts into codelldb launch configs without a launch.json (Zed-style)

## Open decisions awaiting input

Tracked in [`docs/blueprint/15-decision-log.md`](docs/blueprint/15-decision-log.md) under "Open" status.
**None.** O01–O04 were answered 2026-07-30 and became D024–D027; the crates.io question was
answered 2026-07-31 and is D051 — no, `publish = false` stays on all seven crates and the
release workflow has no publish job.
