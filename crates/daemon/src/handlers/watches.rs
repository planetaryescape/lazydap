//! Watches: project state that outlives every session.
//!
//! Simpler than [`super::breakpoints`] in one specific way, and it is worth
//! knowing why. A breakpoint has to be handed to a live adapter to do anything,
//! so every mutation there does two things — record it, and tell the session.
//! There is no DAP request that installs a watch: an expression is evaluated on
//! demand, at a stop, by whoever wants to know. So these handlers only ever
//! touch the store, and there is no `applied_to_session` to report.
//!
//! Everything else is the same discipline. Both mutations support `--dry-run`
//! (non-negotiable #4), and the preview is not a separate code path: it calls
//! `store.select_watches` with the same selector the mutation would, so the two
//! cannot disagree about what is about to happen. Both announce themselves, so
//! a TUI watching the stream learns about a `lazydap watch add` typed in
//! another terminal (D043's lesson).

use super::Result;
use crate::state::DaemonState;
use lazydap_core::{NewWatch, Watch, WatchId, WatchSelector};
use lazydap_protocol::{Event, Response, WatchAction, WatchReport};
use std::sync::Arc;

pub fn list(state: &Arc<DaemonState>) -> Result<Response> {
    Ok(Response::Watches(WatchReport {
        action: WatchAction::Listed,
        dry_run: false,
        watches: state.store.watches(),
        not_found: Vec::new(),
    }))
}

pub fn add(state: &Arc<DaemonState>, new: NewWatch, dry_run: bool) -> Result<Response> {
    if dry_run {
        // The preview cannot go through the selector: there is nothing to
        // select until the watch exists. It looks the expression up instead,
        // which is the same key `add` dedupes on, so a preview of an add that
        // would be a no-op correctly shows the watch already there.
        let existing = state
            .store
            .select_watches(&WatchSelector::Expression(new.expression.clone()));
        let previewed = existing.into_iter().next().unwrap_or(Watch {
            // Id `0` is deliberately not a real one: nothing has been
            // allocated, and printing the id this watch *would* get would be a
            // promise a preview has no way to keep.
            id: WatchId(0),
            expression: new.expression,
            label: new.label,
        });

        return Ok(Response::Watches(WatchReport {
            action: WatchAction::Added,
            dry_run: true,
            watches: vec![previewed],
            not_found: Vec::new(),
        }));
    }

    let watch = state.store.add_watch(new);
    announce(state, std::slice::from_ref(&watch));

    Ok(Response::Watches(WatchReport {
        action: WatchAction::Added,
        dry_run: false,
        watches: vec![watch],
        not_found: Vec::new(),
    }))
}

pub fn remove(state: &Arc<DaemonState>, selector: WatchSelector, dry_run: bool) -> Result<Response> {
    let picked = state.store.select_watches(&selector);
    let not_found = missing(&selector, &picked);

    if dry_run {
        return Ok(Response::Watches(WatchReport {
            action: WatchAction::Removed,
            dry_run: true,
            watches: picked,
            not_found,
        }));
    }

    let removed = state.store.remove_watches(&selector);
    announce(state, &removed);

    Ok(Response::Watches(WatchReport {
        action: WatchAction::Removed,
        dry_run: false,
        // Deliberately what was removed, not what is left: a caller that piped
        // ids in wants to know which of them went.
        watches: removed,
        not_found,
    }))
}

/// Tell every subscriber the project's watch list changed.
///
/// Without this a TUI's watches pane would go on drawing the previous list
/// indefinitely after a `lazydap watch add` in another terminal — which is
/// exactly the bug D043 found in the breakpoint gutter, avoided here by
/// announcing from the start rather than after somebody hit it.
fn announce(state: &Arc<DaemonState>, changed: &[Watch]) {
    for watch in changed {
        state.emit_project(Event::WatchUpdated { watch_id: watch.id });
    }
}

