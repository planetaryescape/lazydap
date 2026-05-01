# M9 — Show a file

## What

Open a hardcoded source file (`examples/c-hello/main.c`), render it full-screen with line numbers in the TUI. Cursor moves with `j`/`k`/`<C-d>`/`<C-u>`. `q` quits.

Still no DAP. Still no daemon connection.

## Why

Source rendering is the centrepiece of the TUI. M9 isolates it: file open, line iteration, scrolling, rendering. Once this works, M10 wraps it in Elm Architecture and M11 adds DAP-driven current-line markers.

## How

### Step 1 — Add a source pane module

`crates/tui/src/panes/source.rs`:

```rust
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use std::path::PathBuf;

pub struct SourceView {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub cursor_line: u32,         // 1-indexed
    pub scroll_offset: u16,       // 0-indexed
}

impl SourceView {
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(&path)?;
        Ok(Self {
            path,
            lines: content.lines().map(String::from).collect(),
            cursor_line: 1,
            scroll_offset: 0,
        })
    }

    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let new = (self.cursor_line as i32 + delta).clamp(1, self.line_count() as i32);
        self.cursor_line = new as u32;
        // Auto-scroll if cursor goes off-screen — set scroll_offset to keep cursor visible.
        // Simplest: viewport_height is passed in render, scroll_offset adjusted there.
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let height = area.height.saturating_sub(2); // borders
        // adjust scroll
        if self.cursor_line < self.scroll_offset as u32 + 1 {
            self.scroll_offset = self.cursor_line.saturating_sub(1) as u16;
        }
        if self.cursor_line >= self.scroll_offset as u32 + height as u32 {
            self.scroll_offset = (self.cursor_line as u16).saturating_sub(height) + 1;
        }

        let total_lines = self.lines.len();
        let line_num_width = total_lines.to_string().len();

        let visible: Vec<Line> = self.lines
            .iter()
            .enumerate()
            .skip(self.scroll_offset as usize)
            .take(height as usize)
            .map(|(i, content)| {
                let line_no = (i + 1) as u32;
                let mut spans = vec![
                    Span::styled(
                        format!("{:>w$} ", line_no, w = line_num_width),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];
                let style = if line_no == self.cursor_line {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(content.clone(), style));
                Line::from(spans)
            })
            .collect();

        let block = Block::default()
            .title(format!("source · {}", self.path.display()))
            .borders(Borders::ALL);
        let para = Paragraph::new(visible).block(block).wrap(Wrap { trim: false });
        frame.render_widget(para, area);
    }
}
```

### Step 2 — Update `lib.rs`

```rust
mod panes;
use panes::source::SourceView;

fn run_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>) -> anyhow::Result<()> {
    let mut source = SourceView::open(PathBuf::from("examples/c-hello/main.c"))?;
    loop {
        terminal.draw(|frame| {
            let area = frame.size();
            source.render(frame, area);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('j') | KeyCode::Down => source.move_cursor(1),
                    KeyCode::Char('k') | KeyCode::Up => source.move_cursor(-1),
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        source.move_cursor(10);
                    }
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        source.move_cursor(-10);
                    }
                    KeyCode::Char('g') => { source.cursor_line = 1; source.scroll_offset = 0; }
                    KeyCode::Char('G') => { source.cursor_line = source.line_count(); }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
```

### Step 3 — Run

```bash
cargo run --bin lazydap
```

You see `examples/c-hello/main.c` rendered with line numbers. `j`/`k` move cursor. `q` quits.

## Success criteria

- Source pane renders the file with line numbers.
- Cursor (highlighted background) moves with `j`/`k`.
- `<C-d>`/`<C-u>` half-page scroll.
- `gg` jumps to line 1, `G` to last line.
- Scrolling: cursor stays on-screen as it moves; content scrolls when cursor exits viewport.
- File path shown in pane title.

## Files

- `crates/tui/src/panes/mod.rs`, `src/panes/source.rs` (new)
- `crates/tui/src/lib.rs` — update run loop

## Verify

```bash
cargo run --bin lazydap
# - press j repeatedly: cursor moves down
# - press G: cursor jumps to last line
# - press gg: cursor jumps to line 1
# - press q: clean exit
```

## Depends on

- [`M08-hello-ratatui`](M08-hello-ratatui.md) — ratatui basics work.

## Notes

- **No syntax highlighting yet.** That's polish; defer.
- **No multi-pane layout.** That's M12 (stack pane). For now full-screen source.
- **State is mutated in `run_loop`.** That's the anti-pattern Phase C.M10 will fix. Don't worry about it now — refactor in M10.
- **`SourceView::open` reads the entire file.** Fine for source files (typically <10KB). For 100MB binaries, optimise later.
