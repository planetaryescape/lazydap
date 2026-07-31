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

---

## Completed 2026-07-31

Watches are project state; their values are not. `Watch` (id, expression, optional label)
lives in `.lazydap/state.toml` beside the breakpoints, with its own `next_watch_id` counter
that never reuses an id. `WatchValue` is never written anywhere — it belongs to one stop,
and a file claiming `pos = 4` tomorrow would be a lie (D056).

**Files.** `crates/core/src/watch.rs` (new: `WatchId`, `Watch`, `NewWatch`, `WatchValue`,
`WatchSelector`), `crates/store/src/{lib,file}.rs` (typed `[[watches]]`, CRUD, adoption of
hand-edited entries), `crates/protocol/src/types.rs` (three requests, `WatchReport`,
`Event::WatchUpdated`, **version 5**), `crates/daemon/src/handlers/watches.rs` (new),
`crates/daemon/src/commands/watch.rs` (new), `crates/tui/src/panes/watches.rs` (new),
`crates/tui/src/panes/input.rs` (new, shared with M17).

### Deviations from the plan above

- **`Cmd::Batch` already existed.** The open question in step 2's comment was answered by
  D041 during M12; watches use it as that entry predicted, and `one_or_batch` keeps the
  "a batch of one is never constructed" rule true now that frame selection returns a
  variable number of commands.
- **Evaluation is sent with `frame_id: None`, not `frames.first()`.** The sketch reads the
  frame id off the stack on screen, which at the moment of a stop belongs to the stop the
  program has *just left* — every id in it addresses nothing. `None` means "the top frame"
  and the daemon resolves it by fetching it fresh, so it costs no extra round trip and
  cannot be stale.
- **Watches follow the selected frame.** Not in the plan. Leaving them on the top frame
  would put the callee's numbers beside the caller's locals in two panes an inch apart,
  with nothing saying they are about different functions. This is what makes the round
  generation load-bearing: two batches for the same expressions can be in flight at once.
- **`WatchReport` has no `applied_to_session`.** Unlike a breakpoint, nothing installs a
  watch, so there is nothing for a session to have been told.
- **`Event::WatchUpdated` was added.** Not in the plan, and the reason is D043: without it a
  `lazydap watch add` in another terminal leaves an open TUI drawing the previous list
  indefinitely — exactly the bug D043 found in the breakpoint gutter, avoided here by
  announcing from the start.
- **The CLI is `watch add/list/remove`**, real subcommands rather than `break`'s flags.
  `break` is flag-driven because its add case carries a location and four modifiers; a
  watch is an expression and nothing else.
- **Protocol v4 → v5.** Not anticipated by the plan. See D056: a new `Request` variant makes
  an older daemon fail to deserialise the whole envelope, so it never reaches the version
  check that exists to turn this into a clean restart.

### Follow-ups discovered

- `lazydap watch list` returns expressions only. An agent that wants the current values
  makes one `lazydap eval` per watch. A `--values` flag that evaluates when a session is
  paused would be a genuine ergonomic win for the agent loop; it is deliberately not in
  v0.1 because it makes the one command that reads project state depend on a session.
- Two store tests used `[[watches]]` as their canonical unmodelled-section fixture and now
  use `[[data_breakpoints]]`. The next typed field will need the same treatment.
- A watch whose `variables_reference` is non-zero (a struct, an array) is shown as a single
  line. Expanding one the way the scopes pane expands a variable is the obvious next step
  and needs no protocol change.