/// Which of the ids a selector named matched nothing.
///
/// Only meaningful for an id selector: every other kind describes a set, and a
/// set that turns out to be empty is an answer rather than a mistake.
fn missing(selector: &WatchSelector, picked: &[Watch]) -> Vec<WatchId> {
    let WatchSelector::Ids(asked) = selector else {
        return Vec::new();
    };
    let found: Vec<WatchId> = picked.iter().map(|watch| watch.id).collect();
    asked
        .iter()
        .filter(|id| !found.contains(id))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::tests::state;
    use lazydap_protocol::{Request, Response};

    fn new_watch(expression: &str) -> NewWatch {
        NewWatch {
            expression: expression.to_string(),
            label: None,
        }
    }

    async fn report(state: &Arc<DaemonState>, request: Request) -> WatchReport {
        match crate::handlers::dispatch(state, request).await.expect("ok") {
            Response::Watches(report) => report,
            other => unreachable!("expected a watch report, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_watch_set_without_a_session_is_recorded_for_the_next_one() {
        // The whole point of watches being project state: you can set them
        // before there is anything to evaluate them against.
        let state = state();
        let report = report(
            &state,
            Request::WatchAdd {
                watch: new_watch("tokens[pos]"),
                dry_run: false,
            },
        )
        .await;

        assert_eq!(report.action, WatchAction::Added);
        assert!(!report.dry_run);
        assert_eq!(report.watches.len(), 1);
        assert_eq!(state.store.watches().len(), 1);
    }

    #[tokio::test]
    async fn a_dry_run_add_records_nothing() {
        let state = state();
        let report = report(
            &state,
            Request::WatchAdd {
                watch: new_watch("counter"),
                dry_run: true,
            },
        )
        .await;

        assert!(report.dry_run);
        assert_eq!(report.watches[0].expression, "counter");
        assert_eq!(
            report.watches[0].id,
            WatchId(0),
            "a preview does not promise an id it has not allocated",
        );
        assert!(
            state.store.watches().is_empty(),
            "a preview changes nothing",
        );
    }

    #[tokio::test]
    async fn a_dry_run_remove_previews_exactly_what_the_removal_takes() {
        let state = state();
        report(
            &state,
            Request::WatchAdd {
                watch: new_watch("a"),
                dry_run: false,
            },
        )
        .await;
        report(
            &state,
            Request::WatchAdd {
                watch: new_watch("b"),
                dry_run: false,
            },
        )
        .await;

        let selector = WatchSelector::Expression("b".to_string());
        let preview = report(
            &state,
            Request::WatchRemove {
                selector: selector.clone(),
                dry_run: true,
            },
        )
        .await;
        assert_eq!(state.store.watches().len(), 2, "a preview changes nothing");

        let real = report(
            &state,
            Request::WatchRemove {
                selector,
                dry_run: false,
            },
        )
        .await;

        assert_eq!(preview.watches, real.watches);
        assert_eq!(state.store.watches().len(), 1);
    }

    #[tokio::test]
    async fn removing_an_id_that_is_gone_says_which_one() {
        let state = state();
        let report = report(
            &state,
            Request::WatchRemove {
                selector: WatchSelector::Ids(vec![WatchId(9)]),
                dry_run: false,
            },
        )
        .await;

        assert_eq!(report.not_found, vec![WatchId(9)]);
        assert!(report.watches.is_empty());
    }

    #[tokio::test]
    async fn adding_the_same_expression_twice_does_not_make_two_watches() {
        let state = state();
        for _ in 0..2 {
            report(
                &state,
                Request::WatchAdd {
                    watch: new_watch("counter"),
                    dry_run: false,
                },
            )
            .await;
        }

        assert_eq!(state.store.watches().len(), 1);
    }

    #[tokio::test]
    async fn listing_reports_every_watch_in_the_project() {
        let state = state();
        for expression in ["a", "b"] {
            report(
                &state,
                Request::WatchAdd {
                    watch: new_watch(expression),
                    dry_run: false,
                },
            )
            .await;
        }

        let report = report(&state, Request::WatchList).await;
        assert_eq!(report.action, WatchAction::Listed);
        assert_eq!(report.watches.len(), 2);
    }

    #[tokio::test]
    async fn a_mutation_announces_itself_so_an_open_tui_can_read_the_list_again() {
        let state = state();
        let mut events = state.events().subscribe();

        report(
            &state,
            Request::WatchAdd {
                watch: new_watch("counter"),
                dry_run: false,
            },
        )
        .await;

        let sequenced = events.try_recv().expect("an announcement");
        match sequenced.event {
            Event::WatchUpdated { watch_id } => assert_eq!(watch_id, WatchId(1)),
            other => unreachable!("expected a watch update, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_dry_run_announces_nothing_because_nothing_changed() {
        let state = state();
        let mut events = state.events().subscribe();

        report(
            &state,
            Request::WatchAdd {
                watch: new_watch("counter"),
                dry_run: true,
            },
        )
        .await;

        assert!(
            events.try_recv().is_err(),
            "a preview must not announce a change it did not make",
        );
    }
}
