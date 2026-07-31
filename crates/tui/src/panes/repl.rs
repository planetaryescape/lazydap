//! The REPL pane: ad-hoc expressions against the frame on screen (M17).
//!
//! The same `Eval` request `lazydap eval` sends, typed into a pane instead of a
//! shell. What it is *for* is the expression you want once — a watch is for the
//! one you want at every stop, and promoting every passing question to project
//! state would fill `.lazydap/state.toml` with rubbish.
//!
//! # Two contexts, and why the default is not `repl`
//!
//! DAP's `evaluate` takes a context, and codelldb reads `repl` literally — a
//! line typed at the debug console, handed to LLDB's *command* interpreter
//! rather than its expression evaluator. Typing `x` there does not print `x`;
//! it runs `memory read` and complains about a missing address (quirk 7, D034).
//! A REPL whose most obvious input fails is not a REPL.
//!
//! So this pane sends `watch` context by default, exactly as `lazydap eval`
//! does and for exactly the reason D034 gives. Adapter *commands* are still
//! reachable, behind a `/` prefix: `/bt` runs a backtrace through LLDB. One
//! character, unambiguous — no expression begins with a division — and it keeps
//! the useful half of `repl` context without making it the trap you fall into
//! first.
//!
//! # History is per-session
//!
//! It lives here and dies with the process. Persisting it is a config option
//! for after v0.1; a debugger that wrote every expression you tried into a file
//! in your repository is a surprise nobody asked for.

use crate::panes::input::TextInput;
use lazydap_core::EvalResult;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// The prefix that means "run this as an adapter command, not an expression".
pub const COMMAND_PREFIX: char = '/';

/// How many entries the scrollback keeps.
///
/// A bound rather than a leak: a session left open all afternoon should not
/// grow without limit, and nobody scrolls back past a hundred expressions.
const MAX_ENTRIES: usize = 200;

#[derive(Default)]
pub struct ReplView {
    entries: Vec<ReplEntry>,
    input: TextInput,
    /// Where `<C-p>` has walked back to. `None` means "at the prompt".
    history_cursor: Option<usize>,
    /// Ids are handed out here rather than taken from the entry's position,
    /// because the scrollback is trimmed from the front — so a position is not
    /// a stable name for an entry, and an answer would land on the wrong one.
    next_entry: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReplEntry {
    pub id: u64,
    pub input: String,
    pub output: ReplOutput,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReplOutput {
    /// Sent, nothing back yet.
    Pending,
    Value(EvalResult),
    Error(String),
}

impl ReplView {
    pub fn input_is_empty(&self) -> bool {
        self.input.is_empty()
    }

    // The pane draws itself from its own fields, so these two exist to let the
    // reducer's tests assert on what a key did without reaching inside.
    #[cfg(test)]
    pub fn input(&self) -> &str {
        self.input.as_str()
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[ReplEntry] {
        &self.entries
    }

    pub fn push_char(&mut self, c: char) {
        self.input.push(c);
        // Typing leaves the history: what is on the line is now the user's, not
        // a recalled entry, so `<C-n>` must not overwrite it with the next one.
        self.history_cursor = None;
    }

    pub fn backspace(&mut self) {
        self.input.backspace();
        self.history_cursor = None;
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.history_cursor = None;
    }

    /// Take what was typed and record it as awaiting an answer.
    ///
    /// Returns the entry's id, which is what the answer will be matched on, and
    /// the expression to send. `None` when there was nothing to submit.
    pub fn submit(&mut self) -> Option<(u64, String)> {
        if self.input.is_empty() {
            return None;
        }
        let input = self.input.take();
        self.history_cursor = None;

        self.next_entry += 1;
        let id = self.next_entry;
        self.entries.push(ReplEntry {
            id,
            input: input.clone(),
            output: ReplOutput::Pending,
        });
        if self.entries.len() > MAX_ENTRIES {
            self.entries.remove(0);
        }
        Some((id, input))
    }

    /// Fill in the answer to one entry, if it is still in the scrollback.
    pub fn answer(&mut self, id: u64, output: ReplOutput) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.output = output;
        }
    }

    /// Give up on everything still waiting.
    ///
    /// What a daemon going away means: no answer is ever coming, and an entry
    /// left saying `…` for the rest of the session reads as a pane that has
    /// stopped working.
    pub fn abandon_pending(&mut self, reason: &str) {
        for entry in self.entries.iter_mut() {
            if entry.output == ReplOutput::Pending {
                entry.output = ReplOutput::Error(reason.to_string());
            }
        }
    }

    /// Walk back through what has been typed. `<C-p>`.
    pub fn previous(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let next = match self.history_cursor {
            None => self.entries.len() - 1,
            Some(0) => 0,
            Some(cursor) => cursor - 1,
        };
        self.history_cursor = Some(next);
        self.input.set(self.entries[next].input.clone());
    }

    /// And forward again. `<C-n>` past the newest empties the line, which is
    /// where the user was before they started walking back.
    pub fn next(&mut self) {
        let Some(cursor) = self.history_cursor else {
            return;
        };
        match cursor + 1 {
            next if next < self.entries.len() => {
                self.history_cursor = Some(next);
                self.input.set(self.entries[next].input.clone());
            }
            _ => {
                self.history_cursor = None;
                self.input.clear();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let block = Block::default()
            .title("repl")
            .borders(Borders::ALL)
            .border_style(super::border_style(focused));
        let inner = block.inner(area);

        let mut rows: Vec<Line> = Vec::new();
        for entry in &self.entries {
            rows.push(Line::from(vec![
                Span::styled("> ", Style::default().fg(Color::DarkGray)),
                Span::raw(entry.input.clone()),
            ]));
            rows.push(output_row(&entry.output));
        }
        rows.push(prompt(self.input.as_str(), focused));

        // The prompt is the point of the pane, so the scrollback is anchored to
        // the bottom: a long session must not push the line being typed off
        // the end of the pane.
        let height = inner.height as usize;
        let top = rows.len().saturating_sub(height.max(1));

        frame.render_widget(
            Paragraph::new(rows.split_off(top.min(rows.len()))).block(block),
            area,
        );
    }
}

fn prompt(input: &str, focused: bool) -> Line<'static> {
    let cursor = match focused {
        // Only when the keys are coming here. A block on an unfocused pane
        // says the TUI is waiting for something it is not.
        true => "█",
        false => "",
    };
    Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::raw(format!("{input}{cursor}")),
    ])
}

