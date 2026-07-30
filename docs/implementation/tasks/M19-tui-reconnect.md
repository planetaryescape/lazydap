# M19 — TUI reconnects when the daemon goes away

## What

When the daemon dies or restarts under a running TUI, the TUI currently reports it in the
status row and goes inert (recorded at M11). Make it recover: detect the death, retry the
connection with backoff, re-run the subscribe-and-snapshot handshake on success, and
reconcile the displayed state with reality.

## Why

The blueprint's TUI notes call reconnection mandatory before v0.1: a debugger UI that has
to be restarted because its backend restarted is not shippable. The daemon can legitimately
go away mid-session (protocol-version upgrade, `lazydap shutdown` from another shell, a
crash) and the TUI is the one client that lives long enough to notice.

## How

1. `ipc_client`: on connection loss emit `Msg::DaemonGone` (exists), then begin retrying —
   `ensure_daemon_running` from a spawned task, backoff 250ms → 4s capped, forever until
   quit. Each attempt result is a `Msg` so the reducer owns all state.
2. On reconnect: fresh `Subscribe` (snapshot semantics per D038 mean the snapshot is the
   reconciliation — no replay to reason about). Reducer applies the snapshot exactly as at
   startup: session present → adopt it; absent → clear session state, keep the file view.
3. Status row states: `daemon gone — reconnecting…` while retrying, normal after. Keys that
   need the daemon are inert-with-notice while disconnected (reducer guard, not key removal).
4. The auto-spawn path means "retry" may *start* a daemon. That is correct and free — it is
   the same call every CLI command makes.

## Success criteria

- `lazydap shutdown` from another shell while the TUI is open: status row shows
  reconnecting, then a daemon is auto-spawned and the TUI returns to "no session" idle.
- Kill -9 the daemon mid-paused-session: TUI reconnects; the snapshot has no session
  (adapter died with the daemon), so the TUI shows session-ended/idle, not stale "paused".
- A reducer test drives the full sequence with synthetic Msgs (Gone → attempts → reconnected
  + snapshot) and asserts every intermediate render state.
- No busy-loop: backoff verified by the retry task's timing (unit-testable via the delay
  sequence, not wall-clock sleeps in tests).

## Files

- `crates/tui/src/ipc_client.rs` — retry loop, backoff
- `crates/tui/src/update.rs`, `state.rs`, `msg.rs` — reconnecting states, snapshot reconcile
- `crates/tui/src/view.rs` — status-row rendering for the reconnect states

## Verify

```bash
cargo test -p lazydap-tui
# manual/PTY: open TUI, `lazydap shutdown` elsewhere, watch status row cycle
```

## Depends on

- [M11-wire-ipc-into-tui](M11-wire-ipc-into-tui.md) — subscription + snapshot handshake exists.

## Notes

- Created 2026-07-30 during ship-mode Wave 4 review: M11 recorded reconnection as a
  mandatory pre-v0.1 follow-up; this file gives it a milestone per AGENTS.md workflow #6.
- Scheduled into Phase D alongside M12–M14 (same crate, same lane) so it lands before
  M15 tags v0.1.
