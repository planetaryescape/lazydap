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
