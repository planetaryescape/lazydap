# TODO

Living list of what's next. Detailed per-milestone files in [`docs/implementation/tasks/`](docs/implementation/tasks/).

## Current teaching session

> **Project is in teaching mode** (see [`/AGENTS.md`](AGENTS.md) for the protocol). Sessions are smaller than milestones — each session covers one new concept.

**Next session: `M2-2` — DAP transport + atomic seq.** Generic methods, `AtomicI64` for sequence numbers, `thiserror` error type, real codelldb wire-up.

- Book chapter: [`docs/book/07-dap-transport-and-seq.md`](docs/book/07-dap-transport-and-seq.md) (stub — to be filled in-session)
- Plan: [`docs/teaching/sessions.md`](docs/teaching/sessions.md) — search for `M2-2`
- Underlying milestone: [`docs/implementation/tasks/M02-initialize-handshake.md`](docs/implementation/tasks/M02-initialize-handshake.md) (M2-1 covered the type definitions; M2-2 adds the transport struct + generic request method)
- Last session: `M2-1` — Serde and typed protocols (2026-05-03). Obsidian: `Lazydap Session 2026-05-03 M2-1.md`. Atomic concept: `Rust Serde.md`. Public chapter: [`docs/book/06-serde-typed-protocols.md`](docs/book/06-serde-typed-protocols.md).
- Obsidian hub: `Lazydap Teaching Sessions.md` (vault root) — log goes here

**M2-1 deliverable** (shipped): `cargo test -p lazydap-dap` passes three tests round-tripping `Capabilities`, `DapResponse<Capabilities>`, and `InitializeArgs` against real DAP wire shapes. New crate `crates/dap` with `types.rs` housing the typed structs. Call-site diff demonstrated: chapter 05's `value["body"]["foo"].as_bool().unwrap_or(false)` becomes chapter 06's `resp.body.unwrap().foo: bool` — compile-time-checked field access replacing runtime hash-map walks.

**Pre-session todo for M2-2**: none. `crates/dap` exists with type definitions; M2-2 adds `crates/dap/src/transport.rs` and the generic `request<T, R>` method. `thiserror` and `tracing` will need to be added to workspace deps (mechanical).

### Repo state notes (for cold-start agent)

The lazydap repo is now on GitHub at [github.com/planetaryescape/lazydap](https://github.com/planetaryescape/lazydap), publicly available. Four chapter releases live: [chapter-04](https://github.com/planetaryescape/lazydap/releases/tag/chapter-04), [chapter-05](https://github.com/planetaryescape/lazydap/releases/tag/chapter-05), [chapter-06](https://github.com/planetaryescape/lazydap/releases/tag/chapter-06), [chapter-07](https://github.com/planetaryescape/lazydap/releases/tag/chapter-07). Each represents the *start state* of that chapter (rule 18 of the teaching skill). Workflow at [.github/workflows/release.yml](.github/workflows/release.yml) auto-creates a release on every `chapter-*` tag push.

### Per-session ship checklist

After M2-1 (or any future session) finishes its artifact and teach-back, before the session is "done":

- [ ] Smoke test written (rule 17) and passing locally with `cargo test --workspace --all-targets`
- [ ] Public book chapter (`docs/book/NN-*.md`) filled per `references/chapter-template.md`
- [ ] Teaching notes companion (`docs/teaching/notes/NN-*.md`) filled per `references/teaching-notes-template.md`
- [ ] Private session note (`Lazydap Session YYYY-MM-DD.md`) in Obsidian + hub updated
- [ ] Atomic concept notes created/extended for keepers
- [ ] **Two-commit dance for chapter tag** (rule 18):
    1. Commit lesson content (chapter file + notes + supporting docs), push main
    2. Tag `chapter-(NN+1)` at that commit, `git push origin chapter-(NN+1)` — workflow auto-creates release
    3. Verify: `gh release view chapter-(NN+1)`
    4. Commit artifact code (example file + smoke test), push main
- [ ] CONTRIBUTING.md and AGENTS.md still match reality after changes

**Note for cold-start me**: The chapter-04 release was manually patched once because its tag's commit predated the release workflow's main-fallback fix. If chapter-04 tag is force-updated later, the workflow will overwrite the manual notes — that's fine since the workflow now generates the right notes from the chapter file on main.

If the user says "drop teaching mode," skip the teaching column and pick milestones directly from the lists below.

## Workspace setup (prerequisite to M0)

- [x] [Workspace setup](docs/implementation/00-workspace-setup.md) — Cargo workspace, daemon binary stub, CI, conventions
  - Completed 2026-05-01 across 3 teaching sessions (`WS-1`, `WS-2`, `WS-3`). Initial commit: `6a06e68`.

## Now

- Decisions to confirm with user (see `docs/blueprint/15-decision-log.md` for in-flight items)
- Continue teaching session `M2-1` (next)
- Fill in the book chapter stubs for chapters 06-39 as the corresponding sessions land

## Phase A — see the protocol (M0–M4)

- [x] [M0 — Hello, adapter](docs/implementation/tasks/M00-hello-adapter.md) — completed 2026-05-02 (session `M0-1`). Public chapter: [`docs/book/04-hello-adapter.md`](docs/book/04-hello-adapter.md). Two follow-up issues filed: [docs/issues/0001](docs/issues/0001-codelldb-symlink-install-broken.md), [docs/issues/0002](docs/issues/0002-codelldb-version-drift-rust-log.md). New reference: [docs/reference/codelldb-quirks.md](docs/reference/codelldb-quirks.md).
- [x] [M1 — Read one message](docs/implementation/tasks/M01-read-one-message.md) — completed 2026-05-03 (session `M1-1`). Public chapter: [`docs/book/05-read-one-message.md`](docs/book/05-read-one-message.md). Side win: `verify-before-publishing` framework propagated to teaching/bookgen skills + global CLAUDE.md after live version-drift hang surfaced the principle.
- [ ] [M2 — Initialize handshake](docs/implementation/tasks/M02-initialize-handshake.md) — session `M2-1` completed 2026-05-03 (typed structs in new `crates/dap`). Public chapter: [`docs/book/06-serde-typed-protocols.md`](docs/book/06-serde-typed-protocols.md). Session `M2-2` next (transport struct + generic request method).
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
