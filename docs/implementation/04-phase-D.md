# 04 — Phase D: useful features → v0.1

**Goal:** ship something people want to install. After Phase D, `cargo install lazydap` produces a binary worth using.

## Milestones

- **[M12 — Stack pane](tasks/M12-stack-pane.md)** — TUI stack pane. `<CR>` jumps to frame.
- **[M13 — Scopes pane](tasks/M13-scopes-pane.md)** — scopes pane with expand-on-`<CR>`.
- **[M14 — Toggle breakpoint](tasks/M14-toggle-breakpoint.md)** — `b` toggles breakpoint at cursor line. Sign in gutter.
- **[M15 — Config file + launch.json import](tasks/M15-config-file.md)** — config file, launch.json. **Tag v0.1. `cargo install lazydap`.**

## What you'll have at the end

- TUI with stack pane, scopes pane, breakpoint toggling, gutter signs.
- Persistent breakpoints across sessions (saved in `.lazydap/state.toml`).
- `.vscode/launch.json` parsed and surfaced as launch configs.
- Config file at `~/.config/lazydap/config.toml`.
- Tagged v0.1 release on crates.io.
- A README with a GIF demo, install instructions.

## v0.1 release checklist

When M12–M15 land, before tagging:

- [ ] `cargo install --path crates/daemon` works on a fresh machine
- [ ] README updated with v0.1 quick-start
- [ ] `lazydap.skill` ZIP up to date
- [ ] `CHANGELOG.md` populated (release-please will manage going forward)
- [ ] `LICENSE-MIT` and `LICENSE-APACHE` present
- [ ] `CONTRIBUTING.md` exists
- [ ] `SECURITY.md` exists
- [ ] CI green on `main`
- [ ] Demo GIF recorded
- [ ] Tag `v0.1.0` on git, push
- [ ] `cargo publish` for each crate (in dependency order)
- [ ] GitHub release created with binaries (post-v0.1.0 if release pipeline isn't ready)

## Phase-level concepts

### Persistent breakpoints

`.lazydap/state.toml` is loaded on daemon startup; breakpoints are sent to the adapter via `setBreakpoints` when the session launches. After session end, the file is updated to reflect any session-time mutations (e.g., user toggled a bp during debug).

### `launch.json` interop

Per [`/docs/blueprint/08-state-and-config.md`](../blueprint/08-state-and-config.md). VS Code-flavoured JSON-with-comments. We use the `json5` crate (or `serde_json` after stripping comments). Imported configs marked `source: VsCodeLaunchJson`.

### Why v0.1 here, not later

After M15:

- A TUI works.
- Persistent breakpoints work.
- Existing repos with `.vscode/launch.json` work out of the box.
- Agents can drive sessions.

That's a coherent product. Watches and REPL (M16, M17) are real features, but they're improvements on a working tool. Shipping v0.1 here gets feedback flowing.

## Risks specific to Phase D

- **Breakpoint persistence subtlety.** Breakpoint IDs (lazydap's UUIDs) must persist across sessions; adapter IDs (DAP's i64s) must NOT. Easy to confuse.
- **launch.json variables.** `${workspaceFolder}`, `${file}`, `${env:VAR}` — handle them or fail with a clear error. Don't substitute silently with empty.
- **Release ergonomics.** First public release means first "I tried lazydap and it crashed" reports. Build with that in mind: the daemon's panic message should be readable.

## Phase D is done when

- All M12–M15 boxes ticked.
- v0.1.0 tag on git, crates.io published, README has the quick-start.
- A new user can `cargo install lazydap`, run on a CMake project with `.vscode/launch.json`, and have a working debug session.

Then move to Phase E.
