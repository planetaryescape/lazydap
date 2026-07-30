//! The stack pane: one row per frame, with a selection (M12).
//!
//! The selected frame is not just a highlight. It is what the scopes pane
//! (M13) asks about, so moving it is what makes "show me the caller's
//! variables" work — the single most useful thing a debugger does that a
//! `printf` cannot.

use lazydap_core::StackFrame;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// What a frame with no file behind it shows instead of a location.
///
/// Inlined code, disassembly and a source the adapter only holds in memory are
/// all real frames worth listing; they simply have nothing to open.
const NO_SOURCE: &str = "<no source>";

#[derive(Default)]
pub struct StackView {
    frames: Vec<StackFrame>,
    /// Index into [`Self::frames`]. Always addresses a frame when there is
    /// one, which is the invariant the methods below exist to keep.
    selected: usize,
    /// Whether these frames still describe where the program is.
    ///
    /// Set the moment a new stop is reported and cleared when that stop's
    /// trace arrives. In between, the frames on screen are the previous
    /// stop's: still worth drawing — clearing them would make the pane blink
    /// empty on every single step — but not worth *acting* on, because the
    /// frame ids in them address nothing the adapter still recognises.
    stale: bool,
    /// Inner height of the last draw, so the list can scroll like the source
    /// pane does. Zero until the pane has been drawn once.
    viewport_height: usize,
    /// First visible row.
    top: usize,
}

impl StackView {
    /// Replace the frames, as a new stop does.
    ///
    /// The selection goes back to the top frame rather than trying to hold its
    /// place: frame ids are only valid until the program moves, so "the third
    /// frame" after a step is a different function, not the same one.
    pub fn replace(&mut self, frames: Vec<StackFrame>) {
        self.frames = frames;
        self.selected = 0;
        self.top = 0;
        self.stale = false;
    }

    /// The program has moved; these frames describe where it was.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    /// Whether the selected frame is one the adapter would still recognise.
    pub fn is_actionable(&self) -> bool {
        !self.stale && !self.frames.is_empty()
    }

    pub fn clear(&mut self) {
        self.replace(Vec::new());
    }

    pub fn selected(&self) -> Option<&StackFrame> {
        self.frames.get(self.selected)
    }

    /// Move the selection by `delta` rows, stopping at either end.
    pub fn move_selection(&mut self, delta: i32) {
        if self.frames.is_empty() {
            return;
        }
        let last = self.frames.len() as i64 - 1;
        let target = self.selected as i64 + i64::from(delta);
        self.selected = target.clamp(0, last) as usize;
        self.scroll_to_selection();
    }

    fn scroll_to_selection(&mut self) {
        if self.viewport_height == 0 {
            self.top = self.selected;
            return;
        }
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + self.viewport_height {
            self.top = self.selected + 1 - self.viewport_height;
        }
    }

    /// Draw the pane. Mutates only the remembered height, as the source pane
    /// does and for the same reason (D012, M10's notes).
    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let block = Block::default()
            .title("stack")
            .borders(Borders::ALL)
            .border_style(super::border_style(focused));
        let inner = block.inner(area);

        self.viewport_height = usize::from(inner.height);
        self.scroll_to_selection();

