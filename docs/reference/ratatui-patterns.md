# ratatui patterns

Quick reference for the ratatui patterns lazydap leans on. Full docs at [ratatui.rs](https://ratatui.rs).

## The render loop

```rust
fn run<B: Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui(frame))?;
        // handle input
    }
}

fn ui(frame: &mut Frame) {
    let block = Block::default().borders(Borders::ALL).title("Pane");
    frame.render_widget(block, frame.size());
}
```

Each `terminal.draw` is a complete redraw. Don't try to incremental-update; ratatui handles delta diffing internally.

## Layout: columns and rows

```rust
let columns = Layout::default()
    .direction(Direction::Horizontal)
    .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
    .split(frame.size());

let left = columns[0];   // Rect
let right = columns[1];

// Nested:
let right_rows = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Length(10),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(right);
```

`Constraint` variants:

- `Length(n)` — exactly n cells
- `Min(n)` — at least n
- `Max(n)` — at most n
- `Percentage(n)` — % of parent
- `Ratio(num, den)` — exact fraction
- `Fill(weight)` — flex-grow style

## Common widgets

### Block

```rust
Block::default()
    .title("source")
    .borders(Borders::ALL)
    .border_style(Style::default().fg(Color::Cyan))
    .style(Style::default().bg(Color::Black))
```

### Paragraph (multi-line text)

```rust
let para = Paragraph::new(lines)         // Vec<Line> | String | &str
    .block(Block::default().borders(Borders::ALL))
    .wrap(Wrap { trim: false })
    .scroll((scroll_offset, 0));
frame.render_widget(para, area);
```

### List (selectable rows)

```rust
let items: Vec<ListItem> = data.iter().map(|d| ListItem::new(d.label.clone())).collect();
let list = List::new(items)
    .block(Block::default().title("frames").borders(Borders::ALL))
    .highlight_style(Style::default().bg(Color::DarkGray));

let mut state = ListState::default();
state.select(Some(selected_index));
frame.render_stateful_widget(list, area, &mut state);
```

`ListState` tracks selected index and scroll offset.

### Tabs

```rust
let titles: Vec<Line> = ["Watches", "Scopes", "Breakpoints", "Repl"]
    .iter()
    .map(|t| Line::from(*t))
    .collect();
let tabs = Tabs::new(titles)
    .block(Block::default().borders(Borders::ALL))
    .highlight_style(Style::default().fg(Color::Yellow))
    .select(active_tab);
frame.render_widget(tabs, area);
```

Useful for nvim-dap-view-style single-pane multi-tab.

### Table

```rust
let header = Row::new(vec!["ID", "Line", "Condition"]);
let rows: Vec<Row> = bps.iter().map(|bp| {
    Row::new(vec![bp.id.to_string(), bp.line.to_string(), bp.condition.clone().unwrap_or_default()])
}).collect();

let table = Table::new(rows, &[
    Constraint::Length(20),
    Constraint::Length(8),
    Constraint::Min(0),
])
    .header(header)
    .block(Block::default().borders(Borders::ALL));
frame.render_widget(table, area);
```

## Styled text

### Span and Line

```rust
let line = Line::from(vec![
    Span::styled("→ ", Style::default().fg(Color::Yellow)),
    Span::raw("42 "),
    Span::styled("printf(", Style::default().fg(Color::Magenta)),
    Span::styled("\"hello\"", Style::default().fg(Color::Green)),
    Span::raw(");"),
]);
```

`Line` is a row of styled spans. `Vec<Line>` is multi-line text.

### Style

```rust
Style::default()
    .fg(Color::Cyan)
    .bg(Color::Black)
    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
```

Modifiers: `BOLD`, `DIM`, `ITALIC`, `UNDERLINED`, `SLOW_BLINK`, `RAPID_BLINK`, `REVERSED`, `HIDDEN`, `CROSSED_OUT`.

## crossterm event handling

```rust
use crossterm::event::{self, Event, KeyCode, KeyModifiers};

// In the input loop:
if event::poll(Duration::from_millis(100))? {
    match event::read()? {
        Event::Key(key) => {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // <C-d>
                }
                KeyCode::F(5) => continue_program(),
                _ => {}
            }
        }
        Event::Resize(w, h) => { ... }
        Event::Mouse(_) => { ... }      // post-v0.1
        _ => {}
    }
}
```

## Async integration with tokio

ratatui's render loop is sync. Reads/writes are sync. To integrate with async:

```rust
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();

// Background: crossterm input → channel
let input_handle = tokio::task::spawn_blocking(move || {
    loop {
        if event::poll(Duration::from_millis(100)).unwrap() {
            if let Ok(ev) = event::read() {
                let msg = match ev { ... };
                if tx.send(msg).is_err() { break; }
            }
        }
    }
});

// Main loop: select over input + IPC + tick
loop {
    terminal.draw(|f| view(f, &state))?;
    let msg = tokio::select! {
        Some(m) = rx.recv() => m,
        Some(m) = ipc_rx.recv() => m,
        _ = tick.tick() => Msg::Tick,
    };
    // ... update + dispatch cmd
}
```

`spawn_blocking` for crossterm input — `event::poll` is blocking. `tokio::select!` for the unified event loop.

## Setup and teardown

```rust
fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal<B: Backend + io::Write>(mut terminal: Terminal<B>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}
```

Wrap in a panic guard if you want, so panics don't leave the terminal in raw mode.

## Common gotchas

- **`frame.size()` excludes borders.** When using a `Block::default().borders(Borders::ALL)`, the block reduces the inner area by 2 cells in each dimension. Compute scroll/cursor positions against the inner area, not the outer.
- **`render_stateful_widget` mutates state.** Specifically, `ListState`'s `offset` is updated based on the rendered area. This is fine; just remember it's mutating.
- **Wrap = false by default for some widgets.** `Paragraph` clips lines unless you opt into `Wrap { trim: false }`.
- **Layouts split exact pixels.** Percentages round; `Fill` distributes leftover. For pixel-precise layouts, use `Length` or `Min` constraints.
- **Don't render the same widget twice.** ratatui doesn't dedupe. Each `render_widget` call adds another paint. Compose with layout, not by stacking.

## Performance

- ratatui's render is O(visible cells). 100x100 terminal = 10K cells, negligible at 60Hz.
- For long lists/files, render only visible rows (scroll offset + take). Don't construct ListItems for off-screen rows.
- Variable expansion in scopes: lazy. Don't fetch children until expanded.
- 60Hz tick is generous. 30Hz is fine for a debugger. Drop further if profiler shows render budget exceeded.

## Patterns specific to lazydap

### Source pane current-line marker

```rust
// In SourceView::render, for each visible line:
let current = current_line == Some(line_no);
let mut spans = vec![];
spans.push(if current {
    Span::styled("→ ", Style::default().fg(Color::Yellow))
} else {
    Span::raw("  ")
});
spans.push(Span::styled(format!("{:>w$} ", line_no), gutter_style));
spans.push(Span::raw(content));
Line::from(spans)
```

### Breakpoint sign in gutter

```rust
let sign = match bp_state {
    Some(BpState::Verified)   => Span::styled("●", Style::default().fg(Color::Red)),
    Some(BpState::Unverified) => Span::styled("◯", Style::default().fg(Color::Yellow)),
    Some(BpState::Disabled)   => Span::styled("⊘", Style::default().fg(Color::DarkGray)),
    None                      => Span::raw(" "),
};
```

### Modal overlay

ratatui doesn't have a native modal. Pattern: render the underlying view first, then on top, render a centered block:

```rust
let main_area = frame.size();
view::view(frame, &state);              // normal render

if let Some(modal) = &state.modal {
    let modal_area = centered_rect(60, 20, main_area);   // % w, % h
    frame.render_widget(Clear, modal_area);              // clear background
    let modal_block = Block::default().title(modal.title()).borders(Borders::ALL);
    frame.render_widget(modal_block, modal_area);
    // render inside the modal...
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
```

`Clear` is a built-in widget that wipes the area before the modal renders.
