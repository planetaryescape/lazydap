//! Drawing the state. Reads it; does not decide anything about it.
//!
//! `&mut AppState` rather than `&AppState` for one reason, and it is worth
//! knowing: [`SourceView::render`] learns the pane's height as it draws, and
//! that height is what "keep the cursor on screen" and "half a page" are
//! measured in. Computing it outside the layout would mean duplicating the
//! layout. Nothing else here writes to the state (D012, M10's notes).

use crate::panes::source::SourceView;
use crate::state::AppState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn view(frame: &mut Frame, state: &mut AppState) {
    // Panes above, one status row that is always exactly one line tall.
    let [body, status] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(frame.area());

    match state.source.as_mut() {
        Some(source) => source.render(frame, body, true),
        None => render_empty(frame, body),
    }

    frame.render_widget(
        Paragraph::new(format!("{} · q to quit", status_text(state)))
            .style(Style::default().fg(Color::DarkGray)),
        status,
    );
}

/// The left-hand end of the status row: where you are, or what went wrong.
fn status_text(state: &AppState) -> String {
    if let Some(notice) = state.notice.as_deref() {
        return notice.to_string();
    }
    match state.source.as_ref() {
        Some(source) => position(source),
        None => "no source".to_string(),
    }
}

fn position(source: &SourceView) -> String {
    format!("{}:{}", source.path().display(), source.cursor_line())
}

fn render_empty(frame: &mut Frame, area: Rect) {
    let block = Block::default().title("lazydap").borders(Borders::ALL);
    frame.render_widget(Paragraph::new("no source loaded").block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::Msg;
    use crate::testing::render;
    use crate::update::update;
    use std::path::PathBuf;

    fn loaded(lines: u32) -> AppState {
        let body: Vec<String> = (1..=lines).map(|line| format!("line {line}")).collect();
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/numbers.txt"),
                contents: Ok(body.join("\n")),
            },
        );
        state
    }

    #[test]
    fn nothing_loaded_yet_still_draws_a_pane_and_a_status_row() {
        assert_eq!(
            render(20, 4, |frame| view(frame, &mut AppState::default())),
            [
                "┌lazydap───────────┐",
                "│no source loaded  │",
                "└──────────────────┘",
                // Clipped at the pane width, not wrapped onto a second row.
                "no source · q to qui",
            ],
        );
    }

    #[test]
    fn a_loaded_file_is_drawn_with_its_line_numbers_and_position() {
        let mut state = loaded(3);
        assert_eq!(
            render(34, 5, |frame| view(frame, &mut state)),
            [
                "┌source · /tmp/numbers.txt───────┐",
                "│1 line 1                        │",
                "│2 line 2                        │",
                "└────────────────────────────────┘",
                "/tmp/numbers.txt:1 · q to quit    ",
            ],
        );
    }

    #[test]
    fn a_file_that_would_not_open_says_so_where_the_position_would_be() {
        let (mut state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("no such file".to_string()),
            },
        );

        let screen = render(40, 4, |frame| view(frame, &mut state));
        assert_eq!(screen[3], "/tmp/gone.c: no such file · q to quit   ");
    }
}
