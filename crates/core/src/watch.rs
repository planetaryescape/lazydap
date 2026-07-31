//! Watch expressions: project state, and what one of them evaluated to.
//!
//! The split here is the whole of M16's design. A [`Watch`] is an *expression*
//! somebody asked to be shown at every stop — it lives in `.lazydap/state.toml`
//! and outlives the session, the daemon and the machine being rebooted. A
//! [`WatchValue`] is what that expression came to at one particular stop, and
//! it is true for exactly as long as the program stays where it is.
//!
//! Persisting the second would be persisting a lie: `tokens[pos]` was `'x'` at
//! the stop before last, and reading that back tomorrow from a file would say
//! it still is. The same division breakpoints make between [`crate::Breakpoint`]
//! and [`crate::AdapterBreakpoint`], for the same reason.

use crate::EvalResult;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::ParseIntError;
use std::str::FromStr;

/// Identifies one watch, for as long as the project keeps it.
///
/// A small integer for the same reasons [`crate::BreakpointId`] is one: it is
/// typed by humans (`lazydap watch remove --id 3`), piped through `xargs`, and
/// read back out of `.lazydap/state.toml`, so it has to be short and stable
/// across daemon restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WatchId(pub u32);

impl fmt::Display for WatchId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for WatchId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.trim().parse().map(Self)
    }
}

/// A watch as lazydap knows it: an expression the project wants evaluated at
/// every stop.
///
/// This is the persisted shape. What it currently *evaluates to* is
/// [`WatchValue`], which is never written to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
    pub id: WatchId,
    /// Handed to the adapter untouched. lazydap does not parse it, because the
    /// language it is written in is the debuggee's, not lazydap's.
    pub expression: String,
    /// What to call it on screen, when the expression itself is unreadable.
    /// `None` means the expression is its own label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Watch {
    /// What to show in the left-hand column: the label if there is one, the
    /// expression otherwise.
    pub fn display_name(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.expression)
    }
}

/// A watch that does not have an id yet: what a client asks for, before the
/// store decides what to call it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewWatch {
    pub expression: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// What a watch came to at one stop.
///
/// Never persisted, and never carried across a stop. An expression that is out
/// of scope in the frame the program is now in is an ordinary outcome rather
/// than a failure of the watch — which is why the error is a variant here and
/// not a `Result` the caller has to decide how to keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchValue {
    /// The adapter evaluated it. [`EvalResult`] rather than a copy of its
    /// fields, because `lazydap eval` already returns exactly this and a second
    /// shape for the same answer is one more thing to keep in step.
    Value(EvalResult),
    /// The adapter refused it — most often because the expression names
    /// something that is not in scope in this frame. Shown against the watch,
    /// dimmed, rather than removing it: the same expression is usually in scope
    /// again a few frames or a few steps later.
    Error(String),
}

impl WatchValue {
    /// One line for a table cell or a pane row.
    pub fn summary(&self) -> String {
        match self {
            Self::Value(result) => result.value.clone(),
            Self::Error(error) => error.clone(),
        }
    }

    /// The adapter's type name, when it gave one.
    pub fn type_name(&self) -> Option<&str> {
        match self {
            Self::Value(result) => result.type_name.as_deref(),
            Self::Error(_) => None,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }
}

/// Which watches a command is talking about.
///
/// A type rather than ad-hoc filters, for the reason
/// [`crate::BreakpointSelector`] is one: non-negotiable #4 requires `--dry-run`
/// and the mutation it previews to pick the *same* watches, and one `pick`
/// called by both is the only way to guarantee that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WatchSelector {
    /// Named explicitly. Unknown ids simply do not match — the caller compares
    /// what it asked for against what came back.
    Ids(Vec<WatchId>),
    /// Whatever was written, matched exactly. What lets somebody remove a watch
    /// they can see on screen without first looking up its id.
    Expression(String),
    All,
}

impl WatchSelector {
    /// The watches this selector picks out, in the order they are held.
    pub fn pick(&self, watches: &[Watch]) -> Vec<Watch> {
        watches
            .iter()
            .filter(|watch| self.matches(watch))
            .cloned()
            .collect()
    }

    pub fn matches(&self, watch: &Watch) -> bool {
        match self {
            Self::Ids(ids) => ids.contains(&watch.id),
            Self::Expression(expression) => watch.expression == *expression,
            Self::All => true,
        }
    }

