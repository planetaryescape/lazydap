//! The scopes pane: a lazily-expanded tree of variables (M13).
//!
//! # Why it is a tree and not a list
//!
//! Because the adapter's answer is. `Locals` is a handle, not a set of
//! variables; expanding it fetches its children, and a struct or a pointer
//! among them is another handle. Lazily is the only way this can work: a
//! 100,000-element array is one `variables_reference` until somebody asks, and
//! fetching it eagerly would freeze the pane on every stop.
//!
//! # Paths, not references
//!
//! A node is addressed by its index path from the root (`[0, 3]` is the fourth
//! child of the first scope) rather than by its `variables_reference`. The
//! reference is the adapter's handle and is not unique across the tree — two
//! pointers to the same struct share one — so a reply keyed on it could
//! populate the wrong node.

use lazydap_core::{Scope, Variable};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

/// The width of one level of indent.
const INDENT: usize = 2;

/// What a row's first column says about whether it opens.
const EXPANDED: &str = "▾ ";
const COLLAPSED: &str = "▸ ";
const PENDING: &str = "⋯ ";
const LEAF: &str = "  ";

/// An index path from the root of the tree.
pub type NodePath = Vec<usize>;

#[derive(Default)]
pub struct ScopesView {
    /// Top level: one per scope (Locals, Arguments, Globals).
    nodes: Vec<Node>,
    /// Index into the *visible* rows, which is what `j` and `k` move through.
    /// Collapsing a node above the selection can leave this past the end, so
    /// every read of it clamps.
    selected: usize,
    /// Whether these handles are ones the adapter would still recognise. Same
    /// discipline as the stack pane's, and for the same reason: a
    /// `variables_reference` belongs to a frame, and a frame lasts until the
    /// program moves.
    stale: bool,
    /// The id of the `Scopes` answer this tree was built from.
    ///
    /// What an in-flight expansion is tagged with, and the reason it is the
    /// *tree's* generation rather than the newest request's: between asking for
    /// a frame's scopes and being given them, the tree on screen is still the
    /// previous frame's, and an expansion of it belongs to that one.
    generation: u64,
    viewport_height: usize,
    top: usize,
}

/// One row's worth of tree: a scope or a variable.
///
/// The two are the same shape on purpose. A scope is a named handle with
/// children and no value of its own; a struct is a named handle with children
/// *and* a value. Splitting them into two types would mean writing the
/// expansion logic twice.
pub struct Node {
    label: String,
    /// `None` for a scope, which has no value of its own.
    value: Option<String>,
    type_name: Option<String>,
    /// The adapter's handle. Zero means "no children" — a plain `int`.
    reference: i64,
    children: Vec<Node>,
    expanded: bool,
    /// Whether the children have been fetched. A node can be loaded and still
    /// have none: an empty struct, or a scope the adapter reports as empty.
    loaded: bool,
    /// A `Variables` request for this node is in flight.
    pending: bool,
}

impl Node {
    fn scope(scope: Scope) -> Self {
        Self {
            label: scope.name,
            value: None,
            type_name: None,
            reference: scope.variables_reference,
            children: Vec::new(),
            expanded: false,
            loaded: false,
            pending: false,
        }
    }

    fn variable(variable: Variable) -> Self {
        Self {
            label: variable.name,
            value: Some(variable.value),
            type_name: variable.type_name,
            reference: variable.variables_reference,
            children: Vec::new(),
            expanded: false,
            loaded: false,
            pending: false,
        }
    }

    pub fn reference(&self) -> i64 {
        self.reference
    }

    pub fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }

    /// Whether this row can be opened at all.
    pub fn expandable(&self) -> bool {
        self.reference != 0
    }

    /// `x = 5 : int`, or just `Locals` for a scope.
    fn text(&self) -> String {
        let mut text = self.label.clone();
        if let Some(value) = self.value.as_deref() {
            text.push_str(" = ");
            text.push_str(value);
        }
        if let Some(type_name) = self.type_name.as_deref() {
            text.push_str(" : ");
            text.push_str(type_name);
        }
        text
    }

    fn marker(&self) -> &'static str {
        match (self.expandable(), self.pending, self.expanded) {
            (false, _, _) => LEAF,
            (true, true, _) => PENDING,
            (true, false, true) => EXPANDED,
            (true, false, false) => COLLAPSED,
        }
    }
}

