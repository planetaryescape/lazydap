//! `lazydap watch` — add, list and remove watch expressions.
//!
//! The CLI half of M16. Every watch the TUI's pane can set, this can set, and
//! through the same requests (non-negotiable #2) — which is also why the pane
//! could be built at all: a TUI-only feature is one the crate boundary forbids.

use super::unexpected;
use crate::auto_spawn::ensure_daemon_running;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use crate::output::{OutputFormat, Row, View, or_dash};
use lazydap_core::{NewWatch, WatchId, WatchSelector};
use lazydap_protocol::{Request, Response, WatchAction, WatchReport};

pub async fn add(
    instance: &Instance,
    expression: String,
    label: Option<String>,
    dry_run: bool,
    format: OutputFormat,
) -> Result<()> {
    send(
        instance,
        Request::WatchAdd {
            watch: NewWatch { expression, label },
            dry_run,
        },
        format,
    )
    .await
}

pub async fn list(instance: &Instance, format: OutputFormat) -> Result<()> {
    send(instance, Request::WatchList, format).await
}

pub async fn remove(
    instance: &Instance,
    expression: Option<String>,
    ids: Vec<u32>,
    all: bool,
    dry_run: bool,
    format: OutputFormat,
) -> Result<()> {
    // Worked out *before* starting a daemon: a usage mistake should not leave a
    // background process behind it.
    let selector = selector(expression, ids, all)?;
    send(instance, Request::WatchRemove { selector, dry_run }, format).await
}

/// Which watches a `remove` is talking about.
///
/// Refuses an ambiguous selection rather than picking one, for the reason
/// `break` does: the two readings of `watch remove x --all` delete different
/// things, and guessing wrong deletes the wrong ones.
fn selector(expression: Option<String>, ids: Vec<u32>, all: bool) -> Result<WatchSelector> {
    match (expression, ids.is_empty(), all) {
        (Some(_), false, _) | (Some(_), _, true) | (None, false, true) => Err(CliError::usage(
            "say which watches once: an expression, --id, or --all",
        )),
        (Some(expression), true, false) => Ok(WatchSelector::Expression(expression)),
        (None, false, false) => Ok(WatchSelector::Ids(ids.into_iter().map(WatchId).collect())),
        (None, true, true) => Ok(WatchSelector::All),
        (None, true, false) => Err(CliError::usage(
            "nothing selected: name an expression, or pass --id or --all",
        )),
    }
}

async fn send(instance: &Instance, request: Request, format: OutputFormat) -> Result<()> {
    let mut client = ensure_daemon_running(instance).await?;
    let response = client.request(request).await?;
    let Response::Watches(report) = response else {
        return Err(unexpected(response));
    };

    view(&report).print(format)
}

fn view(report: &WatchReport) -> View {
    let rows: Vec<Row> = report
        .watches
        .iter()
        .map(|watch| {
            Row::new(
                watch.id.to_string(),
                vec![
                    watch.id.to_string(),
                    watch.expression.clone(),
                    or_dash(watch.label.as_ref()),
                ],
                watch,
            )
        })
        .collect();

    let json = serde_json::json!({
        "action": report.action,
        "dry_run": report.dry_run,
        "watches": report.watches,
        "not_found": report.not_found,
    });

    let mut view = View::list(json, &["id", "expression", "label"], rows);
    if let Some(note) = note(report) {
        view = view.with_note(note);
    }
    view
}

/// The line a person needs that the table itself does not say.
fn note(report: &WatchReport) -> Option<String> {
    let mut notes = Vec::new();

    if report.dry_run {
        notes.push(format!(
            "dry run: {} {} watch(es), nothing changed",
            report.action.would(),
            report.watches.len(),
        ));
    }
    if !report.not_found.is_empty() {
        notes.push(format!(
            "no watch with id {}",
            report
                .not_found
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    // Worth saying once, when a watch is first set: nothing about a watch is
    // visible until the program stops, and silence after `watch add` otherwise
    // reads as the command having done nothing.
    if !report.dry_run && report.action == WatchAction::Added {
        notes.push("evaluated at every stop; `lazydap eval` reads it now".to_string());
    }

    (!notes.is_empty()).then(|| notes.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::Watch;

    fn watch(id: u32, expression: &str) -> Watch {
        Watch {
            id: WatchId(id),
            expression: expression.to_string(),
            label: None,
        }
    }

    #[test]
    fn an_expression_selects_the_watch_somebody_can_see() {
        let selected = selector(Some("counter".to_string()), Vec::new(), false).expect("select");
        assert_eq!(selected, WatchSelector::Expression("counter".to_string()));
    }

    #[test]
    fn ids_select_what_a_previous_ids_format_run_printed() {
        let selected = selector(None, vec![1, 3], false).expect("select");
        assert_eq!(
            selected,
            WatchSelector::Ids(vec![WatchId(1), WatchId(3)]),
            "so `lazydap watch list --format ids | xargs ...` composes",
        );
    }

    #[test]
    fn removing_everything_and_removing_one_thing_cannot_be_asked_for_together() {
        // The two readings delete different watches, and guessing wrong
        // deletes the wrong ones.
        let error = selector(Some("x".to_string()), Vec::new(), true).expect_err("ambiguous");
        assert!(error.to_string().contains("once"), "got: {error}");

        let error = selector(None, vec![1], true).expect_err("ambiguous");
        assert!(error.to_string().contains("once"), "got: {error}");
    }

    #[test]
    fn a_removal_that_selects_nothing_is_a_usage_error_rather_than_a_silent_no_op() {
        let error = selector(None, Vec::new(), false).expect_err("nothing selected");
        assert!(
            error.to_string().contains("nothing selected"),
            "got: {error}"
        );
    }

    #[test]
    fn a_dry_run_report_says_what_it_would_have_done() {
        let note = note(&WatchReport {
            action: WatchAction::Removed,
            dry_run: true,
            watches: vec![watch(1, "counter")],
            not_found: Vec::new(),
        })
        .expect("a note");

        assert!(note.contains("would remove 1 watch"), "got: {note}");
        assert!(note.contains("nothing changed"), "got: {note}");
    }

    #[test]
    fn an_id_that_matched_nothing_is_named_so_a_piped_id_can_be_seen_to_be_stale() {
        let note = note(&WatchReport {
            action: WatchAction::Removed,
            dry_run: false,
            watches: Vec::new(),
            not_found: vec![WatchId(9)],
        })
        .expect("a note");

        assert!(note.contains("no watch with id 9"), "got: {note}");
    }

    #[test]
    fn a_plain_list_says_nothing_extra() {
        assert_eq!(
            note(&WatchReport {
                action: WatchAction::Listed,
                dry_run: false,
                watches: vec![watch(1, "counter")],
                not_found: Vec::new(),
            }),
            None,
            "a list has nothing to add that the table does not say",
        );
    }

    #[test]
    fn a_watch_report_renders_in_the_list_shape_every_format_needs() {
        // `View::single` would make `--format ids`, `jsonl` and `csv` a usage
        // error, which is what breaks `lazydap watch list --format ids | xargs`.
        // The round trip itself is proved end to end in `tests/cli_watches.rs`.
        let report = WatchReport {
            action: WatchAction::Listed,
            dry_run: false,
            watches: vec![watch(1, "counter"), watch(4, "tokens[pos]")],
            not_found: Vec::new(),
        };

        for format in [OutputFormat::Ids, OutputFormat::Jsonl, OutputFormat::Csv] {
            view(&report)
                .print(format)
                .unwrap_or_else(|error| unreachable!("{format:?} should render: {error}"));
        }
    }
}
