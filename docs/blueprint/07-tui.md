# 07 — TUI

The TUI is one client of the lazydap protocol. It does not have privileged access to the daemon. Every operation flows through IPC, exactly like the CLI.

## Design principles

1. **Keyboard-first.** Mouse support is post-v0.1. Every operation has a key binding.
2. **No blocking on the daemon.** All daemon I/O is async; the render loop never waits.
3. **Pure render from state.** No mutations during `view()`. Reducer is the only place state changes.
4. **vim-ish keymap by default.** Customisable via config, post-v0.1.

## Architecture

`crates/tui/`:

```
src/
├── main.rs               # not the binary's main; library entry
├── app.rs                # App struct, run_loop
├── state.rs              # Model — entire UI state
├── update.rs             # update(state, msg) -> (state, cmd)
├── view.rs               # view(frame, state) — pure render
├── ipc_client.rs         # IPC connection, sends Request, receives Response/Event
├── input.rs              # KeyEvent → Msg
└── panes/
    ├── source.rs
    ├── stack.rs
    ├── scopes.rs
    ├── breakpoints.rs
    ├── watches.rs
    ├── repl.rs
    ├── output.rs
    └── status_bar.rs
```

## State model

```rust
pub struct AppState {
    pub session: Option<SessionView>,        // None when no active session
    pub focused_pane: Pane,
    pub source_view: SourceView,
    pub stack_view: StackView,
    pub scopes_view: ScopesView,
    pub breakpoints_view: BreakpointsView,
    pub watches_view: WatchesView,
    pub repl_view: ReplView,
    pub output_view: OutputView,
    pub status: StatusLine,
    pub modal: Option<Modal>,                // active modal (eval prompt, confirm, etc.)
    pub size: (u16, u16),
}

pub enum Pane { Source, Stack, Scopes, Breakpoints, Watches, Repl, Output }

pub struct SessionView {
    pub session_id: SessionId,
    pub state: SessionState,                 // mirrors daemon
    pub current_frame: Option<FrameId>,
    pub current_thread: Option<ThreadId>,
}
```

Note: `SessionView` is a *cached projection* of the daemon's state. It's updated by `Event::Stopped`, `Event::Continued`, etc. Never written by clients directly.

## Message types

```rust
pub enum Msg {
    // Input
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,                                    // 60Hz UI refresh

    // IPC inbound
    DaemonResponse { id: u64, response: Response },
    DaemonEvent(Event),
    DaemonError(IpcError),
    DaemonDisconnected,

    // Internal
    LoadSourceCompleted(PathBuf, Result<String>),
    OpenModal(Modal),
    CloseModal,
}

pub enum Cmd {
    Quit,
    SendIpc(Request),
    LoadSource(PathBuf),
    None,
}
```

## The reducer

`update.rs` is a giant `match`. Every key, every IPC event, every internal signal becomes one branch.

Stub:

```rust
pub fn update(state: AppState, msg: Msg) -> (AppState, Cmd) {
    match msg {
        Msg::Key(KeyEvent { code: KeyCode::F(5), .. }) => {
            (state, Cmd::SendIpc(Request::Continue {
                session_id: state.session.as_ref().map(|s| s.session_id).unwrap_or_default(),
                thread_id: state.session.and_then(|s| s.current_thread),
                wait: WaitMode::NoWait,
                all_threads: false,
            }))
        }
        Msg::Key(KeyEvent { code: KeyCode::Char('b'), .. }) if state.focused_pane == Pane::Source => {
            // Toggle breakpoint at current source line
            let line = state.source_view.cursor_line;
            let path = state.source_view.path.clone();
            (state, Cmd::SendIpc(Request::BreakpointToggle { source: path, line }))
        }
        Msg::DaemonEvent(Event::Stopped { thread_id, reason, .. }) => {
            let state = state.with_paused(thread_id, reason);
            // Trigger refetch of stack & scopes
            (state, Cmd::SendIpc(Request::StackTrace { ... }))
        }
        // ... every event one branch
        _ => (state, Cmd::None),
    }
}
```

## Layout

Default layout (post-M11):

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│ lazydap · session 01ABC · paused at main.c:42 · breakpoint hit                      │ status bar
├──────────────────────────────────────────────────────┬──────────────────────────────┤
│                                                      │ ▾ Stack (3 frames)           │
│   1  #include <stdio.h>                              │   main main.c:42             │
│   2                                                  │   process foo.c:101          │
│   3  int main(int argc, char *argv[]) {              │   _start crt.S:67            │
│ → 4      int x = 5;                                  │                              │
│   5      int y = x * 2;                              │ ▾ Scopes                     │
│   6      printf("hello\n");                          │   ▸ Locals (3)               │
│   7      return 0;                                   │   ▸ Arguments (2)            │
│   8  }                                               │   ▸ Globals (12)             │
│   9                                                  │                              │
│                                                      │ ▾ Watches (2)                │
│                                                      │   x + y = 15                 │
│                                                      │   *ptr = 0xdeadbeef          │
├──────────────────────────────────────────────────────┴──────────────────────────────┤
│ REPL                                                                                │
│ > p tokens                                                                          │
│ {array of 12 elements}                                                              │
│ > █                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

