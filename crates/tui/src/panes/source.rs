//! The source pane: a file, a cursor, and the line the program is on.
//!
//! The centrepiece of the TUI. M9 is the file and the cursor; M11 adds the
//! marker that follows the debuggee.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::path::{Path, PathBuf};

/// What the gutter shows before the line number when the program is stopped
/// here, and the two columns of space that keep the text from shifting
/// sideways when it is not.
const MARKER: &str = "▶ ";
const NO_MARKER: &str = "  ";

/// One open file.
///
/// Line numbers are 1-indexed throughout, because every other part of the
/// system that names a line — DAP, the breakpoint file, `file:line` on the
/// command line — is 1-indexed, and a view that counted from zero would be the
/// one place a translation could go wrong.
/// Reading is direct; writing goes through the methods below. A read cannot
/// break an invariant, and the invariants here are worth protecting: the
/// cursor is always on a line that exists, and the scroll offset always keeps
/// it on screen.
pub struct SourceView {
    path: PathBuf,
    lines: Vec<String>,
    pub(crate) cursor_line: u32,
    /// First visible line, 1-indexed.
    top_line: u32,
    /// Inner height of the last draw. Zero until the pane has been drawn once,
    /// which is why every use of it copes with zero.
    viewport_height: u32,
    /// Where the program is stopped, when it is stopped in this file.
    pub(crate) marker_line: Option<u32>,
}

