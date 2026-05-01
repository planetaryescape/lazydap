# 02 — Phase B: daemon + protocol

**Goal:** wrap the DAP plumbing in a daemon, expose it through a JSON-over-Unix-socket protocol, build CLI subcommands, ship an agent skill.

By the end, you have a working CLI debugger. No TUI yet. Agents can use it. Real, shippable.

## Milestones

- **[M5 — IPC protocol + daemon binary](tasks/M05-ipc-protocol-daemon.md)** — define lazydap protocol types in `crates/protocol`. Daemon binary holds session, accepts IPC client. Single subcommand `lazydap launch` works end-to-end.
- **[M6 — CLI subcommands](tasks/M06-cli-subcommands.md)** — full surface: `break`, `continue`, `step`, `step-into`, `step-out`, `pause`, `stack`, `scopes`, `eval`, `status`, `disconnect`. `--wait` semantics implemented.
- **[M7 — Skill + agent verification](tasks/M07-skill-agent-verification.md)** — build `lazydap.skill` ZIP. Test end-to-end with Claude Code: agent reads skill, drives a debug session.

## What you'll have at the end

- `crates/protocol` with all IPC types
- `crates/dap` cleaned up (Phase A was exploratory; B tidies it)
- `crates/store` for `.lazydap/state.toml` reads
- `crates/config` for `~/.config/lazydap/config.toml` and `.vscode/launch.json` import
- `crates/adapter-codelldb` separated out as its own crate
- `crates/daemon` containing the IPC server and CLI dispatcher
- A `lazydap.skill` ZIP at repo root
- Working CLI: `lazydap launch ./bin && lazydap break main.c:42 && lazydap continue --wait`

## Phase-level concepts

### Auto-spawning daemon

Per [`/docs/blueprint/01-architecture.md`](../blueprint/01-architecture.md): first subcommand needing the daemon checks for an existing socket; if missing, forks a daemon child. M5 implements this primitive; M6 uses it for every subcommand.

### `--wait` is the bridging primitive

Per [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md). Stepping/continue commands have `--wait` mode that blocks until next stable state, returning all intervening events. M6 implements; testing in M7.

### State persistence

`.lazydap/state.toml` for breakpoints, watches, launch configs. M5 reads it on session startup; later milestones add the per-state mutations.

### The agent skill is just docs

`lazydap.skill` is a ZIP of `SKILL.md` + `references/commands.md`. No runtime logic. The agent invokes shell commands. M7 builds and tests the ZIP.

## Risks specific to Phase B

- **Crate boundary discipline.** Tempting to put everything in `daemon`. Don't. (Per [`/ARCHITECTURE.md`](../../ARCHITECTURE.md).)
- **`--wait` edge cases.** Multi-thread, adapter death, timeout. Per [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md).
- **Daemon crash recovery.** Clients must auto-respawn. M5 handles the basics; tighten in M6.
- **TOML state on first run.** No `.lazydap/state.toml` → empty state, run fine. Don't crash.

## Phase B is done when

- Cargo workspace structure matches [`/ARCHITECTURE.md`](../../ARCHITECTURE.md) crate layout.
- `cargo install --path crates/daemon` produces a working `lazydap` binary.
- A Claude Code agent can: read `lazydap.skill`, run `lazydap launch ./bin --stop-on-entry --format json`, set a bp, continue with `--wait`, eval an expression, disconnect.
- All commands return JSON when piped or `--format json`, table when interactive.
- All tests pass against real codelldb.

Then move to Phase C.
