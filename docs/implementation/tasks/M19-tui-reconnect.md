# M19 — TUI reconnects after the daemon goes away

## What

When the daemon the TUI is talking to disappears — `lazydap shutdown` from another
terminal, a crash, a version-mismatch replacement — the TUI starts a new one, reconnects,
re-subscribes, and reconciles the screen against whatever it finds. The status row says
which of those it is doing.

## Why

Phase C left `Msg::DaemonGone` as a dead end: it printed "the daemon went away" and the
TUI sat there with a screen that had quietly stopped being true. Every key that talks to
the daemon did nothing, and the only way out was `q` and a restart.

That is not an exotic case. `lazydap shutdown` is the documented way to clear a daemon
from an older build, and the natural time to run it is while a TUI is open. A debugger
that has to be restarted whenever its daemon is upgraded is a debugger people stop
leaving open.

## How

### Step 1 — a connection state, and a status row that reads it

`crates/tui/src/state.rs`:

```rust
pub(crate) enum Connection {
    Connected,
    Reconnecting { attempt: u32 },
    Lost,
}
```

The status row checks it *before* anything else, including a notice. Every other thing
the row could say is about a daemon it cannot reach and would read as current.

### Step 2 — a reducer-owned retry curve

`Msg::DaemonGone` forgets the session and returns `Cmd::Reconnect { delay_ms }`. The
backoff lives in the reducer so it is testable without waiting for it: 250 ms, doubling
to a 4 s ceiling, six attempts, then `Connection::Lost`.

Finite on purpose. A TUI retrying for ever behind a status row nobody is reading looks
alive while showing a screen that stopped being true.

### Step 3 — the loop runs it in a task

`Cmd::Reconnect` sleeps, calls the `EnsureDaemon` callback, connects, and hands the new
client back to the loop. Never inline: four seconds of a blocked loop is four seconds in
which `q` does not work.

### Step 4 — the callback, because the boundary holds

`lazydap-tui` may not depend on the daemon crate (D037), and starting a daemon means
starting a *process*. So `run` takes:

```rust
pub type EnsureDaemon = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>> + Send + Sync,
>;
```

and `crates/daemon/src/commands/tui.rs` supplies one that calls the same
`ensure_daemon_running` every subcommand takes (D003) — spawn lock and all, so a TUI and
a CLI command racing to revive a daemon do not end up with two.

### Step 5 — reconcile, do not reconstruct

A reconnection replays the opening moves of the first connection (`Msg::Connected` →
`Subscribe` + `BreakpointList`). The `Subscribe` reply is a state snapshot taken at the
moment the stream attaches (D038), so a session started from another terminal while the
daemon was down is picked up rather than waited for.

## Success criteria

- `lazydap shutdown` from another terminal leaves the TUI showing `reconnecting…`, not a
  frozen screen.
- The TUI starts a daemon and reconnects on its own.
- A session that exists after the reconnection is adopted, with its marker, stack and
  scopes back.
- Breakpoints survive: they are the project's, not the session's.
- `q` works throughout, including while a backoff is being waited out.
- Six failed attempts end in `daemon lost` rather than an endless retry.

## Files

- `crates/tui/src/state.rs` — `Connection`
- `crates/tui/src/msg.rs` — `Msg::Connected`, `Msg::Reconnected`, `Cmd::Reconnect`
- `crates/tui/src/update.rs` — the backoff and the reconcile
- `crates/tui/src/lib.rs` — `EnsureDaemon`, the reconnect task, installing the new client
- `crates/tui/src/view.rs` — the status row
- `crates/daemon/src/commands/tui.rs` — supplying the callback

## Verify

```bash
# In one terminal: a TUI on a paused session.
lazydap break examples/c-hello/main.c:19
lazydap launch examples/c-hello/build/hello
lazydap tui

# In another: take the daemon away.
lazydap shutdown

# The TUI says `reconnecting… (attempt 1)`, then comes back on its own.
lazydap status      # a new daemon, started by the TUI
```

## Depends on

- [`M11-wire-ipc-into-tui`](M11-wire-ipc-into-tui.md).

## Notes

- **Nothing new is needed daemon-side.** Auto-spawn is the client's own path, and
  `Subscribe` already answers with a snapshot.
- **A `Msg` cannot carry the new client.** Messages are `Clone` and a connection is not,
  so the reconnecting task hands it back over a channel of its own and the loop installs
  it *before* the reducer hears about it — otherwise the requests the reducer asks for in
  reply would go down the dead connection.
- **A daemon from another build** is handled for free: `ensure_daemon_running` already
  shuts it down and starts a current one.


## Completed 2026-07-30

Built as specified. `Connection::{Connected, Reconnecting { attempt }, Lost}`, a
reducer-owned backoff (250 ms doubling to a 4 s ceiling, six attempts), a `Cmd::Reconnect`
the loop waits out in a task, and an `EnsureDaemon` callback supplied by
`crates/daemon/src/commands/tui.rs` so the daemon is started by exactly the path a CLI
command would use (D042). Nothing was added daemon-side.

Verified in a pseudo-terminal against real codelldb. `lazydap shutdown` run from another
shell while the TUI was on a paused session:

```
│●   5     int doubled = a * 2;      /* line 5 — the breakpoint ││no stack                         │
…
│▸ scopes                                                       ││no scopes                        │
reconnecting… (attempt 1) · F5 continue · F10 step · b break · Tab pane · q quit
```

Five seconds later, without a keystroke:

```
│◯   5     int doubled = a * 2;      /* line 5 — the breakpoint ││no stack                         │
no session · F5 continue · F10 step · b break · Tab pane · q quit
```

```console
$ lazydap status --format json
{ "daemon_pid": 67923, "instance": "agent-a7ed363971-…", "protocol_version": 2,
  "session": null, "uptime_ms": 4809 }
```

A daemon four seconds old — the TUI started it. The session is gone because `shutdown`
ended it, and the breakpoint correctly drops from `●` back to `◯`: it survived (it is the
project's, in `.lazydap/state.toml`) but nothing has applied it to a session yet.

### Deviations from the plan

- **The task file did not exist.** TODO had no M19 entry and
  `docs/implementation/tasks/M19-tui-reconnect.md` was absent; the orchestrator's brief
  said only the phase-doc line was missing. The file was written from the brief's spec
  before building, then the phase-doc line and the TODO entry were added.
- **`Msg::Connected`.** Initialisation became a reducer decision rather than something the
  loop hard-codes, so a reconnection replays the *same* opening moves (`Subscribe` +
  `BreakpointList`) instead of a second copy of them.

### Noticed, not changed

- **`EndReason` → `SessionState` is still duplicated** between `crates/daemon/src/state.rs`
  and `crates/tui/src/update.rs` (noted at M11). The reconcile path did not touch it —
  reconciliation goes through the `Subscribe` snapshot, which carries a `SessionState`
  already — so moving it to `lazydap-core` stayed out of scope.
- **The reconnect is not throttled across TUIs.** Several open TUIs would each try to
  revive the daemon; the spawn lock in `ensure_daemon_running` means they cannot start
  more than one, so the cost is a few wasted connects.
- **A version-mismatch replacement is handled for free** but untested here: it is
  `ensure_daemon_running`'s existing path.
