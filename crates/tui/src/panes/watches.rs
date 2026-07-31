//! The watches pane: the project's expressions, and what each came to at this
//! stop (M16).
//!
//! Two lifetimes in one pane, and keeping them apart is the whole design. The
//! **expressions** are the project's — they come from `.lazydap/state.toml`,
//! they are here before a session exists and they are still here after one
//! ends. The **values** belong to a single stop: the moment the program moves,
//! every one of them describes somewhere it has been.
//!
//! So a value is never carried across a stop. It is marked stale the instant a
//! new stop is reported and replaced when that stop's answer lands — the same
//! discipline the stack and scopes panes use, and for a sharper reason than
//! tidiness: a watch still showing `pos = 4` three steps later is a debugger
//! lying about the thing it was asked to keep an eye on.

use lazydap_core::{Watch, WatchId, WatchValue};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct WatchesView {
    watches: Vec<Watch>,
    /// What each expression came to at the stop [`Self::generation`] names.
    /// Missing means "not answered yet", which is drawn as pending.
    values: BTreeMap<WatchId, WatchValue>,
    /// Index into [`Self::watches`].
    selected: usize,
    /// Whether the values describe where the program is now.
    ///
    /// Set the moment a new stop is reported, cleared as answers arrive. In
    /// between, the numbers on screen are the previous stop's — still worth
    /// drawing, because clearing them would make the pane blink empty on every
    /// step, but drawn dimmed so nobody reads them as current.
    stale: bool,
    /// Which round of evaluation the values belong to.
    ///
    /// Every batch of watch evaluations takes a new one, and an answer carrying
    /// an older one is dropped. Without it, selecting a caller frame and then
    /// its callee would let the first batch's answers land on top of the
    /// second's: the right expressions, the wrong frame, and nothing on screen
    /// to say so. The same argument [`super::scopes`] makes for its tree.
    generation: u64,
    viewport_height: usize,
    top: usize,
}

impl WatchesView {
    /// Replace the expressions, as a `WatchList` answer does.
    ///
    /// Values for expressions that are still here are kept: the list changing
    /// because somebody added a fourth watch is no reason to blank the three
    /// that were already evaluated.
    pub fn replace(&mut self, watches: Vec<Watch>) {
        let kept: Vec<WatchId> = watches.iter().map(|watch| watch.id).collect();
        self.values.retain(|id, _| kept.contains(id));
        self.watches = watches;
        self.selected = self.selected.min(self.watches.len().saturating_sub(1));
        self.scroll_to_selection();
    }

    pub fn watches(&self) -> &[Watch] {
        &self.watches
    }

    pub fn is_empty(&self) -> bool {
        self.watches.is_empty()
    }

