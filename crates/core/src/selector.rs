//! Which breakpoints a command is talking about.
//!
//! Selection is a type rather than a set of ad-hoc filters because
//! non-negotiable #4 requires `--dry-run` and the mutation it previews to pick
//! the *same* breakpoints. One `pick`, called by both, is the only way to
//! guarantee that.

use crate::{Breakpoint, BreakpointId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointSelector {
    /// Named explicitly. Unknown ids simply do not match — the caller compares
    /// what it asked for against what came back.
    Ids(Vec<BreakpointId>),
    /// Everything at one `file:line`.
    Location { source: PathBuf, line: u32 },
    /// Every breakpoint in one file.
    Source(PathBuf),
    All,
}

impl BreakpointSelector {
    /// The breakpoints this selector picks out, in the order they are held.
    pub fn pick(&self, breakpoints: &[Breakpoint]) -> Vec<Breakpoint> {
        breakpoints
            .iter()
            .filter(|breakpoint| self.matches(breakpoint))
            .cloned()
            .collect()
    }

    pub fn matches(&self, breakpoint: &Breakpoint) -> bool {
        match self {
            Self::Ids(ids) => ids.contains(&breakpoint.id),
            Self::Location { source, line } => {
                breakpoint.source == *source && breakpoint.line == *line
            }
            Self::Source(source) => breakpoint.source == *source,
            Self::All => true,
        }
    }

    /// How to describe the selection in an error or a table.
    pub fn describe(&self) -> String {
        match self {
            Self::Ids(ids) => {
                let ids: Vec<String> = ids.iter().map(BreakpointId::to_string).collect();
                format!("id {}", ids.join(", "))
            }
            Self::Location { source, line } => format!("{}:{line}", source.display()),
            Self::Source(source) => source.display().to_string(),
            Self::All => "all breakpoints".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn breakpoint(id: u32, source: &str, line: u32) -> Breakpoint {
        Breakpoint {
            id: BreakpointId(id),
            source: PathBuf::from(source),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }
    }

    fn fixture() -> Vec<Breakpoint> {
        vec![
            breakpoint(1, "/p/main.c", 10),
            breakpoint(2, "/p/main.c", 20),
            breakpoint(3, "/p/other.c", 10),
        ]
    }

    #[test]
    fn ids_pick_exactly_what_was_named() {
        let picked = BreakpointSelector::Ids(vec![BreakpointId(1), BreakpointId(3)]).pick(&fixture());
        let ids: Vec<u32> = picked.iter().map(|breakpoint| breakpoint.id.0).collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn an_id_that_does_not_exist_picks_nothing_rather_than_erroring_here() {
        // The caller compares what it asked for with what came back; the
        // selector's job is only to say what matched.
        let picked = BreakpointSelector::Ids(vec![BreakpointId(99)]).pick(&fixture());
        assert!(picked.is_empty());
    }

    #[test]
    fn a_location_needs_both_the_file_and_the_line_to_match() {
        let selector = BreakpointSelector::Location {
            source: PathBuf::from("/p/main.c"),
            line: 10,
        };
        let picked = selector.pick(&fixture());
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].id, BreakpointId(1));
    }

    #[test]
    fn a_source_picks_every_line_in_that_file_and_no_other() {
        let picked = BreakpointSelector::Source(PathBuf::from("/p/main.c")).pick(&fixture());
        assert_eq!(picked.len(), 2);
        assert!(picked.iter().all(|bp| bp.source == Path::new("/p/main.c")));
    }

    #[test]
    fn all_means_all() {
        assert_eq!(BreakpointSelector::All.pick(&fixture()).len(), 3);
    }

    #[test]
    fn a_selector_can_say_what_it_selected_for_a_message() {
        assert_eq!(
            BreakpointSelector::Ids(vec![BreakpointId(1), BreakpointId(2)]).describe(),
            "id 1, 2",
        );
        assert_eq!(BreakpointSelector::All.describe(), "all breakpoints");
    }
}
