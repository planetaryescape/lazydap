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