impl SourceView {
    /// Build a pane from a file that has already been read.
    ///
    /// Reading it is the loop's job, not the pane's: a `Cmd::LoadSource` runs
    /// off the render thread and comes back as a `Msg` (D012). Takes the whole
    /// file — source files are kilobytes, and the day lazydap opens a
    /// generated one that is not, the fix is a windowed reader rather than a
    /// lazier line split.
    pub fn from_contents(path: impl Into<PathBuf>, contents: &str) -> Self {
        Self {
            path: path.into(),
            lines: contents.lines().map(str::to_string).collect(),
            cursor_line: 1,
            top_line: 1,
            viewport_height: 0,
            marker_line: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// At least 1, even for an empty file: the cursor has to be somewhere, and
    /// "line 0" is not a line anybody can talk about.
    pub fn line_count(&self) -> u32 {
        (self.lines.len() as u32).max(1)
    }

    /// Put the marker on a line and take the cursor there.
    ///
    /// Moving the cursor too is deliberate: the marker means "the program is
    /// here", and a debugger that showed you that without scrolling to it
    /// would make you go looking.
    pub fn set_marker(&mut self, line: u32) {
        self.marker_line = Some(line.clamp(1, self.line_count()));
        self.go_to_line(line);
    }

    pub fn clear_marker(&mut self) {
        self.marker_line = None;
    }

    /// Move the cursor by `delta` lines, stopping at either end.
    pub fn move_cursor(&mut self, delta: i32) {
        let target = i64::from(self.cursor_line) + i64::from(delta);
        self.go_to_line(target.clamp(1, i64::from(self.line_count())) as u32);
    }

    pub fn go_to_line(&mut self, line: u32) {
        self.cursor_line = line.clamp(1, self.line_count());
        self.scroll_to_cursor();
    }

    pub fn go_to_top(&mut self) {
        self.go_to_line(1);
    }

    pub fn go_to_bottom(&mut self) {
        self.go_to_line(self.line_count());
    }

    /// How far `<C-d>` and `<C-u>` move: half the visible height, as in vim.
    ///
    /// Before the first draw there is no height to halve, so fall back to
    /// something that at least moves. A key that did nothing until the screen
    /// had been painted once would look broken.
    pub fn half_page(&self) -> i32 {
        (self.viewport_height / 2).max(1) as i32
    }

    /// Keep the cursor on screen, scrolling as little as possible.
    fn scroll_to_cursor(&mut self) {
        if self.viewport_height == 0 {
            self.top_line = self.cursor_line;
            return;
        }
        if self.cursor_line < self.top_line {
            self.top_line = self.cursor_line;
        } else if self.cursor_line >= self.top_line + self.viewport_height {
            self.top_line = self.cursor_line - self.viewport_height + 1;
        }
    }

    /// Draw the pane.
    ///
    /// Mutates: the pane learns its height here and nowhere else, because the
    /// height is a fact about the layout rather than about the state. That is
    /// the one exception to "no mutation in the view" (D012, M10's notes).
    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .title(format!("source · {}", self.path.display()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);

        self.viewport_height = u32::from(inner.height);
        self.scroll_to_cursor();

        let gutter_width = self.line_count().to_string().len();
        let visible: Vec<Line> = self
            .lines
            .iter()
            .enumerate()
            .skip(self.top_line.saturating_sub(1) as usize)
            .take(inner.height as usize)
            .map(|(index, content)| self.line(index as u32 + 1, content, gutter_width))
            .collect();

        // Deliberately not wrapped. A wrapped long line would take two rows
        // and push every line below it down by one, so the row a line number
        // is painted on would stop matching the line it names — and the cursor
        // highlight would drift off the line it is meant to be on.
        frame.render_widget(Paragraph::new(visible).block(block), area);
    }

    fn line(&self, number: u32, content: &str, gutter_width: usize) -> Line<'static> {
        let is_marker = self.marker_line == Some(number);

        let marker = if is_marker {
            Span::styled(
                MARKER,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw(NO_MARKER)
        };
        let gutter = Span::styled(
            format!("{number:>gutter_width$} "),
            Style::default().fg(if is_marker {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        );
        let text = Span::styled(
            content.to_string(),
            if number == self.cursor_line {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            },
        );

        Line::from(vec![marker, gutter, text])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;

    fn numbered(count: u32) -> SourceView {
        let body: Vec<String> = (1..=count).map(|line| format!("line {line}")).collect();
        SourceView::from_contents("/tmp/numbers.txt", &body.join("\n"))
    }

    /// Draw once so the pane knows how tall it is, the way the real loop does.
    fn draw(view: &mut SourceView, width: u16, height: u16) -> Vec<String> {
        render(width, height, |frame| view.render(frame, frame.area()))
    }

    #[test]
    fn the_cursor_stops_at_both_ends_rather_than_wrapping() {
        let mut view = numbered(3);

        view.move_cursor(-5);
        assert_eq!(view.cursor_line, 1, "there is nothing above line 1");

        view.move_cursor(99);
        assert_eq!(view.cursor_line, 3, "nor below the last line");
    }

    #[test]
    fn an_empty_file_still_has_a_line_for_the_cursor_to_be_on() {
        let mut view = SourceView::from_contents("/tmp/empty.txt", "");
        view.go_to_bottom();
        assert_eq!(view.line_count(), 1);
        assert_eq!(view.cursor_line, 1);
    }

    #[test]
    fn the_content_scrolls_once_the_cursor_would_leave_the_screen() {
        let mut view = numbered(50);
        // Four rows of text inside a six-row pane.
        draw(&mut view, 20, 6);

        view.go_to_line(4);
        let screen = draw(&mut view, 20, 6);
        assert!(screen[1].contains("1 line 1"), "got: {screen:?}");

        view.move_cursor(1);
        let screen = draw(&mut view, 20, 6);
        assert!(
            screen[1].contains("2 line 2"),
            "the top should have moved by exactly one, got: {screen:?}",
        );
        assert!(screen[4].contains("5 line 5"), "got: {screen:?}");
    }

    #[test]
    fn jumping_to_the_end_scrolls_the_end_into_view() {
        let mut view = numbered(50);
        draw(&mut view, 20, 6);

        view.go_to_bottom();
        let screen = draw(&mut view, 20, 6);

        assert_eq!(view.cursor_line, 50);
        assert!(screen[4].contains("50 line 50"), "got: {screen:?}");
    }

    #[test]
    fn half_a_page_is_half_the_visible_height() {
        let mut view = numbered(50);
        draw(&mut view, 20, 12); // ten rows of text
        assert_eq!(view.half_page(), 5);

        view.move_cursor(view.half_page());
        assert_eq!(view.cursor_line, 6);
    }

    #[test]
    fn half_a_page_still_moves_before_anything_has_been_drawn() {
        // Otherwise <C-d> does nothing until the first paint, which reads as a
        // broken key rather than as an unpainted screen.
        let view = numbered(50);
        assert_eq!(view.half_page(), 1);
    }

    #[test]
    fn the_pane_shows_line_numbers_and_the_file_it_is_showing() {
        let mut view = numbered(3);
        let screen = draw(&mut view, 34, 5);

        assert!(
            screen[0].contains("source · /tmp/numbers.txt"),
            "got: {screen:?}",
        );
        assert_eq!(screen[1], "│  1 line 1                      │");
        assert_eq!(screen[3], "│  3 line 3                      │");
    }

    #[test]
    fn the_marker_sits_in_the_gutter_and_the_view_follows_it() {
        let mut view = numbered(50);
        draw(&mut view, 24, 6);

        view.set_marker(40);
        let screen = draw(&mut view, 24, 6);

        assert_eq!(view.marker_line, Some(40));
        assert_eq!(view.cursor_line, 40, "the marker takes the cursor with it");
        assert!(
            screen.iter().any(|row| row.contains("▶ 40 line 40")),
            "got: {screen:?}",
        );
    }

    #[test]
    fn the_gutter_is_the_same_width_with_and_without_a_marker() {
        // Or every line shifts sideways as the program steps, which reads as
        // the whole file moving rather than the marker.
        let mut view = numbered(9);
        let without = draw(&mut view, 24, 5);
        view.set_marker(2);
        let with = draw(&mut view, 24, 5);

        assert_eq!(without[1], "│  1 line 1            │");
        assert_eq!(with[2], "│▶ 2 line 2            │");
    }

    #[test]
    fn a_marker_past_the_end_of_the_file_lands_on_the_last_line() {
        // An adapter reporting a line the file does not have is a real thing
        // — a stale build — and it must not put the marker nowhere.
        let mut view = numbered(3);
        view.set_marker(99);
        assert_eq!(view.marker_line, Some(3));
    }

    #[test]
    fn clearing_the_marker_leaves_the_cursor_where_it_was() {
        let mut view = numbered(50);
        view.set_marker(20);
        view.clear_marker();

        assert_eq!(view.marker_line, None);
        assert_eq!(view.cursor_line, 20, "resuming should not scroll you away");
    }
}
