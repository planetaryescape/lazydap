# M13 — Scopes pane with expansion

## What

Pane below stack showing scopes (Locals, Arguments, Globals) for the currently-selected frame. `<CR>` on a variable expands its children inline.

## Why

The thing you actually use during debugging. After M13, you can inspect state at the current pause without leaving the TUI.

## How

### Step 1 — Pane type with expand-tracking

`crates/tui/src/panes/scopes.rs`:

```rust
pub struct ScopesView {
    pub scopes: Vec<ScopeNode>,
    pub selected: ScopePath,            // path through the tree to the selected row
}

pub struct ScopeNode {
    pub scope: Scope,
    pub variables: Vec<VariableNode>,   // populated on first expansion
    pub expanded: bool,
    pub loaded: bool,
}

pub struct VariableNode {
    pub variable: Variable,
    pub children: Vec<VariableNode>,
    pub expanded: bool,
    pub loaded: bool,                   // false until Variables request returns
}

pub type ScopePath = Vec<usize>;        // index path into the tree
```

### Step 2 — Lazy load on expand

When user presses `<CR>` on an unloaded row, send `Request::Variables { variables_reference }` and mark `loaded: false; pending: true`. When response arrives, populate children.

```rust
Msg::Key(KeyEvent { code: KeyCode::Enter, .. }) if state.focused_pane == Pane::Scopes => {
    let node = state.scopes_view.selected_node();
    if !node.loaded && node.variables_reference != 0 {
        return (state, Cmd::SendIpc(Request::Variables { ... }));
    }
    node.expanded = !node.expanded;
    (state, Cmd::None)
}
Msg::DaemonResponse { response: Response::Variables(vars), .. } => {
    // Match the request_id back to the requesting node, populate children.
    state.scopes_view.populate_pending(vars);
    (state, Cmd::None)
}
```

### Step 3 — Render with indent

Each row prefixed with indent + expand marker (`▸` collapsed, `▾` expanded, ` ` leaf).

```
▾ Locals
  x = 5 : int
  y = 10 : int
  ▸ buf : char[256]
▸ Arguments
▸ Globals
```

## Success criteria

- Scopes pane renders Locals/Arguments/Globals.
- `<CR>` on a scope expands; renders nested variables.
- Lazy load: only fetch children when expanded.
- Re-fetched on each `Stopped` event (state may have changed).

## Files

- `crates/tui/src/panes/scopes.rs` (new)
- `crates/tui/src/state.rs` — add `scopes_view`
- `crates/tui/src/update.rs` — extend

## Verify

Pause at `int y = x * 2;` in main.c. Tab to scopes pane. `<CR>` on Locals. See `x = 5`. Step over. `<CR>` on Locals again. See `x = 5, y = 10`.

## Depends on

- [`M12-stack-pane`](M12-stack-pane.md).

## Notes

- **Lazy loading is the entire point.** A 100,000-element array shouldn't fetch all children at once.
- **Variables tree can be cyclic** in pathological cases (mutually-referencing pointers). Track visited variables_references to avoid infinite expansion.
- **Errors during eval show inline** ("(error: variable not in scope)").
- **Pending requests need correlation.** When you send `Variables { variables_reference: 42 }`, remember which node asked for it. Use the request `id` to correlate when the response arrives.


## Completed 2026-07-30

`crates/tui/src/panes/scopes.rs`. Lazily-expanded tree: `<CR>` on a closed, unfetched row
sends `Request::Variables` and marks it `⋯`; the answer fills it in and opens it.
Re-pressing closes and reopens without asking again. Scopes are re-fetched on every stop
and for whichever frame M12's `<CR>` selected.

Verified in a pseudo-terminal against real codelldb, at the M4 breakpoint:

```
┌scopes───────────────────────────┐
│▾ Local                          │
│    x = 5 : int                  │
│    y = 10 : int                 │
│▸ Static                         │
│▸ Global                         │
│▸ Registers                      │
```

And on the nested program, after `Tab`, `j`, `<CR>` to select `main`'s frame — its own
locals, not the top frame's, which is the whole point of depending on M12:

```
│▾ Local                          │
│    seed = 5 : int               │
│    result = 1 : int             │
```

`lazydap scopes` and `lazydap variables --reference N` are the CLI equivalents.

### Deviations from the plan

- **A flat `selected: usize` over the visible rows**, not the sketched `ScopePath`
  selection. `j`/`k` walk what is on screen, which is what the user sees; the index-path
  form (`NodePath`) is still how a *node* is addressed, which is what a reply needs.
- **One `Node` type for scopes and variables.** A scope is a named handle with children
  and no value; a struct is the same thing with a value. Two types would have meant
  writing the expansion logic twice.
- **Correlation is by request id** (D040), as the task file's last note asked for.
  `Response::Variables` is a bare list — nothing in it says which node asked, not even the
  reference — so `pending_variables` maps id → node path, and a new frame's `Scopes`
  request clears it.
- **Cycles are refused rather than depth-capped.** Expanding a handle already open above
  the row is declined with a notice. A doubly linked list is the ordinary case, and a
  depth cap would either cut off legitimate nesting or still cost a fetch per step.

### Noticed, not changed

- **Expansion state is not carried across stops.** Every stop rebuilds the tree collapsed.
  Remembering which paths were open and re-expanding them is a real feature and a
  different one; half-doing it would show last stop's values under this stop's labels.
- **Paging (`start`/`count`) is not wired.** `VariableFilter::All` with no window fetches
  a whole container. The lazy *expansion* is what bounds the cost today; a 100,000-element
  array opened deliberately would still arrive in one go.
- **Errors do not show inline** as the task file suggested — a failed fetch goes to the
  status row and the row becomes openable again, so `<CR>` retries.
