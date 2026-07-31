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

---

## Completed 2026-07-31

`Tab` reaches the REPL, typing goes into a prompt, `<CR>` sends the same `Request::Eval`
that `lazydap eval` sends, and the answer is appended under the line that asked for it.

**Files.** `crates/tui/src/panes/repl.rs` (new), `crates/tui/src/panes/input.rs` (new,
shared with M16's prompt), `crates/tui/src/{state,update,view}.rs`, and
`crates/daemon/src/commands/tui.rs` for the logging fix below.

### Deviations from the plan above

- **The context is `watch`, not `repl`** (D057). The sketch sends `EvalContext::Repl`, which
  D034 had already established means "run this through LLDB's command interpreter" — so
  `x + 1` fails on a program where `x` exists. Adapter commands are still reachable, behind
  a `/` prefix: `/bt` is LLDB's `bt`. The task file's own "natural UX for raw adapter
  commands" is preserved without making it the trap the first expression falls into.
- **Answers are matched by entry id, not by "the last pending one".** The sketch fills in
  the newest `Pending` entry it can find. Two submissions in flight answered out of order
  would then put the first line's value under the second. Ids also survive the scrollback
  being trimmed from the front, which positions do not.
- **`frame_id` is the frame the stack pane has selected**, falling back to `None` (the top
  frame, resolved daemon-side) when the stack on screen belongs to a stop the program has
  left. The sketch always takes the top frame, which would disagree with the scopes pane
  the moment somebody selects a caller.
- **`Esc` leaves the pane when the line is empty.** Not in the plan, and necessary: `q` is a
  character in here, so without it somebody who tabbed in has no way out that does not
  require already knowing about `Tab`.
- **`repl_claims` is a separate predicate** from the handler, so "what the REPL swallows" is
  one readable list. Function keys are deliberately not claimed — none of them can be part
  of an expression, so F5 still continues while a half-typed line sits on the prompt.
- **`ReplOutput::Value` carries an `EvalResult`** rather than `(String, Option<String>)`,
  so the pane and `lazydap eval` cannot drift apart about what an answer is.

### The bug this milestone exposed

Driving the pane in a real pseudo-terminal showed log lines drawn *across the panes*. Every
non-daemon command logs to stderr, which is right for one that prints and exits and wrong
for the one that enters the alternate screen. It was reachable before — a source file that
would not open warns — but M17 makes it routine: a mistyped expression is a refused
request, and an out-of-scope watch is refused on every single step. The TUI now installs
its own subscriber pointed at the instance log file, which `lazydap logs` already reads, so
the lines are kept rather than dropped.

### Follow-ups discovered

- History is per-session, as the plan's note says. Persisting it behind a config key is
  post-v0.1 and deliberately unbuilt.
- The scrollback keeps 200 entries and has no scroll keys of its own: it is anchored to the
  bottom so the prompt is always visible, and older entries scroll off. `<C-u>`/`<C-d>`
  still reach the source pane while the REPL has focus, which is a reasonable place to put
  REPL scrolling later.
- `codelldb` answers a C expression with a note that it ran it as ISO C++. Harmless, and
  worth a line in `docs/reference/codelldb-quirks.md` if it ever confuses anybody.
- `expressions: "native"` mode (the plan's third note) is still undocumented in
  `references/examples.md`.
