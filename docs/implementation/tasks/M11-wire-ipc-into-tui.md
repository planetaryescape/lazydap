# M11 — Wire IPC into TUI

## What

The TUI becomes a real client of the daemon. It connects via Unix socket, subscribes to events, sends requests for keypresses (continue, step, etc.), shows current line based on `Stopped` events.

By the end, you can: start `lazydap` (TUI), set a breakpoint, press F5, see the program pause at the breakpoint with the line highlighted.

## Why

This is lazydap v0.1-prerelease. The TUI was scaffolding until now; M11 makes it real.

## How

### Step 1 — IPC client in TUI crate

`crates/tui/src/ipc_client.rs`:

```rust
use lazydap_protocol::{IpcMessage, Request, Response, Event};
use tokio::sync::mpsc;

pub struct IpcClient {
    tx: mpsc::UnboundedSender<Request>,
    next_id: AtomicU64,
}

pub fn connect(socket_path: &Path) -> anyhow::Result<(IpcClient, mpsc::UnboundedReceiver<Msg>)> {
    let stream = tokio::net::UnixStream::connect(socket_path).await?;
    // Spawn read pump → Msg::DaemonResponse / Msg::DaemonEvent
    // Return write handle as IpcClient
    todo!()
}

impl IpcClient {
    pub fn send(&self, req: Request) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        // ... send to write task
        id
    }
}
```

### Step 2 — Extend `Msg` and `Cmd`

`crates/tui/src/msg.rs`:

```rust
pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
    LoadSourceCompleted(PathBuf, std::io::Result<String>),
    // New:
    DaemonResponse { id: u64, response: Response },
    DaemonEvent(Event),
    DaemonError(IpcError),
    DaemonDisconnected,
}

pub enum Cmd {
    Quit,
    LoadSource(PathBuf),
    SendIpc(Request),
    None,
}
```

### Step 3 — Connection on startup

In `run()`:

```rust
ensure_daemon_running().await?;
let (ipc, mut ipc_rx) = ipc_client::connect(&socket_path()?).await?;
ipc.send(Request::Subscribe { channels: vec![EventKind::Stopped, EventKind::Output, EventKind::Continued, EventKind::SessionEnded] });

loop {
    terminal.draw(...)?;
    let msg = tokio::select! {
        Some(m) = input_rx.recv() => m,
        Some(m) = ipc_rx.recv() => m,
        _ = tick.tick() => Msg::Tick,
    };
    let (new_state, cmd) = update::update(state, msg);
    state = new_state;
    match cmd {
        Cmd::Quit => break,
        Cmd::LoadSource(p) => spawn_load_source(input_tx.clone(), p),
        Cmd::SendIpc(req) => { ipc.send(req); }
        Cmd::None => {}
    }
}
```

### Step 4 — Reducer extensions

In `update.rs`, new branches:

```rust
Msg::Key(KeyEvent { code: KeyCode::F(5), .. }) => {
    if let Some(session) = &state.session {
        let req = Request::Continue {
            session_id: session.session_id,
            thread_id: session.current_thread,
            wait: WaitMode::NoWait,
            all_threads: false,
        };
        return (state, Cmd::SendIpc(req));
    }
    (state, Cmd::None)
}
Msg::Key(KeyEvent { code: KeyCode::F(10), .. }) => {
    // step over — same shape, Request::Step
    ...
}
// F11, S-F11, b, q, etc.

Msg::DaemonEvent(Event::Stopped { thread_id, reason, .. }) => {
    if let Some(s) = state.session.as_mut() {
        s.state = SessionState::Paused { reason: reason.clone(), thread_id };
        s.current_thread = Some(thread_id);
    }
    // Trigger refetch of stack to know current line.
    let session_id = state.session.as_ref().unwrap().session_id;
    let cmd = Cmd::SendIpc(Request::StackTrace {
        session_id, thread_id, start_frame: Some(0), levels: Some(1),
    });
    (state, cmd)
}
Msg::DaemonResponse { id: _, response: Response::StackTrace { frames, .. } } => {
    if let Some(top) = frames.first() {
        if let Some(path) = top.source.as_ref().and_then(|s| s.path.clone()) {
            // Load this file if not already loaded.
            // Highlight line top.line.
            state.current_line = Some((path, top.line));
        }
    }
    (state, Cmd::None)
}
```

### Step 5 — Source pane shows current line

Add `current_line: Option<(PathBuf, u32)>` to `AppState`. In `SourceView::render`, mark the current line with a colored arrow `→` in the gutter.

### Step 6 — Run

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello

# Pre-set a breakpoint via CLI, then enter TUI:
lazydap break examples/c-hello/main.c:6
lazydap launch ./examples/c-hello/build/hello --stop-on-entry
lazydap            # TUI

