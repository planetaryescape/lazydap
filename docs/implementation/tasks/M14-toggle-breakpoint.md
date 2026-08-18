# M14 — Toggle breakpoint from TUI

## What

Press `b` on a source line to toggle a breakpoint. Sign appears/disappears in gutter (`●` verified, `◯` unverified). Breakpoint persists in `.lazydap/state.toml`.

## Why

Setting breakpoints from the TUI is the missing piece for "this is actually a debugger." Until now, breakpoints required CLI invocations. M14 closes the loop.

## How

### Step 1 — Reducer

```rust
Msg::Key(KeyEvent { code: KeyCode::Char('b'), .. })
    if state.focused_pane == Pane::Source =>
{
    let Some(sv) = &state.source_view else { return (state, Cmd::None); };
    let path = sv.path.clone();
    let line = sv.cursor_line;
    let session_id = state.session.as_ref().map(|s| s.session_id);
    return (state, Cmd::SendIpc(Request::BreakpointToggle {
        source: path,
        line,
        session_id,                      // None if no live session — adds to state.toml only
    }));
}
Msg::DaemonResponse { response: Response::Breakpoint(bp), .. } => {
    // Update local view of breakpoints.
    state.breakpoints_view.upsert(bp);
    (state, Cmd::None)
}
Msg::DaemonEvent(Event::BreakpointUpdated(adapter_bp)) => {
    state.breakpoints_view.update_adapter_status(adapter_bp);
    (state, Cmd::None)
}
```

### Step 2 — Source pane gutter rendering

Modify `SourceView::render` to take a list of breakpoints in the file. For each rendered line, prefix with a sign:

```
●  6  printf("hello\n");
◯  8  printf("y=%d\n", y);
   9  return 0;
```

Symbols:

- `●` (verified, enabled) — colored red
- `◯` (unverified, enabled) — colored yellow (adapter hasn't confirmed)
- `⊘` (disabled) — dimmed

### Step 3 — Daemon-side toggle handler

`crates/daemon/src/handlers/breakpoint.rs`:

```rust
async fn handle_breakpoint_toggle(state: Arc<DaemonState>, source: PathBuf, line: u32, session_id: Option<SessionId>) -> Result<SourceBreakpoint> {
    let mut store = state.store.write().await;
    let existing = store.find_breakpoint(&source, line);
    let bp = if let Some(bp) = existing {
        store.remove_breakpoint(bp.id);
        SourceBreakpoint::removed(bp)
    } else {
        let bp = SourceBreakpoint::new(source.clone(), line);
        store.add_breakpoint(bp.clone());
        bp
    };
    store.persist().await?;     // debounced write to .lazydap/state.toml

    // If session is active, sync with adapter.
    if let Some(session_id) = session_id {
        let session = state.sessions.read().await.get(&session_id).cloned();
        if let Some(session) = session {
            // Send setBreakpoints with the current full list for that file.
            session.sync_breakpoints_for_file(&source).await?;
        }
    }
    Ok(bp)
}
```

### Step 4 — Persistence on session start

When a new `Launch` happens, after `initialized` event, the daemon sends `setBreakpoints` for every source file that has breakpoints in `state.toml`. Already partial work from M5; flesh out for full persistence.

## Success criteria

- `b` on a source line toggles a breakpoint.
- Verified breakpoints show `●`, unverified `◯`, disabled `⊘`.
- Breakpoint persists in `.lazydap/state.toml` immediately (within debounce window).
- Closing TUI, restarting, breakpoints still in place when `lazydap launch` runs.
- `lazydap break --list --format json` shows the same set as TUI.

## Files

- `crates/tui/src/update.rs` — extend
- `crates/tui/src/panes/source.rs` — gutter rendering
- `crates/daemon/src/handlers/breakpoint.rs` — toggle handler
- `crates/store/src/lib.rs` — add `find_breakpoint`, `upsert`, `remove`

## Verify

```bash
# Set bp in TUI:
# - Open lazydap (TUI)
# - Move cursor to line 6 of main.c
# - Press 'b'
# - See ● appear

# Verify in CLI:
lazydap break --list --format json | jq '.breakpoints[] | select(.line == 6)'

# Verify persistence:
# Quit TUI. Restart. Same bp still there.
```

## Depends on

- [`M11-wire-ipc-into-tui`](M11-wire-ipc-into-tui.md).

## Notes

- **Adapter `setBreakpoints` replaces all breakpoints in a source file.** When toggling one, send the full list.
- **Verification is async.** When you toggle a bp, the response is a `SourceBreakpoint` (stored locally) AND an `AdapterBreakpoint` event (later). Show unverified initially, update to verified when event arrives.
- **`B` (capital)** for conditional breakpoint with prompt — defer to post-v0.1 unless it's trivial.


## Completed 2026-07-30

`b` on the cursor line adds a breakpoint or takes away the one already there, through the
same `BreakpointAdd`/`BreakpointRemove` requests `lazydap break` and `lazydap break
--remove` send. Gutter signs in a column of their own: `●` verified, `◯` unverified,
`⊘` disabled, drawn on the line the *adapter* used rather than the one that was typed.

Verified in a pseudo-terminal against real codelldb. `gg`, 14×`j`, `b` — a second
breakpoint appears on line 15 beside the original on 19:

```
│   14     int x = 5;                                           ││▾ Local                          │
│●  15     printf("hello from m3\n");                           ││    x = 5 : int                  │
│   16     fflush(stdout); /* stdout is a pipe under the adapter││    y = 10 : int                 │
│   17                        event arrives before the breakpoin││▸ Static                         │
│   18     int y = x * 2;                                       ││▸ Global                         │
│●▶ 19     printf("goodbye y=%d\n", y); /* line 19 — M4 breakpoi││▸ Registers                      │
```

`lazydap break --list --format json` agrees, immediately:

```json
"breakpoints": [
  { "enabled": true, "id": 1, "line": 19, "verified": true, "message": "Resolved locations: 1" },
  { "enabled": true, "id": 2, "line": 15, "verified": true, "message": "Resolved locations: 1" }
]
```

Pressing `b` again on line 15 removes it, and the list drops back to `[(1, 19)]`.

### Deviations from the plan

- **No daemon-side handler was added.** The task file sketched a
  `handle_breakpoint_toggle(source, line, session_id)`. It is not needed and would have
  been a second way to do what `BreakpointAdd` and `BreakpointRemove` already do — both of
  which already persist, already `setBreakpoints` the whole file, and are already what the
  CLI sends. The reducer decides which of the two `b` means from its own view of the list.
  `crates/store` needed nothing either: `select`, `add` and `remove` were already there.
- **`b` is add-or-remove, not the daemon's `BreakpointToggle`,** which flips enabled and
  disabled. Both are useful and they are different things; `b` is the one a gutter does.
- **The gutter updates optimistically**, which is the only way `b` pressed twice quickly
  toggles rather than piling up two adds — the second press has to see what the first
  asked for. It is bounded: the answer overwrites it (matched by *place*, since the
  optimistic entry has no id yet), a refusal re-reads the whole list, and an added
  breakpoint shows as `◯`, which is exactly what it is until the adapter says otherwise.
- **`BreakpointUpdated` is now subscribed to**, and is applied regardless of which session
  it came from: a breakpoint is the project's, and refusing an update because the session
  ids had rolled would leave the gutter saying `◯` for one the adapter had confirmed.

### Noticed, not changed

- **`B` for a conditional breakpoint** is still deferred, as the task file allows.
- **Persistence across a TUI restart** is the store's existing behaviour (D006) and was
  not re-verified here beyond `break --list` agreeing; M6 covers it.

### Review round, 2026-07-30

Three defects, all about the gutter telling the truth.

- **`b` acted on the persisted line, the gutter drew the adapter's.** They differ exactly
  when codelldb moves a breakpoint to the next line with code. Pressing `b` on the visible
  sign at moved-line 12 added a *second* breakpoint; pressing it on now-blank line 10 took
  away the sign at 12. `b` now finds a breakpoint by the line the gutter draws it on and
  builds the removal selector from the line the store knows it by.
- **Verification was treated as project-global.** `verified` and `adapter_line` are one
  adapter's opinion, true only while its session lives. There was no session filter on
  `BreakpointUpdated` and no reset when a session ended, so `●` stayed on a program that
  had exited and a queued update from a dead session could overwrite the live one's.
  Filtered by the session being followed, and reset on `SessionEnded` and `DaemonGone`.
- **`b` flipped the gutter while the daemon was unreachable**, and `IpcClient::send`
  dropped the request — a sign on screen and `.lazydap/state.toml` disagreeing for the rest
  of the run, with nothing to put them back. Every key that needs the daemon is now inert
  with a notice while it is away.

Daemon-side, the reverse direction was broken too: a `lazydap break` with no live session
persisted the breakpoint and announced nothing, so an open TUI's gutter stayed stale
indefinitely. A removal was invisible even *with* a session, because an adapter is handed
the new list for a file and says nothing about what is no longer in it. Every mutation now
announces itself as a project-scope `BreakpointUpdated` (D043, protocol v3) and the TUI
answers by re-reading the list — correct for adds, removals and toggles without the event
having to express any of them.

Verified live: `lazydap break examples/c-hello/main.c:6` typed in another shell made `◯ 6`
appear in a running TUI with no keypress, and `--remove` took it away again.

### Review round, 2026-08-18 (defect campaign)

**The gutter and `b` compared source paths literally; the CLI canonicalises.** `lazydap break`
resolves a path before recording it and the store dedupes on the exact `(source, line)`, so on
macOS — `/tmp` is `/private/tmp`, and a checkout under a symlinked directory is ordinary — a
breakpoint set from the shell was stored under a spelling the pane never matched: no sign on
the line the program was stopped on, and `b` there recorded a duplicate that
`lazydap break --remove /tmp/…` could not select. The source pane now holds the file under the
name the filesystem gives it (resolved by the read that was already happening, off the
reducer), remembers the name it was asked for so the next stop is still "already open", and
every breakpoint match and request is built from the canonical one (**D-WP6-2**).