    /// How to describe the selection in an error or a table.
    pub fn describe(&self) -> String {
        match self {
            Self::Ids(ids) => {
                let ids: Vec<String> = ids.iter().map(WatchId::to_string).collect();
                format!("id {}", ids.join(", "))
            }
            Self::Expression(expression) => format!("`{expression}`"),
            Self::All => "all watches".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(id: u32, expression: &str) -> Watch {
        Watch {
            id: WatchId(id),
            expression: expression.to_string(),
            label: None,
        }
    }

    fn fixture() -> Vec<Watch> {
        vec![watch(1, "counter"), watch(2, "tokens[pos]"), watch(3, "*p")]
    }

    #[test]
    fn a_watch_id_round_trips_as_a_bare_number() {
        let json = serde_json::to_string(&WatchId(7)).expect("serialise");
        assert_eq!(json, "7", "got: {json}");
        assert_eq!("7".parse::<WatchId>().expect("parse"), WatchId(7));
    }

    #[test]
    fn ids_pick_exactly_what_was_named() {
        let picked = WatchSelector::Ids(vec![WatchId(1), WatchId(3)]).pick(&fixture());
        let ids: Vec<u32> = picked.iter().map(|watch| watch.id.0).collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn an_id_that_does_not_exist_picks_nothing_rather_than_erroring_here() {
        // The caller compares what it asked for with what came back; the
        // selector's job is only to say what matched.
        assert!(WatchSelector::Ids(vec![WatchId(99)]).pick(&fixture()).is_empty());
    }

    #[test]
    fn an_expression_picks_the_watch_somebody_can_see_without_its_id() {
        let picked = WatchSelector::Expression("tokens[pos]".to_string()).pick(&fixture());
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, WatchId(2));
    }

    #[test]
    fn an_expression_is_matched_whole_rather_than_as_a_substring() {
        // `counter` must not take `counter + 1` with it: removing more than was
        // asked for is the one mistake a remove cannot be undone from.
        let mut watches = fixture();
        watches.push(watch(4, "counter + 1"));

        let picked = WatchSelector::Expression("counter".to_string()).pick(&watches);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, WatchId(1));
    }

    #[test]
    fn all_means_all() {
        assert_eq!(WatchSelector::All.pick(&fixture()).len(), 3);
    }

    #[test]
    fn a_selector_can_say_what_it_selected_for_a_message() {
        assert_eq!(
            WatchSelector::Ids(vec![WatchId(1), WatchId(2)]).describe(),
            "id 1, 2",
        );
        assert_eq!(WatchSelector::All.describe(), "all watches");
    }

    #[test]
    fn a_label_stands_in_for_the_expression_only_when_there_is_one() {
        let mut watch = watch(1, "self.parser.tokens[self.pos]");
        assert_eq!(watch.display_name(), "self.parser.tokens[self.pos]");

        watch.label = Some("token".to_string());
        assert_eq!(watch.display_name(), "token");
    }

    #[test]
    fn an_out_of_scope_watch_reports_the_error_rather_than_pretending_to_a_value() {
        let error = WatchValue::Error("use of undeclared identifier 'x'".to_string());
        assert!(error.is_error());
        assert_eq!(error.type_name(), None);
        assert!(error.summary().contains("undeclared"));
    }

    #[test]
    fn a_value_carries_the_adapters_type_name_when_it_gave_one() {
        let value = WatchValue::Value(EvalResult {
            value: "42".to_string(),
            type_name: Some("int".to_string()),
            variables_reference: 0,
        });
        assert!(!value.is_error());
        assert_eq!(value.summary(), "42");
        assert_eq!(value.type_name(), Some("int"));
    }

    #[test]
    fn an_optional_field_is_omitted_rather_than_written_as_null() {
        let json = serde_json::to_string(&watch(1, "counter")).expect("serialise");
        assert!(!json.contains("null"), "got: {json}");
    }

    #[test]
    fn a_state_file_entry_without_a_label_still_reads() {
        // Hand-edited `.lazydap/state.toml` is a supported way to add a watch,
        // exactly as it is for a breakpoint (D006), so the minimum sensible
        // entry has to work.
        let watch: Watch = toml::from_str("id = 3\nexpression = \"counter\"\n").expect("read");
        assert_eq!(watch.id, WatchId(3));
        assert_eq!(watch.label, None);
    }
}
