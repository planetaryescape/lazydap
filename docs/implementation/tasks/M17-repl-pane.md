# M17 — REPL pane

## What

Bottom-split REPL pane. Type expression, `<CR>` submits via `Eval`, response appended to history. `<C-p>`/`<C-n>` navigate previous/next.

## Why

Sometimes you want to type ad-hoc expressions without committing them as watches. REPL is for that. Plus it's the natural UX for raw adapter commands (codelldb's `expressions: "native"` mode).

## How

### Step 1 — Pane

```rust
pub struct ReplView {
    pub history: Vec<ReplEntry>,             // input + output
    pub input: String,
    pub history_cursor: Option<usize>,       // for <C-p>/<C-n>
    pub scroll_offset: u16,
}

pub struct ReplEntry {
    pub input: String,
    pub output: ReplOutput,
}

pub enum ReplOutput {
    Pending,
    Value(String, Option<String>),           // value, type
    Error(String),
}
```

### Step 2 — Reducer

```rust
Msg::Key(KeyEvent { code: KeyCode::Enter, .. }) if state.focused_pane == Pane::Repl => {
    let input = std::mem::take(&mut state.repl_view.input);
    if input.is_empty() { return (state, Cmd::None); }
    state.repl_view.history.push(ReplEntry { input: input.clone(), output: ReplOutput::Pending });
    let session_id = state.session.as_ref().unwrap().session_id;
    let frame_id = state.stack_view.frames.first().map(|f| f.id);
    return (state, Cmd::SendIpc(Request::Eval {
        session_id,
        expression: input,
        frame_id,
        context: EvalContext::Repl,
    }));
}
Msg::DaemonResponse { response: Response::EvalResult { value, type_name, .. }, .. } => {
    if let Some(last) = state.repl_view.history.iter_mut().rev().find(|e| matches!(e.output, ReplOutput::Pending)) {
        last.output = ReplOutput::Value(value, type_name);
    }
    (state, Cmd::None)
}
// <C-p> previous: cursor history backward, replace input
// <C-n> next: cursor history forward
// Char append: state.repl_view.input.push(c)
// Backspace: state.repl_view.input.pop()
```

### Step 3 — Render

Two areas inside the pane: history (scrollable), input prompt.

```
┌── repl ──────────────────────────────────────┐
│ > p tokens                                   │
│   <Vec<Token>> 12 elements                   │
│ > p tokens[3]                                │
│   <Token> { kind: Identifier, lexeme: "x" }  │
│ > █                                          │
└──────────────────────────────────────────────┘
```

## Success criteria

- Type expression, submit, see value or error.
- `<C-p>`/`<C-n>` navigate input history.
- Pending submissions show "..." until response arrives.
- History scrollable when it grows beyond pane height.
- Tab focus integration: Tab cycles through Source / Stack / Scopes / Watches / Repl.

## Files

- `crates/tui/src/panes/repl.rs` (new)
- `crates/tui/src/state.rs` — add `repl_view`
- `crates/tui/src/update.rs` — extend

## Verify

Pause at `int y = x * 2`. Tab to REPL. Type `x + 1`. `<CR>`. See `6`.

## Depends on

- [`M16-watches`](M16-watches.md).

## Notes

- **REPL history per-session by default.** Persist optionally via config (post-v0.1).
- **Input doesn't validate.** Send raw to adapter; let the adapter return errors.
- **`expressions: "native"` mode in codelldb** lets users write `p (int)x` style. Document this in `references/examples.md`.
