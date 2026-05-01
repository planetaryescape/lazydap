# M12 — Stack pane

## What

Right-side panel showing the call stack. `<CR>` jumps the source view to the selected frame's source/line.

## Why

After M11, you can pause and see the current line. Stack pane lets you navigate up the call chain — essential for non-trivial debugging.

## How

### Step 1 — Pane type

`crates/tui/src/panes/stack.rs`:

```rust
pub struct StackView {
    pub frames: Vec<StackFrame>,
    pub selected: usize,
}

impl StackView {
    pub fn move_selection(&mut self, delta: i32) { ... }
    pub fn render(&self, frame: &mut Frame, area: Rect, focused: bool) {
        // Render each frame as one row: "main.c:42 main".
        // Selected row highlighted. Focused border colored.
    }
}
```

### Step 2 — Layout

In `view.rs`, split horizontally: source 70%, stack 30%.

```rust
let columns = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
    .split(layout[0]);
```

### Step 3 — Reducer extensions

```rust
Msg::DaemonEvent(Event::Stopped { .. }) => {
    // ... existing handler ...
    // Fetch full stack now (M11 fetched levels: 1).
    return (state, Cmd::SendIpc(Request::StackTrace { levels: Some(20), ... }));
}
Msg::DaemonResponse { response: Response::StackTrace { frames, .. }, .. } => {
    state.stack_view.frames = frames;
    state.stack_view.selected = 0;
    // ... update current_line from top frame
    (state, Cmd::None)
}
Msg::Key(KeyEvent { code: KeyCode::Tab, .. }) => {
    state.focused_pane = match state.focused_pane {
        Pane::Source => Pane::Stack,
        Pane::Stack => Pane::Source,
        _ => Pane::Source,
    };
    (state, Cmd::None)
}
Msg::Key(KeyEvent { code: KeyCode::Char('j') | KeyCode::Down, .. })
    if state.focused_pane == Pane::Stack =>
{
    state.stack_view.move_selection(1);
    (state, Cmd::None)
}
Msg::Key(KeyEvent { code: KeyCode::Enter, .. }) if state.focused_pane == Pane::Stack => {
    if let Some(frame) = state.stack_view.frames.get(state.stack_view.selected) {
        if let Some(path) = frame.source.as_ref().and_then(|s| s.path.clone()) {
            state.current_line = Some((path, frame.line));
        }
    }
    (state, Cmd::None)
}
```

## Success criteria

- Stack pane renders with one row per frame.
- Tab cycles focus between source and stack.
- `j`/`k` in stack pane moves selection.
- `<CR>` on stack frame jumps source pane to that file/line.
- After step events, stack is refreshed.

## Files

- `crates/tui/src/panes/stack.rs` (new)
- `crates/tui/src/state.rs` — add `stack_view`
- `crates/tui/src/update.rs` — extend
- `crates/tui/src/view.rs` — layout

## Verify

Manual: pause at a deep call, press Tab to focus stack, `j` to navigate, `<CR>` to jump.

## Depends on

- [`M11-wire-ipc-into-tui`](M11-wire-ipc-into-tui.md).

## Notes

- **Frame ranking heuristics for AI clients** are a future feature. Stack pane is just the raw frames in M12.
- **Frame source paths can be `None`** for synthetic frames. Display "<no source>".