fn output_row(output: &ReplOutput) -> Line<'static> {
    match output {
        ReplOutput::Pending => {
            Line::from(Span::styled("  …", Style::default().fg(Color::DarkGray)))
        }
        ReplOutput::Error(error) => Line::from(Span::styled(
            format!("  {}", first_line(error)),
            Style::default().fg(Color::Red),
        )),
        ReplOutput::Value(result) => {
            let text = match result.type_name.as_deref() {
                Some(type_name) => format!("  {} : {type_name}", result.value),
                None => format!("  {}", result.value),
            };
            Line::from(Span::styled(
                text,
                Style::default().add_modifier(Modifier::BOLD),
            ))
        }
    }
}

/// Adapters return multi-line diagnostics, and a scrollback row is one line.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

/// Split a submitted line into what to evaluate and whether it is a command.
///
/// `/bt` is LLDB's `bt`; everything else is an expression in the debuggee's
/// language. Returns `None` for a `/` with nothing after it, which is a typo
/// rather than a command.
pub fn parse(input: &str) -> Option<(&str, bool)> {
    match input.strip_prefix(COMMAND_PREFIX) {
        Some(command) if command.trim().is_empty() => None,
        Some(command) => Some((command, true)),
        None => Some((input, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;

    fn result(value: &str, type_name: Option<&str>) -> EvalResult {
        EvalResult {
            value: value.to_string(),
            type_name: type_name.map(str::to_string),
            variables_reference: 0,
        }
    }

    fn typed(view: &mut ReplView, text: &str) {
        for c in text.chars() {
            view.push_char(c);
        }
    }

    fn draw(view: &mut ReplView, width: u16, height: u16, focused: bool) -> Vec<String> {
        render(width, height, |frame| {
            view.render(frame, frame.area(), focused)
        })
    }

    #[test]
    fn submitting_records_the_input_and_waits_for_an_answer() {
        let mut view = ReplView::default();
        typed(&mut view, "x + 1");
        let (id, expression) = view.submit().expect("a submission");

        assert_eq!(expression, "x + 1");
        assert_eq!(view.entries()[0].output, ReplOutput::Pending);
        assert!(
            view.input_is_empty(),
            "the prompt is ready for the next one"
        );

        view.answer(id, ReplOutput::Value(result("6", Some("int"))));
        assert_eq!(
            view.entries()[0].output,
            ReplOutput::Value(result("6", Some("int"))),
        );
    }

    #[test]
    fn an_empty_line_submits_nothing() {
        let mut view = ReplView::default();
        assert!(view.submit().is_none());

        typed(&mut view, "   ");
        assert!(
            view.submit().is_none(),
            "nor does a line of spaces, which would evaluate to an adapter error",
        );
        assert!(view.entries().is_empty());
    }

    #[test]
    fn an_answer_is_matched_to_its_entry_rather_than_to_the_newest_one() {
        // Two in flight, answered out of order. Filling in "the last pending
        // one" would put the first expression's value under the second.
        let mut view = ReplView::default();
        typed(&mut view, "first");
        let (first, _) = view.submit().expect("first");
        typed(&mut view, "second");
        let (second, _) = view.submit().expect("second");

        view.answer(second, ReplOutput::Value(result("2", None)));
        view.answer(first, ReplOutput::Value(result("1", None)));

        assert_eq!(
            view.entries()[0].output,
            ReplOutput::Value(result("1", None))
        );
        assert_eq!(
            view.entries()[1].output,
            ReplOutput::Value(result("2", None))
        );
    }

    #[test]
    fn an_answer_to_an_entry_that_has_scrolled_out_of_the_buffer_is_dropped() {
        let mut view = ReplView::default();
        typed(&mut view, "oldest");
        let (oldest, _) = view.submit().expect("one");
        for index in 0..MAX_ENTRIES {
            typed(&mut view, &format!("e{index}"));
            view.submit().expect("one");
        }

        // Nothing to panic on, and nothing to put it in.
        view.answer(oldest, ReplOutput::Value(result("gone", None)));
        assert_eq!(view.entries().len(), MAX_ENTRIES);
        assert!(view.entries().iter().all(|entry| entry.input != "oldest"));
    }

    #[test]
    fn a_daemon_going_away_stops_everything_waiting_from_waiting_forever() {
        let mut view = ReplView::default();
        typed(&mut view, "x");
        view.submit().expect("one");

        view.abandon_pending("the daemon went away");

        assert_eq!(
            view.entries()[0].output,
            ReplOutput::Error("the daemon went away".to_string()),
        );
    }

    #[test]
    fn history_walks_back_through_what_was_typed_and_forward_again() {
        let mut view = ReplView::default();
        for expression in ["first", "second"] {
            typed(&mut view, expression);
            view.submit().expect("one");
        }

        view.previous();
        assert_eq!(view.input(), "second", "the newest first");
        view.previous();
        assert_eq!(view.input(), "first");
        view.previous();
        assert_eq!(view.input(), "first", "and it stops rather than wrapping");

        view.next();
        assert_eq!(view.input(), "second");
        view.next();
        assert_eq!(
            view.input(),
            "",
            "past the newest is the empty prompt the user started from",
        );
    }

    #[test]
    fn history_on_an_empty_repl_is_a_no_op_rather_than_a_panic() {
        let mut view = ReplView::default();
        view.previous();
        view.next();
        assert!(view.input_is_empty());
    }

    #[test]
    fn typing_after_recalling_leaves_the_history_alone() {
        // Otherwise `<C-n>` would throw away the edit the user just made.
        let mut view = ReplView::default();
        typed(&mut view, "counter");
        view.submit().expect("one");

        view.previous();
        typed(&mut view, " + 1");
        assert_eq!(view.input(), "counter + 1");

        view.next();
        assert_eq!(
            view.input(),
            "counter + 1",
            "the edit survives, because typing left the history",
        );
    }

    #[test]
    fn a_slash_prefix_means_an_adapter_command_and_everything_else_is_an_expression() {
        assert_eq!(parse("x + 1"), Some(("x + 1", false)));
        assert_eq!(parse("/bt"), Some(("bt", true)));
        assert_eq!(parse("/thread list"), Some(("thread list", true)));
        assert_eq!(parse("/"), None, "a bare slash is a typo, not a command");
        assert_eq!(parse("/  "), None);
    }

    #[test]
    fn the_scrollback_and_the_prompt_are_drawn_together() {
        let mut view = ReplView::default();
        typed(&mut view, "x + 1");
        let (id, _) = view.submit().expect("one");
        view.answer(id, ReplOutput::Value(result("6", Some("int"))));

        let screen = draw(&mut view, 30, 5, true);
        assert!(screen[1].contains("> x + 1"), "{screen:?}");
        assert!(screen[2].contains("6 : int"), "{screen:?}");
        assert!(screen[3].contains("> █"), "the prompt: {screen:?}");
    }

    #[test]
    fn an_error_is_shown_against_the_expression_that_caused_it() {
        let mut view = ReplView::default();
        typed(&mut view, "nonsense");
        let (id, _) = view.submit().expect("one");
        view.answer(
            id,
            ReplOutput::Error("use of undeclared identifier 'nonsense'".to_string()),
        );

        let screen = draw(&mut view, 46, 5, true);
        assert!(screen[1].contains("> nonsense"), "{screen:?}");
        assert!(screen[2].contains("undeclared"), "{screen:?}");
    }

    #[test]
    fn a_long_session_keeps_the_prompt_on_screen() {
        // The scrollback is anchored to the bottom. Anchoring it to the top
        // would push the line being typed off the end of the pane.
        let mut view = ReplView::default();
        for index in 0..20 {
            typed(&mut view, &format!("e{index}"));
            let (id, _) = view.submit().expect("one");
            view.answer(id, ReplOutput::Value(result(&index.to_string(), None)));
        }

        let screen = draw(&mut view, 30, 6, true);
        assert!(
            screen[4].contains("> █"),
            "the prompt is the last row: {screen:?}",
        );
        assert!(
            screen.iter().any(|row| row.contains("e19")),
            "and the newest entry is visible: {screen:?}",
        );
    }

    #[test]
    fn an_unfocused_prompt_draws_no_cursor() {
        let mut view = ReplView::default();
        let screen = draw(&mut view, 30, 3, false);
        assert!(
            !screen[1].contains('█'),
            "a block on an unfocused pane says the TUI is waiting for something it is not: {screen:?}",
        );
    }

    #[test]
    fn the_pane_draws_at_heights_that_cannot_fit_it() {
        let mut view = ReplView::default();
        typed(&mut view, "x");
        view.submit().expect("one");

        for (width, height) in [(1, 1), (4, 2), (10, 3)] {
            assert_eq!(draw(&mut view, width, height, true).len(), height as usize);
        }
    }
}
