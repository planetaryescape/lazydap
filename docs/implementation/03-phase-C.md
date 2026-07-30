# 03 — Phase C: TUI

**Goal:** add the TUI as a second client of the same protocol. Keep it strictly a client.

By the end, lazydap has a TUI that shows source, stack, scopes, and lets you drive a debug session with vim-flavoured keys. Same protocol as the CLI.

## Milestones

- **[M8 — Hello ratatui](tasks/M08-hello-ratatui.md)** — minimal ratatui app. No DAP. Just learn the render loop.
- **[M9 — Show a file](tasks/M09-show-a-file.md)** — render a hardcoded source file in ratatui. Still no DAP.
- **[M10 — Elm-ify the loop](tasks/M10-elm-ify.md)** — refactor M9 into Model/Msg/update/view. Don't skip this.
- **[M11 — Wire IPC into TUI](tasks/M11-wire-ipc-into-tui.md)** — TUI subscribes to daemon events over IPC. Keypresses send IPC requests. Source pane shows current line.

## What you'll have at the end

- `crates/tui` with the ratatui shell
- TUI binary launchable as `lazydap tui` or bare `lazydap` (auto if interactive)
- Source pane with current-line marker
- Basic keymap: F5 continue, F10 step, F11 step-in, S-F11 step-out, b breakpoint toggle, q quit
- TUI as a peer client of the daemon — uses same IPC the CLI uses

## Phase-level concepts

### Hand-rolled Elm Architecture

Per [`/docs/blueprint/15-decision-log.md`](../blueprint/15-decision-log.md) D012. M10 is the dedicated milestone for this — write the `(State, Msg) -> (State, Cmd)` reducer by hand. Every other milestone in Phase C+ depends on this shape being right.

### TUI is a client, not a peer

The TUI doesn't get special features. It uses the same `Subscribe { channels }` and `Request` API the agent skill uses. If the TUI needs information the protocol doesn't expose, the protocol is wrong, not the TUI.

### Async I/O integration

The TUI runs `tokio::main`. The render loop uses `tokio::select!` over: input events (crossterm), IPC events (mpsc from daemon connection), tick (60Hz refresh). Per [`/docs/blueprint/07-tui.md`](../blueprint/07-tui.md).

## Risks specific to Phase C

- **Skipping M10.** Tempting after M9 ("it works without Elm shape, why bother?"). Skip and the project collapses. M11+ is built on M10.
- **Mutating state in `view()`.** Pure render. No mutation. No I/O. Anti-pattern.
- **Caching too much in the TUI.** The daemon is the source of truth. The TUI shows what the daemon told it.
- **Diverging from CLI vocabulary.** Use protocol types directly in TUI state. Don't define a parallel UI vocabulary that drifts.

## Phase C is done when

- `lazydap tui` launches a working ratatui app.
- A debug session runs: F5 continues, F10 steps, breakpoints get added/removed via `b`, source pane shows current line.
- Every TUI action has an equivalent CLI invocation. (Test by running both side-by-side: same outcome.)
- M10's reducer pattern is in place; new features in Phase D extend the match without restructuring.

Then move to Phase D — useful features → v0.1.

## Completed 2026-07-30

All four boxes ticked. `crates/tui` is a ratatui client of the daemon: it subscribes to events, shows the source with a marker that follows the debuggee, and drives the program with F5/`c`, F10/`n`, F11 and shift-F11 — each sending the request behind the matching subcommand, so the side-by-side check the criteria ask for is the same code path rather than two that happen to agree.

Two things this phase settled beyond the milestones:

- **The dependency arrow between `daemon` and `tui` points the other way from what `ARCHITECTURE.md` said** (D037). The daemon crate is also the `lazydap` binary, so it is what starts the TUI. The direction that protects the boundary — `tui` → `daemon` — is unchanged and now has a row in the CI check.
- **`Subscribe` is answered with a state snapshot and replays nothing** (D038). That is what lets a TUI started against an already-running session find out where the program is without waiting for it to move.

One thing the "done when" list does not yet cover: **`b` (toggle breakpoint) is M14**, not M11, so the keymap above is complete apart from that. The other gap is reconnection — a daemon that dies while the TUI runs is reported in the status row, and picking the connection back up needs its own milestone before v0.1.
