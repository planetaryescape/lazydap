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


## Completed 2026-07-30

`crates/tui/src/panes/stack.rs`. Tab cycles source → stack → scopes (BackTab the other
way), `j`/`k` move the selection in whichever pane has the keys, `<CR>` jumps the source
pane to the selected frame *and* fetches that frame's scopes — one intention, so both.

Verified in a pseudo-terminal against real codelldb, on a program with a stack worth
navigating:

```
┌source · …/nested.c────────────────────────────────────────────┐┌stack────────────────────────────┐
│●▶  5     int doubled = a * 2;      /* line 5 — the breakpoint ││nested.c:5 inner                 │
│    6     return doubled;                                      ││nested.c:11 middle               │
│    7 }                                                        ││nested.c:16 main                 │
│    8                                                          ││<no source> start                │
paused at nested.c:5 · F5 continue · F10 step · b break · Tab pane · q quit
```

`Tab`, `j`, `<CR>` — the marker moves to the caller and the status row follows:

```
│ ▶ 11     return inner(bumped);     /* line 11 */              │
paused at nested.c:11 · F5 continue · F10 step · b break · Tab pane · q quit
```

`lazydap stack --format json` reports the same four frames (non-negotiable 2).

### Deviations from the plan

- **`levels: Some(64)`, not 20.** Twenty truncates real C recursion. Sixty-four is enough
  for a stack a person reads and few enough that a runaway recursion is cut off rather
  than paged in; the adapter's `total` still says how deep it really is.
- **A stop asks for the stack *and* the scopes at once** (`Cmd::Batch`, D041) rather than
  fetching scopes after the stack lands. The daemon queues to one adapter either way
  (non-negotiable 6), so waiting would have added a round trip to every step.
- **The staleness discipline M11 introduced for file reads now covers stack fetches**
  (D040). A trace for the stop before last is not merely out of date — every frame id in
  it addresses nothing, so it is dropped rather than applied.
- `Cmd::SendIpc` gained an `id` the reducer allocates (D040), which M13 needs and which
  makes the above decidable.

### Noticed, not changed

- **The marker follows the selected frame.** Jumping to `main`'s frame puts `▶` on line 16
  and the status row says `paused at nested.c:16`, while the program is really stopped at
  line 5. That is what the task file asked for and what most debuggers do, but VS Code
  draws a non-top frame's marker differently (hollow). Worth a distinct sign later.
- **Frame ranking for AI clients** remains a future feature, as the task file says.

### Review round, 2026-07-30

Two defects, both the same shape: D040's staleness discipline was applied to *answers* but
not to what is on screen while one is outstanding.

- **A frame from the previous stop could be jumped to.** Between a `stopped` and the trace
  answering it, the pane still lists the previous stop's frames, whose ids the adapter has
  discarded. `<CR>` in that gap sent a dead `frame_id` *and* the scopes request that went
  with it superseded the legitimate one the new stop had just made — so the scopes pane
  ended up empty rather than merely late. Both panes are now marked stale the moment a stop
  is reported and live again only when that stop's answer lands. They keep drawing in the
  meantime: clearing them would make both blink empty on every single step.
