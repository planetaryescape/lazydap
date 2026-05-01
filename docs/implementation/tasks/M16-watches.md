# M16 — Watches

## What

Watches pane below scopes. `a` adds an expression. Re-evaluates each time the program pauses. Displays current value, type, error if any. Persisted in `.lazydap/state.toml`.

## Why

Stepping through code where you're tracking 2-3 specific values manually with `eval` gets old fast. Watches automate it.

## How

### Step 1 — Pane

`crates/tui/src/panes/watches.rs`:

```rust
pub struct WatchesView {
    pub watches: Vec<Watch>,
    pub values: HashMap<WatchId, Result<WatchValue, String>>,
    pub selected: usize,
}
```

### Step 2 — Reducer

```rust
Msg::Key(KeyEvent { code: KeyCode::Char('a'), .. })
    if state.focused_pane == Pane::Watches =>
{
    state.modal = Some(Modal::AddWatch(String::new()));
    (state, Cmd::None)
}
Msg::Modal(ModalAction::SubmitAddWatch(expr)) => {
    state.modal = None;
    (state, Cmd::SendIpc(Request::WatchAdd { expression: expr, label: None }))
}
Msg::DaemonEvent(Event::Stopped { .. }) => {
    // Existing handler ...
    // Plus: re-evaluate all watches.
    let watches = state.watches_view.watches.clone();
    let session_id = state.session.as_ref().unwrap().session_id;
    let frame_id = state.stack_view.frames.first().map(|f| f.id);
    // Send batched evals.
    let cmds = watches.iter().map(|w| Cmd::SendIpc(Request::Eval {
        session_id,
        expression: w.expression.clone(),
        frame_id,
        context: EvalContext::Watch,
    })).collect::<Vec<_>>();
    // (Multiple Cmds → ergonomics question. Probably a Cmd::Batch(Vec<Cmd>) variant.)
    (state, Cmd::Batch(cmds))
}
```

### Step 3 — Persistence

`Watch` persists in `.lazydap/state.toml`. `WatchValue` is per-pause, never persisted.

## Success criteria

- `a` opens an "add watch" modal.
- Submitted expression appears in watches pane.
- Each `Stopped` event triggers re-evaluation; values update.
- Errors (variable out of scope) shown inline, dimmed.
- `dd` deletes selected watch.
- Watches persist across sessions.

## Files

- `crates/tui/src/panes/watches.rs` (new)
- `crates/tui/src/state.rs` — add `watches_view`, `Modal::AddWatch`
- `crates/tui/src/update.rs` — extend
- `crates/store/src/lib.rs` — add `Watch` CRUD

## Verify

Set a watch on `tokens[pos]`, step through a parser, watch values update.

## Depends on

- [`M13-scopes-pane`](M13-scopes-pane.md), [`M15-config-file`](M15-config-file.md).

## Notes

- **`Cmd::Batch(Vec<Cmd>)`** is the cleanest way to send multiple IPC requests from one update. Add to `Cmd` enum.
- **Modal handling is new in M16.** First modal in the TUI; design carefully so M17's REPL prompt and confirm dialogs follow the pattern.
- **Errored watches stay in the list.** Don't auto-remove. The expression might be in scope at a different frame.