/// One visible row, which is what the pane draws and what `j` counts.
pub struct Row {
    pub path: NodePath,
    pub depth: usize,
    pub marker: &'static str,
    pub text: String,
}

impl ScopesView {
    /// Take a fresh set of scopes, as every stop produces.
    ///
    /// Everything already expanded is thrown away rather than re-fetched. The
    /// alternative — remembering which paths were open and re-expanding them —
    /// is a real feature, but it is not this milestone's, and half-doing it
    /// would leave the pane showing values from the previous stop.
    pub fn replace(&mut self, scopes: Vec<Scope>, generation: u64) {
        self.nodes = scopes.into_iter().map(Node::scope).collect();
        self.selected = 0;
        self.top = 0;
        self.stale = false;
        self.generation = generation;
    }

    /// Which answer built the tree that is on screen right now.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The program has moved; every handle in here belongs to a frame it left.
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn is_actionable(&self) -> bool {
        !self.stale
    }

    pub fn clear(&mut self) {
        self.replace(Vec::new(), self.generation);
    }

    /// Every visible row, depth-first, skipping the children of collapsed
    /// nodes. Built on demand: the tree is tens of rows, and a cached copy
    /// would be one more thing that can disagree with the tree.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        collect(&self.nodes, &mut Vec::new(), &mut rows);
        rows
    }

    pub(crate) fn selected_index(&self) -> usize {
        self.selected.min(self.rows().len().saturating_sub(1))
    }

    pub fn selected_path(&self) -> Option<NodePath> {
        let rows = self.rows();
        rows.get(self.selected.min(rows.len().saturating_sub(1)))
            .map(|row| row.path.clone())
    }

    pub fn move_selection(&mut self, delta: i32) {
        let count = self.rows().len();
        if count == 0 {
            return;
        }
        let target = self.selected.min(count - 1) as i64 + i64::from(delta);
        self.selected = target.clamp(0, count as i64 - 1) as usize;
        self.scroll_to_selection();
    }

    pub fn node_at(&self, path: &[usize]) -> Option<&Node> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.nodes.get(first)?;
        for &index in rest {
            node = node.children.get(index)?;
        }
        Some(node)
    }

    fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut Node> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.nodes.get_mut(first)?;
        for &index in rest {
            node = node.children.get_mut(index)?;
        }
        Some(node)
    }

    /// Open or close an already-loaded node.
    pub fn set_expanded(&mut self, path: &[usize], expanded: bool) {
        if let Some(node) = self.node_at_mut(path) {
            node.expanded = expanded;
        }
    }

    /// Mark a node as waiting for its children.
    pub fn mark_pending(&mut self, path: &[usize]) {
        if let Some(node) = self.node_at_mut(path) {
            node.pending = true;
        }
    }

    /// Fill in a node's children and open it.
    ///
    /// A path that no longer addresses anything is dropped rather than
    /// panicking: the tree can be replaced by a new stop while a `Variables`
    /// request for the old one is still in flight.
    pub fn populate(&mut self, path: &[usize], variables: Vec<Variable>) -> bool {
        let Some(node) = self.node_at_mut(path) else {
            return false;
        };
        node.children = variables.into_iter().map(Node::variable).collect();
        node.loaded = true;
        node.pending = false;
        node.expanded = true;
        true
    }

    /// Give up on a node whose fetch failed, so its row stops saying `⋯` for
    /// ever and pressing `<CR>` again retries.
    pub fn abandon_pending(&mut self, path: &[usize]) {
        if let Some(node) = self.node_at_mut(path) {
            node.pending = false;
        }
    }

    /// The handles on the way to `path`, itself included.
    ///
    /// Used to refuse an expansion that would revisit a reference already open
    /// above it. Mutually-referencing pointers are a real shape — a doubly
    /// linked list is one — and without this, holding `<CR>` walks the cycle
    /// until the fetches or the memory run out.
    pub fn ancestor_references(&self, path: &[usize]) -> Vec<i64> {
        let mut references = Vec::new();
        for depth in 1..path.len() {
            if let Some(node) = self.node_at(&path[..depth]) {
                references.push(node.reference);
            }
        }
        references
    }

    fn scroll_to_selection(&mut self) {
        let selected = self.selected_index();
        if self.viewport_height == 0 {
            self.top = selected;
            return;
        }
        if selected < self.top {
            self.top = selected;
        } else if selected >= self.top + self.viewport_height {
            self.top = selected + 1 - self.viewport_height;
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let block = Block::default()
            .title("scopes")
            .borders(Borders::ALL)
            .border_style(super::border_style(focused));
        let inner = block.inner(area);

        self.viewport_height = usize::from(inner.height);
        self.scroll_to_selection();

        let rows = self.rows();
        let selected = self.selected_index();
        let lines: Vec<Line> = if rows.is_empty() {
            vec![Line::from(Span::styled(
                "no scopes",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            rows.iter()
                .enumerate()
                .skip(self.top)
                .take(inner.height as usize)
                .map(|(index, row)| line(row, index == selected, focused))
                .collect()
        };

        frame.render_widget(Paragraph::new(lines).block(block), area);
    }
}

fn line(row: &Row, is_selected: bool, focused: bool) -> Line<'static> {
    let mut style = Style::default();
    if is_selected {
        style = style.bg(Color::DarkGray);
        if focused {
            style = style.add_modifier(Modifier::BOLD);
        }
    }

    Line::from(Span::styled(
        format!(
            "{:indent$}{}{}",
            "",
            row.marker,
            row.text,
            indent = row.depth * INDENT,
        ),
        style,
    ))
}

/// Walk the tree depth-first, gathering the rows that are actually visible.
fn collect(nodes: &[Node], path: &mut NodePath, rows: &mut Vec<Row>) {
    for (index, node) in nodes.iter().enumerate() {
        path.push(index);
        rows.push(Row {
            path: path.clone(),
            depth: path.len() - 1,
            marker: node.marker(),
            text: node.text(),
        });
        if node.expanded {
            collect(&node.children, path, rows);
        }
        path.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::render;

    fn scope(name: &str, reference: i64) -> Scope {
        Scope {
            name: name.to_string(),
            variables_reference: reference,
            expensive: false,
            named_variables: None,
            indexed_variables: None,
        }
    }

    fn variable(name: &str, value: &str, type_name: &str, reference: i64) -> Variable {
        Variable {
            name: name.to_string(),
            value: value.to_string(),
            type_name: Some(type_name.to_string()),
            variables_reference: reference,
            named_variables: None,
            indexed_variables: None,
        }
    }

    fn locals() -> ScopesView {
        let mut view = ScopesView::default();
        view.replace(vec![scope("Locals", 1000), scope("Globals", 1001)], 1);
        view
    }

    fn texts(view: &ScopesView) -> Vec<String> {
        view.rows()
            .iter()
            .map(|row| {
                format!(
                    "{:indent$}{}{}",
                    "",
                    row.marker,
                    row.text,
                    indent = row.depth * INDENT
                )
            })
            .collect()
    }

    fn draw(view: &mut ScopesView, width: u16, height: u16) -> Vec<String> {
        render(width, height, |frame| {
            view.render(frame, frame.area(), true)
        })
    }

    #[test]
    fn a_collapsed_scope_hides_its_children() {
        let mut view = locals();
        view.populate(&[0], vec![variable("x", "5", "int", 0)]);
        view.set_expanded(&[0], false);

        assert_eq!(texts(&view), ["▸ Locals", "▸ Globals"]);
    }

    #[test]
    fn expanding_a_scope_shows_its_variables_indented() {
        let mut view = locals();
        view.populate(
            &[0],
            vec![
                variable("x", "5", "int", 0),
                variable("buf", "[256]", "char[256]", 1002),
            ],
        );

        assert_eq!(
            texts(&view),
            [
                "▾ Locals",
                "    x = 5 : int",
                "  ▸ buf = [256] : char[256]",
                "▸ Globals"
            ],
        );
    }

    #[test]
    fn a_variable_with_no_children_gets_no_expansion_marker() {
        let mut view = locals();
        view.populate(&[0], vec![variable("x", "5", "int", 0)]);

        let node = view.node_at(&[0, 0]).expect("the variable");
        assert!(!node.expandable(), "a plain int has nothing to open");
    }

    #[test]
    fn a_node_waiting_for_its_children_says_so() {
        let mut view = locals();
        view.mark_pending(&[0]);
        assert_eq!(texts(&view)[0], "⋯ Locals");

        view.populate(&[0], vec![variable("x", "5", "int", 0)]);
        assert_eq!(texts(&view)[0], "▾ Locals", "and stops saying it");
    }

    #[test]
    fn a_fetch_that_failed_leaves_the_row_openable_again() {
        // Otherwise the row says `⋯` for the rest of the session and <CR>
        // does nothing, which reads as a dead pane rather than a failed fetch.
        let mut view = locals();
        view.mark_pending(&[0]);
        view.abandon_pending(&[0]);

        assert_eq!(texts(&view)[0], "▸ Locals");
        assert!(!view.node_at(&[0]).expect("the scope").is_pending());
    }

    #[test]
    fn the_selection_walks_the_visible_rows_not_the_hidden_ones() {
        let mut view = locals();
        view.populate(&[0], vec![variable("x", "5", "int", 0)]);

        // Locals, x, Globals — three rows, because Locals is open.
        view.move_selection(1);
        assert_eq!(view.selected_path(), Some(vec![0, 0]));

        view.move_selection(1);
        assert_eq!(view.selected_path(), Some(vec![1]), "past x is Globals");

        view.move_selection(1);
        assert_eq!(view.selected_path(), Some(vec![1]), "and it stops there");
    }

    #[test]
    fn collapsing_above_the_selection_does_not_leave_it_off_the_end() {
        let mut view = locals();
        view.populate(&[0], vec![variable("x", "5", "int", 0)]);
        view.move_selection(2); // Globals, the third row.
        view.set_expanded(&[0], false); // Now there are only two rows.

        assert_eq!(view.selected_path(), Some(vec![1]));
    }

    #[test]
    fn a_path_that_no_longer_exists_is_dropped_rather_than_panicking() {
        // The tree is replaced by the next stop while a fetch for the old one
        // is still in flight.
        let mut view = locals();
        view.replace(vec![scope("Locals", 2000)], 2);

        assert!(!view.populate(&[7], vec![variable("x", "5", "int", 0)]));
        assert!(view.node_at(&[0, 3]).is_none());
    }

    #[test]
    fn the_references_above_a_node_are_reportable_so_a_cycle_can_be_refused() {
        let mut view = locals();
        view.populate(&[0], vec![variable("node", "0x1", "Node *", 1002)]);
        view.populate(&[0, 0], vec![variable("next", "0x2", "Node *", 1000)]);

        // The grandchild points back at Locals' own handle.
        assert_eq!(view.ancestor_references(&[0, 0, 0]), vec![1000, 1002]);
    }

    #[test]
    fn an_empty_tree_says_so_rather_than_drawing_a_blank_box() {
        let mut view = ScopesView::default();
        let screen = draw(&mut view, 30, 4);
        assert!(screen[1].contains("no scopes"), "{screen:?}");
    }

    #[test]
    fn the_pane_draws_the_tree_it_reports() {
        let mut view = locals();
        view.populate(&[0], vec![variable("x", "5", "int", 0)]);
        let screen = draw(&mut view, 30, 6);

        assert_eq!(screen[1], "│▾ Locals                    │");
        assert_eq!(screen[2], "│    x = 5 : int             │");
        assert_eq!(screen[3], "│▸ Globals                   │");
    }

    #[test]
    fn a_long_tree_scrolls_to_keep_the_selection_on_screen() {
        let mut view = ScopesView::default();
        view.replace(vec![scope("Locals", 1000)], 1);
        view.populate(
            &[0],
            (0..20)
                .map(|index| variable(&format!("v{index}"), "0", "int", 0))
                .collect(),
        );
        draw(&mut view, 30, 5); // Three rows of text.

        view.move_selection(20);
        let screen = draw(&mut view, 30, 5);

        assert!(screen[3].contains("v19"), "{screen:?}");
        assert!(!screen[1].contains("Locals"), "{screen:?}");
    }
}