## Keybindings (default, vim-flavoured)

### Global

- `q` — quit
- `?` — open keybinding help modal
- `:` — command mode (forthcoming, post-v0.1)
- `Tab` / `Shift-Tab` — cycle focus between panes
- `gt` / `gT` — switch session (post-v0.1)
- `<C-l>` — redraw

### Source pane (focused)

- `j` / `k` / `↓` / `↑` — line down/up
- `gg` / `G` — file start / end
- `<C-d>` / `<C-u>` — half-page down/up
- `b` — toggle breakpoint at cursor line
- `B` — set conditional breakpoint (opens modal)
- `<CR>` — jump to top-frame line (re-centre)
- `]b` / `[b` — next/previous breakpoint in file
- `K` — hover (eval expression under cursor, show in popup)

### Stack pane

- `j` / `k` — frame down/up
- `<CR>` — jump source view to selected frame

### Scopes pane

- `j` / `k` — row down/up
- `<CR>` — expand/collapse variable
- `e` — eval expression (modal), result added to watches

### Breakpoints pane

- `j` / `k` — row down/up
- `<CR>` — jump source to breakpoint location
- `d` — disable/enable
- `dd` — delete

### Watches pane

- `a` — add expression (modal)
- `dd` — delete current
- `e` — edit current

### REPL pane

- typing — append to prompt
- `<CR>` — submit
- `<C-p>` / `<C-n>` — history previous/next

### Stepping (work in any pane)

- `F5` / `c` — continue
- `F10` / `n` — step over
- `F11` / `s` — step in
- `Shift-F11` / `S` — step out
- `F6` — pause

These match nvim-dap conventions where possible.

## Render-loop pattern

```rust
async fn run_loop(mut state: AppState, mut ipc: IpcClient, mut input_rx: mpsc::Receiver<Msg>) -> Result<()> {
    let mut terminal = setup_terminal()?;
    let mut tick = tokio::time::interval(Duration::from_millis(16)); // ~60Hz

    loop {
        terminal.draw(|f| view::view(f, &state))?;

        let msg = tokio::select! {
            Some(input_msg) = input_rx.recv() => input_msg,
            Some(daemon_msg) = ipc.next() => daemon_msg,
            _ = tick.tick() => Msg::Tick,
        };

        let (new_state, cmd) = update::update(state, msg);
        state = new_state;

        match cmd {
            Cmd::Quit => break,
            Cmd::SendIpc(req) => { ipc.send(req).await?; }
            Cmd::LoadSource(path) => { spawn_load_source(path, /* sender */); }
            Cmd::None => {}
        }
    }

    restore_terminal()?;
    Ok(())
}
```

That's the entire core loop. ~30 lines. Discipline (everything goes through `update`) is what keeps it small.

## What the TUI must NOT do

1. **Mutate daemon state directly.** Always send a `Request`.
2. **Cache more than necessary.** The daemon is the source of truth. The TUI shows what the daemon told it.
3. **Block the render thread.** Every I/O goes through `Cmd` and `tokio::spawn`.
4. **Use a different vocabulary than the protocol.** TUI types in `crates/tui/` should map 1:1 to `crates/protocol/` types.
5. **Implement features the CLI doesn't have.** This breaks the CLI-first rule.

## Source rendering details

- Streamed once per file open, cached per session.
- Syntax highlighting via `syntect` or `tree-sitter`. Decision deferred — `tree-sitter` if we already use it, `syntect` if not. Probably `syntect` for simplicity.
- Current line marker: coloured arrow `→` in left gutter.
- Breakpoint markers: `●` (verified), `◯` (unverified), `⊘` (disabled). Per [`02-data-model.md`](02-data-model.md) `AdapterBreakpoint::verified`.
- Line numbers always on (configurable).

## Modals

Modal types in v0.1:

- **Add breakpoint condition** — text input
- **Add watch** — text input
- **Eval expression** — text input + result
- **Confirm dangerous action** — yes/no (e.g., remove all breakpoints)
- **Help** — keybinding cheatsheet

Modals are managed in `state.modal: Option<Modal>`. Closing a modal sends `Msg::CloseModal`.

## Multi-pane focus

The focused pane is highlighted (border colour change). `Tab` cycles. Focus determines which keys are interpreted in pane-specific bindings.

## Performance

- 60Hz tick is aggressive but ratatui handles it fine for typical screen sizes.
- Source pane only renders visible lines (`Paragraph` with offset).
- Variable expansion is lazy (cached after first expansion).
- Output pane uses a ring buffer (1MB cap, oldest dropped).

If profiling shows render budget exceeded: drop tick rate first (30Hz / 15Hz acceptable for a debugger), optimise rendering second.

## See also

- [`02-data-model.md`](02-data-model.md) — types the TUI displays
- [`04-protocol.md`](04-protocol.md) — IPC the TUI uses
- [`docs/reference/ratatui-patterns.md`](../reference/ratatui-patterns.md) — patterns we lean on (TODO)
