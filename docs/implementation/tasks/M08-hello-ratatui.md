# M8 — Hello ratatui

## What

Minimal ratatui binary. Empty terminal. "lazydap" centered. `q` quits. No DAP. No protocol. No daemon. ~80 lines.

## Why

You need to learn ratatui's render loop and crossterm event handling before adding DAP into it. Doing it in isolation is the right size of bite — one new concept (ratatui), no other moving parts.

## How

### Step 1 — Create the TUI crate

```bash
mkdir -p crates/tui/src
```

`crates/tui/Cargo.toml`:

```toml
[package]
name = "lazydap-tui"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
ratatui = "0.27"        # check current version on crates.io at time of M8
crossterm = "0.27"
tokio.workspace = true
anyhow = "1"
```

Add to root `Cargo.toml` workspace members.

### Step 2 — Minimal app

`crates/tui/src/lib.rs`:

```rust
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{io, time::Duration};

pub fn run() -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    result
}

fn run_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> anyhow::Result<()> {
    loop {
        terminal.draw(|frame| {
            let area = frame.size();
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0)])
                .split(area);
            let block = Block::default()
                .title("lazydap")
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Cyan));
            let text = Paragraph::new(Line::from("press q to quit"))
                .alignment(Alignment::Center)
                .block(block);
            frame.render_widget(text, layout[0]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                    break;
                }
            }
        }
    }
    Ok(())
}
```

### Step 3 — Wire into daemon binary

`crates/daemon/src/main.rs` — add a `Tui` subcommand and a bare-no-args path that enters the TUI when interactive:

```rust
match cli.command {
    None => {
        if atty::is(atty::Stream::Stdout) {
            lazydap_tui::run()?;
        } else {
            print_help();
        }
    }
    Some(Commands::Tui) => lazydap_tui::run()?,
    // ...
}
```

Add `lazydap-tui = { path = "../tui" }` to `crates/daemon/Cargo.toml`.

### Step 4 — Run

```bash
cargo run --bin lazydap
# or
cargo run --bin lazydap -- tui
```

You see a bordered box, "lazydap" title, "press q to quit" text. `q` exits. Terminal restored cleanly.

## Success criteria

- `lazydap` (bare, in TTY) opens the TUI.
- TUI shows a bordered area with "lazydap" title and "press q to quit" centered.
- `q` and `Esc` quit cleanly. Terminal is restored on quit (no garbled prompt).
- `cargo build` and `cargo test` still pass.
- Running `lazydap` non-interactively (e.g. piping output) does NOT enter the TUI.

## Files

- `crates/tui/Cargo.toml`, `src/lib.rs` (new)
- `crates/daemon/src/main.rs` — wire up the bare-no-args path

## Verify

```bash
cargo build -p lazydap-tui
cargo run --bin lazydap   # should open TUI; press q
echo "" | lazydap         # should NOT open TUI; should print help or behave gracefully
```

## Depends on

- [`M07-skill-agent-verification`](M07-skill-agent-verification.md) — Phase B done. Could in principle be done earlier, but Phase B's discipline (CLI-first) anchors why the TUI is a thin client later.

## Notes

- **No ratatui state model yet.** That's M10.
- **No connection to daemon.** That's M11.
- **No layout panes.** Just one centered text. M9 adds source rendering.
- **`alternate_screen` and `raw_mode`** must be cleaned up on panic too — wrap the run loop in a panic guard if you're paranoid. Optional for M8.
- **Don't try to make it pretty yet.** M9–M11 are about correctness. Phase D adds polish.