    pub fn selected(&self) -> Option<&Watch> {
        self.watches.get(self.selected)
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Begin a new round of evaluation, marking what is on screen as describing
    /// where the program *was*.
    ///
    /// Returns the generation the answers must carry to be applied.
    pub fn begin_round(&mut self) -> u64 {
        self.generation += 1;
        self.stale = true;
        self.generation
    }

    /// Record what one expression came to, if this is still the round being
    /// waited on.
    pub fn record(&mut self, generation: u64, id: WatchId, value: WatchValue) -> bool {
        if generation != self.generation {
            return false;
        }
        self.values.insert(id, value);
        // Stale until every expression in the round has answered. Clearing on
        // the first would un-dim the ones still showing the previous stop.
        self.stale = !self
            .watches
            .iter()
            .all(|watch| self.values.contains_key(&watch.id));
        true
    }

    /// Forget every value, keeping every expression.
    ///
    /// What a session ending or a daemon going away means: the expressions are
    /// the project's and outlive both, the values were only ever true while the
    /// program was sitting still.
    pub fn forget_values(&mut self) {
        self.values.clear();
        self.stale = false;
        // A round nobody can answer any more. Bumping it means a reply that
        // arrives after a reconnection cannot land on the cleared pane.
        self.generation += 1;
    }

    /// The pane draws from its own map, so this exists to let the reducer's
    /// tests assert on what an answer did without reaching inside.
    #[cfg(test)]
    pub fn value(&self, id: WatchId) -> Option<&WatchValue> {
        self.values.get(&id)
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.watches.is_empty() {
            return;
        }
        let last = self.watches.len() as i64 - 1;
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

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let block = Block::default()
            .title("watches")
            .borders(Borders::ALL)
            .border_style(super::border_style(focused));
        let inner = block.inner(area);

        self.viewport_height = usize::from(inner.height);
        self.scroll_to_selection();

        let rows: Vec<Line> = if self.watches.is_empty() {
            vec![Line::from(Span::styled(
                "no watches — a to add",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            self.watches
                .iter()
                .enumerate()
                .skip(self.top)
                .take(inner.height as usize)
                .map(|(index, watch)| self.row(index, watch, focused))
                .collect()
        };

        frame.render_widget(Paragraph::new(rows).block(block), area);
    }

    fn row(&self, index: usize, watch: &Watch, focused: bool) -> Line<'static> {
        let mut style = Style::default();
        if index == self.selected {
            style = style.bg(Color::DarkGray);
            if focused {
                style = style.add_modifier(Modifier::BOLD);
            }
        }

        let value = self.values.get(&watch.id);
        // Dimmed when it belongs to a stop the program has left, so a number
        // that is no longer true does not read as one that is.
        let value_style = match (self.stale, value) {
            (_, None) => style.fg(Color::DarkGray),
            (true, _) => style.fg(Color::DarkGray),
            (false, Some(value)) if value.is_error() => style.fg(Color::Red),
            (false, Some(_)) => style,
        };

        Line::from(vec![
            Span::styled(format!("{} = ", watch.display_name()), style),
            Span::styled(describe(value), value_style),
        ])
    }
}

/// The right-hand half of a row: the value, the error, or a mark saying the
/// answer has not arrived.
fn describe(value: Option<&WatchValue>) -> String {
    match value {
        None => "…".to_string(),
        Some(WatchValue::Error(error)) => first_line(error),
        Some(WatchValue::Value(result)) => match result.type_name.as_deref() {
            Some(type_name) => format!("{} : {type_name}", result.value),
            None => result.value.clone(),
        },
    }
}

/// Adapters return multi-line diagnostics, and a pane row is one line.
fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;
    use lazydap_core::EvalResult;

    fn watch(id: u32, expression: &str) -> Watch {
        Watch {
            id: WatchId(id),
            expression: expression.to_string(),
            label: None,
        }
    }

    fn value(text: &str, type_name: Option<&str>) -> WatchValue {
        WatchValue::Value(EvalResult {
            value: text.to_string(),
            type_name: type_name.map(str::to_string),
            variables_reference: 0,
        })
    }

    fn two() -> WatchesView {
        let mut view = WatchesView::default();
        view.replace(vec![watch(1, "counter"), watch(2, "tokens[pos]")]);
        view
    }

    fn draw(view: &mut WatchesView, width: u16, height: u16) -> Vec<String> {
        render(width, height, |frame| {
            view.render(frame, frame.area(), true)
        })
    }

    #[test]
    fn an_answered_watch_shows_its_value_and_type() {
        let mut view = two();
        let round = view.begin_round();
        view.record(round, WatchId(1), value("5", Some("int")));

        let screen = draw(&mut view, 30, 4);
        assert!(screen[1].contains("counter = 5 : int"), "{screen:?}");
    }

    #[test]
    fn a_watch_still_waiting_shows_a_mark_rather_than_a_stale_number() {
        let mut view = two();
        let round = view.begin_round();
        view.record(round, WatchId(1), value("5", None));

        let screen = draw(&mut view, 30, 4);
        assert!(
            screen[2].contains("tokens[pos] = …"),
            "the unanswered one says so: {screen:?}",
        );
    }

    #[test]
    fn an_out_of_scope_watch_keeps_its_row_and_shows_the_error() {
        // Errored watches are not removed: the same expression is usually in
        // scope again a few frames or a few steps later.
        let mut view = two();
        let round = view.begin_round();
        view.record(
            round,
            WatchId(1),
            WatchValue::Error("use of undeclared identifier 'counter'".to_string()),
        );

        let screen = draw(&mut view, 46, 4);
        assert!(screen[1].contains("undeclared"), "{screen:?}");
        assert_eq!(view.watches().len(), 2, "the row stays");
    }

    #[test]
    fn a_multi_line_adapter_error_is_cut_to_one_row() {
        let mut view = two();
        let round = view.begin_round();
        view.record(
            round,
            WatchId(1),
            WatchValue::Error("error: one\nnote: two\nnote: three".to_string()),
        );

        let screen = draw(&mut view, 40, 4);
        assert!(screen[1].contains("error: one"), "{screen:?}");
        assert!(!screen[1].contains("note:"), "{screen:?}");
    }

    #[test]
    fn an_answer_from_a_superseded_round_is_refused() {
        // Selecting a caller and then its callee puts two rounds in flight. The
        // first one's answers describe a frame the pane is no longer showing,
        // and applying them would be the right expression against the wrong
        // frame with nothing on screen to say so.
        let mut view = two();
        let old = view.begin_round();
        let new = view.begin_round();

        assert!(
            !view.record(old, WatchId(1), value("stale", None)),
            "the superseded round is refused",
        );
        assert!(view.record(new, WatchId(1), value("fresh", None)));
        assert_eq!(
            view.value(WatchId(1)),
            Some(&value("fresh", None)),
            "and the current one lands",
        );
    }

    #[test]
    fn a_new_round_dims_what_is_on_screen_rather_than_blanking_it() {
        // Clearing would make the pane blink empty on every single step.
        let mut view = two();
        let round = view.begin_round();
        view.record(round, WatchId(1), value("5", None));
        view.record(round, WatchId(2), value("7", None));

        view.begin_round();
        let screen = draw(&mut view, 30, 4);
        assert!(
            screen[1].contains("counter = 5"),
            "the previous value is still drawn: {screen:?}",
        );
    }

    #[test]
    fn a_session_ending_forgets_the_values_and_keeps_the_expressions() {
        let mut view = two();
        let round = view.begin_round();
        view.record(round, WatchId(1), value("5", None));

        view.forget_values();

        assert_eq!(view.watches().len(), 2, "the project's expressions stay");
        assert_eq!(
            view.value(WatchId(1)),
            None,
            "the adapter that produced this is gone",
        );
    }

    #[test]
    fn an_answer_that_arrives_after_a_session_ended_cannot_land() {
        let mut view = two();
        let round = view.begin_round();
        view.forget_values();

        assert!(
            !view.record(round, WatchId(1), value("5", None)),
            "a reply from the dead session must not repopulate the pane",
        );
    }

    #[test]
    fn removing_a_watch_takes_its_value_with_it() {
        let mut view = two();
        let round = view.begin_round();
        view.record(round, WatchId(2), value("7", None));

        view.replace(vec![watch(1, "counter")]);

        assert_eq!(
            view.value(WatchId(2)),
            None,
            "a value with no expression to belong to is a leak",
        );
    }

    #[test]
    fn adding_a_watch_keeps_the_values_the_others_already_have() {
        let mut view = two();
        let round = view.begin_round();
        view.record(round, WatchId(1), value("5", None));

        view.replace(vec![
            watch(1, "counter"),
            watch(2, "tokens[pos]"),
            watch(3, "new"),
        ]);

        assert_eq!(
            view.value(WatchId(1)),
            Some(&value("5", None)),
            "a fourth watch is no reason to blank the first three",
        );
    }

    #[test]
    fn the_selection_stops_at_both_ends_rather_than_wrapping() {
        let mut view = two();
        view.move_selection(-1);
        assert_eq!(view.selected().expect("one").expression, "counter");

        view.move_selection(99);
        assert_eq!(view.selected().expect("one").expression, "tokens[pos]");
    }

    #[test]
    fn the_selection_survives_the_list_it_points_into_shrinking() {
        let mut view = two();
        view.move_selection(1);
        view.replace(vec![watch(1, "counter")]);

        assert_eq!(
            view.selected().expect("still a selection").expression,
            "counter",
            "an index past the end would panic the next time `dd` was pressed",
        );
    }

    #[test]
    fn moving_the_selection_with_no_watches_is_a_no_op_rather_than_a_panic() {
        let mut view = WatchesView::default();
        view.move_selection(1);
        assert!(view.selected().is_none());
    }

    #[test]
    fn an_empty_pane_says_how_to_fill_it() {
        let mut view = WatchesView::default();
        let screen = draw(&mut view, 30, 4);
        assert!(screen[1].contains("no watches — a to add"), "{screen:?}");
    }

    #[test]
    fn a_label_stands_in_for_a_long_expression() {
        let mut view = WatchesView::default();
        view.replace(vec![Watch {
            id: WatchId(1),
            expression: "self.parser.tokens[self.pos]".to_string(),
            label: Some("token".to_string()),
        }]);
        let round = view.begin_round();
        view.record(round, WatchId(1), value("'x'", None));

        let screen = draw(&mut view, 30, 4);
        assert!(screen[1].contains("token = 'x'"), "{screen:?}");
    }

    #[test]
    fn a_long_list_scrolls_to_keep_the_selection_on_screen() {
        let mut view = WatchesView::default();
        view.replace(
            (1..=20)
                .map(|id| watch(id, &format!("w{id}")))
                .collect::<Vec<_>>(),
        );
        draw(&mut view, 30, 5);

        view.move_selection(19);
        let screen = draw(&mut view, 30, 5);

        assert!(
            screen[3].contains("w20"),
            "the last one is visible: {screen:?}"
        );
        assert!(
            !screen[1].contains("w1 "),
            "and the top scrolled: {screen:?}"
        );
    }
}