        let rows: Vec<Line> = if self.frames.is_empty() {
            vec![Line::from(Span::styled(
                "no stack",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.frames
                .iter()
                .enumerate()
                .skip(self.top)
                .take(inner.height as usize)
                .map(|(index, frame)| self.row(index, frame, focused))
                .collect()
        };

        frame.render_widget(Paragraph::new(rows).block(block), area);
    }

    fn row(&self, index: usize, frame: &StackFrame, focused: bool) -> Line<'static> {
        let is_selected = index == self.selected;
        let mut style = Style::default();
        if is_selected {
            style = style.bg(Color::DarkGray);
            // Bold only when the pane has the keys, so a glance at the screen
            // says which selection `j` is about to move.
            if focused {
                style = style.add_modifier(Modifier::BOLD);
            }
        }

        Line::from(Span::styled(describe(frame), style))
    }
}

/// One frame as one row: `main.c:19 main`.
///
/// File name rather than full path — the pane is a third of the screen wide,
/// and the interesting end of `/Users/.../examples/c-hello/main.c` is `main.c`.
fn describe(frame: &StackFrame) -> String {
    let location = match frame
        .source
        .as_ref()
        .and_then(|source| source.path.as_ref())
    {
        Some(path) => format!(
            "{}:{}",
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            frame.line,
        ),
        None => NO_SOURCE.to_string(),
    };
    format!("{location} {}", frame.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;
    use lazydap_core::SourceRef;
    use std::path::PathBuf;

    fn frame(id: i64, name: &str, path: Option<&str>, line: u32) -> StackFrame {
        StackFrame {
            id,
            name: name.to_string(),
            source: path.map(|path| SourceRef {
                name: None,
                path: Some(PathBuf::from(path)),
                source_reference: None,
            }),
            line,
            column: 1,
        }
    }

    fn three() -> StackView {
        let mut view = StackView::default();
        view.replace(vec![
            frame(1, "inner", Some("/tmp/main.c"), 19),
            frame(2, "middle", Some("/tmp/main.c"), 30),
            frame(3, "main", Some("/tmp/main.c"), 42),
        ]);
        view
    }

    fn selected(view: &StackView) -> &str {
        &view.selected().expect("a selected frame").name
    }

    fn draw(view: &mut StackView, width: u16, height: u16, focused: bool) -> Vec<String> {
        render(width, height, |frame| {
            view.render(frame, frame.area(), focused)
        })
    }

    #[test]
    fn the_selection_stops_at_both_ends_rather_than_wrapping() {
        let mut view = three();

        view.move_selection(-1);
        assert_eq!(selected(&view), "inner", "there is nothing above the top");

        view.move_selection(99);
        assert_eq!(selected(&view), "main", "nor below the outermost frame");
    }

    #[test]
    fn moving_the_selection_with_no_stack_is_a_no_op_rather_than_a_panic() {
        let mut view = StackView::default();
        view.move_selection(1);
        assert!(view.selected().is_none());
    }

    #[test]
    fn a_new_stack_selects_the_frame_the_program_is_actually_in() {
        // Holding the old index would leave the pane pointing at whatever
        // function now happens to be third — the ids are new every stop.
        let mut view = three();
        view.move_selection(2);
        view.replace(vec![frame(9, "other", Some("/tmp/other.c"), 3)]);

        assert_eq!(selected(&view), "other");
    }

    #[test]
    fn each_frame_is_one_row_of_file_line_and_function() {
        let mut view = three();
        let screen = draw(&mut view, 28, 5, true);

        assert_eq!(screen[1], "│main.c:19 inner           │");
        assert_eq!(screen[3], "│main.c:42 main            │");
    }

    #[test]
    fn a_frame_with_no_file_behind_it_still_gets_a_row() {
        let mut view = StackView::default();
        view.replace(vec![frame(1, "__libc_start", None, 0)]);
        let screen = draw(&mut view, 28, 4, true);

        assert!(screen[1].contains("<no source> __libc_start"), "{screen:?}");
    }

    #[test]
    fn an_empty_stack_says_so_rather_than_drawing_a_blank_box() {
        let mut view = StackView::default();
        let screen = draw(&mut view, 28, 4, false);
        assert!(screen[1].contains("no stack"), "{screen:?}");
    }

    #[test]
    fn a_long_stack_scrolls_to_keep_the_selection_on_screen() {
        let mut view = StackView::default();
        view.replace(
            (0..20)
                .map(|index| {
                    frame(
                        index,
                        &format!("f{index}"),
                        Some("/tmp/main.c"),
                        index as u32,
                    )
                })
                .collect(),
        );
        // Three rows of text inside a five-row pane.
        draw(&mut view, 28, 5, true);

        view.move_selection(19);
        let screen = draw(&mut view, 28, 5, true);

        assert!(
            screen[3].contains("f19"),
            "the last frame is visible: {screen:?}"
        );
        assert!(
            !screen[1].contains("f0"),
            "and the top has scrolled: {screen:?}"
        );
    }
}
