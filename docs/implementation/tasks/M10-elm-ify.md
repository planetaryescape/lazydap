# M10 — Elm-ify the loop

## What

Refactor M9's TUI into the Model/Msg/update/view shape. Same observable behaviour, structured. Write the `(State, Msg) -> (State, Cmd)` reducer by hand. ~150 lines.

## Why

This is the load-bearing milestone of Phase C. Every milestone after M10 (M11, M12, M13, M14, M15, M16, M17) extends the reducer with new `Msg` and `Cmd` variants. If M10 is right, those extensions are one-branch additions to the match. If M10 is wrong, every later milestone has to fight the architecture.

**Don't skip M10.** Per [`/docs/blueprint/15-decision-log.md`](../../blueprint/15-decision-log.md) D012 and [`/AGENTS.md`](../../AGENTS.md).

## How

### Step 1 — Define the Msg / Cmd / Model

`crates/tui/src/state.rs`:

```rust
use std::path::PathBuf;
use crate::panes::source::SourceView;

pub struct AppState {
    pub source_view: Option<SourceView>,
    pub focused_pane: Pane,
    pub size: (u16, u16),
    pub status: String,
}

pub enum Pane {
    Source,
    // Stack, Scopes, etc. — added in later milestones
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            source_view: None,
            focused_pane: Pane::Source,
            size: (0, 0),
            status: "lazydap · M10".into(),
        }
    }
}
```

`crates/tui/src/msg.rs`:

```rust
use crossterm::event::KeyEvent;
use std::path::PathBuf;

pub enum Msg {
    Key(KeyEvent),
    Resize(u16, u16),
    Tick,
    LoadSourceCompleted(PathBuf, std::io::Result<String>),
}

pub enum Cmd {
    Quit,
    LoadSource(PathBuf),
    None,
}
```

### Step 2 — Reducer

`crates/tui/src/update.rs`:

```rust
use crossterm::event::{KeyCode, KeyModifiers};
use crate::{msg::*, state::*, panes::source::SourceView};

pub fn update(mut state: AppState, msg: Msg) -> (AppState, Cmd) {
    match msg {
        Msg::Resize(w, h) => {
            state.size = (w, h);
            (state, Cmd::None)
        }
        Msg::Tick => (state, Cmd::None),
        Msg::Key(key) => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => (state, Cmd::Quit),
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(s) = state.source_view.as_mut() { s.move_cursor(1); }
                (state, Cmd::None)
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if let Some(s) = state.source_view.as_mut() { s.move_cursor(-1); }
                (state, Cmd::None)
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(s) = state.source_view.as_mut() { s.move_cursor(10); }
                (state, Cmd::None)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(s) = state.source_view.as_mut() { s.move_cursor(-10); }
                (state, Cmd::None)
            }
            KeyCode::Char('g') => {
                if let Some(s) = state.source_view.as_mut() {
                    s.cursor_line = 1; s.scroll_offset = 0;
                }
                (state, Cmd::None)
            }
            KeyCode::Char('G') => {
                if let Some(s) = state.source_view.as_mut() { s.cursor_line = s.line_count(); }
                (state, Cmd::None)
            }
            _ => (state, Cmd::None),
        },
        Msg::LoadSourceCompleted(path, Ok(content)) => {
            let mut sv = SourceView::open_from_string(path, content);
            state.source_view = Some(sv);
            (state, Cmd::None)
        }
        Msg::LoadSourceCompleted(_, Err(e)) => {
            state.status = format!("load failed: {e}");
            (state, Cmd::None)
        }
    }
}
```

### Step 3 — View

`crates/tui/src/view.rs`:

```rust
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use crate::state::AppState;

pub fn view(frame: &mut Frame, state: &mut AppState) {
    let area = frame.size();
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    if let Some(sv) = state.source_view.as_mut() {
        sv.render(frame, layout[0]);
    } else {
        // Empty / "no file" state
        let para = ratatui::widgets::Paragraph::new("No source loaded")
            .block(ratatui::widgets::Block::default().borders(ratatui::widgets::Borders::ALL));
        frame.render_widget(para, layout[0]);
    }

    // Status bar
    let status = ratatui::widgets::Paragraph::new(state.status.clone());
    frame.render_widget(status, layout[1]);
}
```

NB: `view` takes `&mut state` because `SourceView::render` mutates `scroll_offset`. Strict Elm would clone state — for our scale, mutate-in-place is fine and pragmatic.

### Step 4 — Main loop

`crates/tui/src/lib.rs`:

```rust
mod state;
mod msg;
mod update;
mod view;
mod panes;

use std::path::PathBuf;
use msg::*;
use state::*;

pub async fn run() -> anyhow::Result<()> {
    let (input_tx, mut input_rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();

    // crossterm input → channel
    let input_handle = tokio::task::spawn_blocking(move || {
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(event) = event::read() {
                    let msg = match event {
                        Event::Key(k) => Msg::Key(k),
                        Event::Resize(w, h) => Msg::Resize(w, h),
                        _ => continue,
                    };
                    if input_tx.send(msg).is_err() { break; }
                }
            }
        }
    });

    let mut terminal = setup_terminal()?;
    let mut state = AppState::default();

    // Initial cmd: load main.c.
    spawn_load_source(input_tx.clone(), PathBuf::from("examples/c-hello/main.c"));

    let mut tick = tokio::time::interval(Duration::from_millis(16));

    loop {
        terminal.draw(|f| view::view(f, &mut state))?;

        let msg = tokio::select! {
            Some(m) = input_rx.recv() => m,
            _ = tick.tick() => Msg::Tick,
        };

        let (new_state, cmd) = update::update(state, msg);
        state = new_state;

        match cmd {
            Cmd::Quit => break,
            Cmd::LoadSource(path) => spawn_load_source(input_tx.clone(), path),
            Cmd::None => {}
        }
    }

    restore_terminal(terminal)?;
    input_handle.abort();
    Ok(())
}

fn spawn_load_source(tx: tokio::sync::mpsc::UnboundedSender<Msg>, path: PathBuf) {
    tokio::spawn(async move {
        let result = tokio::fs::read_to_string(&path).await;
        let _ = tx.send(Msg::LoadSourceCompleted(path, result));
    });
}
```

## Success criteria

- TUI behaviour identical to M9: source rendered, cursor moves with `j`/`k`, etc.
- All state changes flow through `update()`. No mutation in `view()` except the unavoidable `SourceView::render` scroll-offset adjustment.
- New keys are added by editing `update.rs` only.
- IO (file reads) goes via `Cmd::LoadSource` and async tasks; never directly in `update()`.

## Files

- `crates/tui/src/state.rs` (new)
- `crates/tui/src/msg.rs` (new)
- `crates/tui/src/update.rs` (new)
- `crates/tui/src/view.rs` (new)
- `crates/tui/src/lib.rs` — refactored to the loop above

## Verify

```bash
cargo run --bin lazydap
# Same behaviour as M9. Cursor moves, q quits, terminal restores.
```

Self-test: try adding a new key. E.g., make `H` jump to line 1. You should add ONE branch to the match in `update.rs`. If you can't make a single-line change, the architecture isn't quite right.

## Depends on

- [`M09-show-a-file`](M09-show-a-file.md) — source rendering works.

## Notes

- **`view` takes `&mut state` deliberately.** `SourceView::render` adjusts `scroll_offset` to keep cursor visible. Pure `&state` would force us to compute scroll separately. Pragmatic choice; document it in the file.
- **`Cmd` is small now (Quit, LoadSource).** That's fine. M11 adds `SendIpc(Request)`. The more `Cmd` grows, the more important M10's discipline becomes.
- **Don't introduce a framework for this.** No `tui-realm`, no `iocraft`. This 150 lines IS the framework.
- **Tests for `update()` are easy** — pure function. Add a couple unit tests showing key → state transitions. They'll catch regressions when M11+ adds branches.

## Completed 2026-07-30

`state.rs`, `msg.rs`, `update.rs`, `view.rs` as described. Behaviour is identical to M9 — verified by driving the binary in a pseudo-terminal and comparing the rendered screen for every key, not by reading the diff.

Sixteen tests cover the reducer: each binding, the `gg` prefix (including `gj` disarming it so a later `g` does not jump), key releases, scrolling with no file open, and both outcomes of a load. The self-test the task asks for holds — making `H` jump to the top would be one arm, `KeyCode::Char('H') => with_source(&mut state, SourceView::go_to_top)`.

Deviations from the sketch above:

- **`Msg::Resize` carries no size.** Every draw asks the frame for its own area, so a stored copy could only ever disagree with it. The message exists to make that draw happen now rather than at the next tick.
- **No `focused_pane` or `size` on the state.** There is one pane, and neither field would have been read. M12 adds focus when there is something to focus between.
- **`status: String` became `notice: Option<String>`.** The status row is derived from what the state already knows; a notice is for the things it cannot derive, i.e. a file that would not open.
- **`SourceView::open` is gone.** Reading a file is a `Cmd` that comes back as a `Msg`, which is the whole discipline; leaving a synchronous constructor next to it would have been an invitation to skip the loop.
- **Tick is 100ms, not 16ms.** 60Hz redraws of a static screen buys nothing in a debugger; input and daemon events wake the loop directly. This also matches M9's staleness exactly, which is part of "identical behaviour".

**Bug this milestone shipped, found at M11:** the input pump only noticed the TUI had quit when the *next* keystroke failed to send. A `spawn_blocking` task cannot be aborted and the runtime waits for it at shutdown, so `q` left the process alive until somebody pressed another key. It is invisible to unit tests and to any check that ignores the exit code; a pseudo-terminal run caught it (exit 255 = SIGHUP). Fixed by checking `tx.is_closed()` each poll. See **D039**.

**Follow-up discovered:** `Cmd` is one command, not a list. Every reducer branch so far wants at most one, but the first branch that needs two (M12's "fetch the stack *and* the scopes") will want `Cmd::Batch(Vec<Cmd>)` or a `Vec<Cmd>` return.
