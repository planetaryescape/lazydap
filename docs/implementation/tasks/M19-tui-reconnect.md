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

`Msg::DaemonGone` forgets the session and returns `Cmd::Reconnect { attempt, delay_ms }`.
The backoff lives in the reducer so it is testable without waiting for it: 250 ms,
doubling to a 4 s ceiling, then 4 s for as long as the TUI is open.

**It never gives up.** Every attempt runs `ensure_daemon_running`, which *starts* a daemon
rather than waiting for one, so "cannot reach it" is never a settled fact — the machine it
would run on is the one the TUI is already on. The ceiling is what makes retrying forever
affordable.

Each attempt is numbered, and both the reducer and the loop check that number before
acting on anything an attempt produced: an answer from a superseded attempt is ignored, a
second `DaemonGone` while one is in flight does not start a second ladder, and a
connection handed back by an attempt that lost the race is dropped rather than installed.

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
- It keeps trying for as long as the TUI is open, at no worse than four seconds apart.
- Keys that need the daemon are inert while it is away, with a notice — `b` especially,
  since it flips the gutter before the answer comes back.

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
  reply would go down the dead connection. It installs it only if that attempt is still
  the one being waited on.
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

### Review round, 2026-07-30

**The give-up was wrong and has been removed.** Six attempts take under ten seconds; a
daemon that became startable fifteen seconds later was never reached, on a screen the user
was still sitting in front of. The mistake was modelling this as a network reconnect — it
is not one, because every attempt *starts* a daemon rather than waiting for one. The ladder
now runs for as long as the TUI is open, at no worse than four seconds apart, and
`Connection::Lost` is gone. See D044.

**Attempts are numbered.** Without an identity on each, three things went wrong: a reply
from a superseded attempt was taken for the current one and started a second ladder; a
`DaemonGone` arriving while an attempt was in flight — which a daemon dying just after a
handshake produces — started one outright; and a connection handed back by whichever
attempt lost the race replaced a working connection with an unsubscribed one, after which
every request went somewhere nobody was listening. `Cmd::Reconnect` and `Msg::Reconnected`
both carry the attempt; the reducer and the loop each check it before acting.

Verified live: pressing `b` inside the `reconnecting… (attempt 1)` window left the gutter
untouched and `lazydap break --list` unchanged, and the TUI still brought a v3 daemon back
on its own.

### Orphaned debuggees, and a correction to what "no strays" meant

Every completion note above claimed "zero strays" on the strength of
`pgrep -x lazydap` and `pgrep -x codelldb`. Both were true and both were beside the point:
the thing that was leaking was neither. codelldb spawns the **debuggee** as its own child,
and a SIGKILLed codelldb never reaps it — so the user's program was reparented to init and
kept running, invisible to a check that only ever looked for the debugger.

46 of them had accumulated across worktrees before anybody counted. Fixed at the product
layer (D045): the daemon records the pid of a program it launched and kills it if the
adapter dies without stopping it, after checking the pid still names that program.

**The check to run from now on** is on the debuggee, not only the tooling:

```bash
pgrep -x lazydap
pgrep -x codelldb
pgrep -f "$PWD/target/debug/c-fixtures"   # the one that was missing
```

Evidence after the fix: four consecutive full runs of `cargo test --test wait_codelldb`
(13 tests each), cumulative orphan count `0` after every one. Before it, that suite leaked
exactly one per run.

### Review round, 2026-08-18 (defect campaign)

**A handshake was taken as proof the daemon works, and it is not.** `daemon_gone` always
started the ladder at attempt 1, so a daemon that accepted the connection and then died on the
first request it was given — a crash handling `Subscribe`, an out-of-memory kill, something
restarting it in a loop — reset the backoff on every cycle: `ensure_daemon()` every 250ms, for
as long as the TUI was open. The ladder now keeps its rung across a connection that did not
last and is only reset once one has been up for five seconds, counted in the loop's ticks
because the reducer has no clock. It still never gives up (**D-WP6-1**). A side effect worth
having: attempt numbers climb across a whole unproven streak instead of restarting at 1 on
every handshake, so the superseded-attempt check cannot mistake one ladder's attempt 1 for the
previous one's. (A connection that proves itself does reset the count — deliberately — so they
are not monotonic for the life of the TUI.)

**A panic left bracketed paste on.** `enable_bracketed_paste()` runs after `ratatui::try_init`,
and ratatui's panic hook only calls `restore()` — raw mode and the alternate screen. A crash
therefore left the user's shell wrapping every later paste in `\x1b[200~ … \x1b[201~` until
they ran `reset`. The hook ratatui installed is now wrapped by one that turns paste mode off
first and then calls it.