# In TUI: press F5 to continue. Should hit the breakpoint at line 6, current-line marker appears.
```

## Success criteria

- TUI connects to daemon on launch.
- Subscribed to events; receives `Stopped`, `Output`, etc.
- F5 / `c` → continue → program runs → pauses at breakpoint → current line marker shows.
- F10 / `n` → step over → next line marker.
- Disconnect via `q` → daemon session continues; bare `lazydap` again reconnects.
- Status bar shows session state ("Paused at main.c:6" / "Running" / "No session").

## Files

- `crates/tui/src/ipc_client.rs` (new)
- `crates/tui/src/state.rs` — add session, current_line fields
- `crates/tui/src/update.rs` — extend with stepping and event handlers
- `crates/tui/src/panes/source.rs` — add current-line marker

## Verify

End-to-end manual:

1. Build hello binary.
2. `lazydap break examples/c-hello/main.c:6 --format json` — bp set.
3. `lazydap launch ./hello --stop-on-entry` — session paused on entry.
4. `lazydap` (bare, in TTY) — TUI opens, source shown, current line at main.
5. F5 → program runs → pauses at line 6 → `→` marker appears at line 6.
6. F10 → marker moves to next line.
7. q → TUI exits cleanly, daemon and session still running.

## Depends on

- [`M10-elm-ify`](M10-elm-ify.md) — reducer pattern in place.
- [`M07-skill-agent-verification`](M07-skill-agent-verification.md) — daemon and protocol working.

## Notes

- **TUI talks ONLY to the daemon.** Doesn't spawn its own adapter. Doesn't open the socket directly without using the IPC client.
- **Subscribe to a small set of events.** Don't subscribe to everything — adds noise.
- **Don't fire `--wait` from the TUI.** TUI uses fire-and-forget continue; it learns about the new state from the `Stopped` event. (Per [`/docs/blueprint/10-async-to-sync.md`](../../blueprint/10-async-to-sync.md): TUI uses streaming events, agents use `--wait`.)
- **Reconnection.** If the daemon dies while TUI is running, show an error banner and offer to restart. Optional for M11; mandatory before v0.1.
- **After M11, lazydap v0.1-prerelease is real.** The rest of Phase D is polish.

## Completed 2026-07-30

The TUI is a client of the daemon. `Subscribe` works server-side, the source pane's marker follows the debuggee, and F5/`c`, F10/`n`, F11 and shift-F11 send the requests behind `continue`, `step`, `step-in` and `step-out`.

### `Subscribe`, and what it answers with

It had been a placeholder answering `Unsupported` since M5. Implementing it is a change to `server::serve_client` rather than to `handlers::dispatch`: the connection is the thing an event stream attaches to, and the dispatcher does not have one. A subscribed connection selects over its socket *and* the session broadcast; events go out as ordinary event frames (id `0`) interleaved with replies, filtered to the kinds asked for. Sends happen in the body of a `select!` arm and never as one — `IpcConnection::send` is not cancellation-safe and a cancelled send leaves half a frame on the wire.

The reply is a `Response::Status`, and **nothing buffered is replayed**. Both are load-bearing and both are argued in **D038**; the short version is that a snapshot taken under the same call has no window for an event to fall into, that a replayed `Stopped` would send the TUI chasing a position the program left, and that the buffer's delivery watermark belongs to `--wait`. Protocol stays at **v2**: no new `Response` variant, no reshaped frame, and the frozen `Shutdown` escape hatch untouched.

### Corrections to the sketch above

- **`connect` is `async`.** The sketch declares it non-async with `.await` inside, and ends in a `todo!()` — which the workspace lints warn on. There is no `todo!()` anywhere.
- **Responses are not correlated back to requests.** A `Response` says what it is, and the daemon answers one request at a time in order, so the last answer of a given kind is the freshest. The id is carried for logging.
- **`state.session.unwrap()` in the sketch's `Stopped` branch is not there.** The branch that sets the session and the one that reads it are the same branch; it reads what it just wrote.
- **The marker is remembered, not passed along.** The daemon can say "line 19 of main.c" while main.c is still being read off disk, so a stop records a `location` and the marker is applied either immediately (file open) or when the load lands. A file that finishes loading *after* the program has moved on is not marked — both cases are tested.

### Boundaries

`crates/tui` depends on `core`, `protocol` and `config` only; the row is in `scripts/check_architecture_boundaries.sh` (D037). Nothing in it names a DAP type. The daemon is ensured by `commands::tui::run`, through the same `ensure_daemon_running` every subcommand uses, and the TUI is handed a socket path — starting a *process* is the binary's job, not a client's.

### Verification

Reducer and IPC client are covered headless (56 tests in `lazydap-tui`, five more in `crates/daemon/tests/ipc_server.rs` for the subscription). The cross-process scenario needs a real terminal and a real codelldb, so it was driven in a pseudo-terminal — see **D039**. Both directions were checked: a `lazydap continue` typed in another shell moves the marker, and so does F5 pressed inside the TUI.

```
┌source · …/examples/c-hello/main.c─────────────────────────────────────┐
│  19     printf("goodbye y=%d\n", y); /* line 19 — M4 breakpoint */     │
│▶ 20     return 0;                                                      │
│  21 }                                                                  │
└────────────────────────────────────────────────────────────────────────┘
paused (step) at main.c:20 · F5 continue · F10 step · q quit
```

Also verified: `q` leaves the session running (the daemon still reports it paused afterwards), and a TUI started against an already-paused session finds the marker from the subscription snapshot rather than waiting for the program to move.

### Follow-ups discovered

- **No reconnection.** A daemon that goes away is reported in the status row and the TUI stays up with a dead connection. The notes call this optional for M11 and mandatory before v0.1; it needs a milestone.
- **`EndReason` → `SessionState` is now written twice** — once in `crates/daemon/src/state.rs` (`end_once`) and once in `crates/tui/src/update.rs`. Its home is `lazydap-core`, next to `EndReason`, but that is a change to a crate outside this milestone's diff.
- **Output events are not subscribed to.** There is no pane for them until M17. `Request::Output` already reads the buffer, so nothing is lost.
