//! Breakpoints: project state that outlives every session.
//!
//! Two things happen on every mutation. The store records it — so it survives
//! the daemon, the session, and the machine being rebooted — and, if a session
//! happens to be live, the adapter is told. Neither depends on the other: you
//! can set breakpoints before ever launching anything, and they will be there
//! when you do.
//!
//! Every mutation supports `--dry-run` (non-negotiable #4), and the preview is
//! not a separate code path: it calls `store.select` with the same selector
//! the mutation would, so the two cannot disagree about what is about to
//! happen.

use super::Result;
use crate::adapter::AdapterError;
use crate::state::DaemonState;
use lazydap_core::{Breakpoint, BreakpointId, BreakpointSelector, BreakpointStatus, NewBreakpoint};
use lazydap_protocol::{BreakpointAction, BreakpointReport, Response};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

pub fn list(state: &Arc<DaemonState>) -> Result<Response> {
    let breakpoints = state.store.breakpoints();
    Ok(Response::Breakpoints(BreakpointReport {
        action: BreakpointAction::Listed,
        dry_run: false,
        breakpoints: decorate(state, breakpoints),
        not_found: Vec::new(),
        applied_to_session: false,
    }))
}

pub async fn add(state: &Arc<DaemonState>, new: NewBreakpoint, dry_run: bool) -> Result<Response> {
    if dry_run {
        // Adding is the one mutation whose preview cannot come from the
        // selector: there is nothing to select yet. What it can do is answer
        // the question the caller is actually asking — is this a new
        // breakpoint, or one I already have?
        let existing = state.store.select(&BreakpointSelector::Location {
            source: new.source.clone(),
            line: new.line,
        });
        let preview = match existing.into_iter().next() {
            Some(existing) => existing,
            // Id `0` is deliberately not a real one: nothing has been
            // allocated, and printing the id the breakpoint *would* get would
            // be a promise this command has no way to keep.
            None => Breakpoint {
                id: BreakpointId(0),
                source: new.source,
                line: new.line,
                column: new.column,
                condition: new.condition,
                hit_condition: new.hit_condition,
                log_message: new.log_message,
                enabled: new.enabled,
            },
        };
        return Ok(Response::Breakpoints(BreakpointReport {
            action: BreakpointAction::Added,
            dry_run: true,
            breakpoints: decorate(state, vec![preview]),
            not_found: Vec::new(),
            applied_to_session: false,
        }));
    }

    let source = new.source.clone();
    let breakpoint = state.store.add(new);
    let applied = apply(state, &[source]).await?;

    Ok(Response::Breakpoints(BreakpointReport {
        action: BreakpointAction::Added,
        dry_run: false,
        breakpoints: decorate(state, vec![breakpoint]),
        not_found: Vec::new(),
        applied_to_session: applied,
    }))
}

pub async fn remove(
    state: &Arc<DaemonState>,
    selector: BreakpointSelector,
    dry_run: bool,
) -> Result<Response> {
    let picked = state.store.select(&selector);
    let not_found = missing(&selector, &picked);

    if dry_run {
        return Ok(Response::Breakpoints(BreakpointReport {
            action: BreakpointAction::Removed,
            dry_run: true,
            breakpoints: decorate(state, picked),
            not_found,
            applied_to_session: false,
        }));
    }

    let removed = state.store.remove(&selector);
    let applied = apply(state, &sources_of(&removed)).await?;

    Ok(Response::Breakpoints(BreakpointReport {
        action: BreakpointAction::Removed,
        dry_run: false,
        // Deliberately what was removed, not what is left: a caller that
        // piped ids in wants to know which of them went.
        breakpoints: removed
            .into_iter()
            .map(BreakpointStatus::unverified)
            .collect(),
        not_found,
        applied_to_session: applied,
    }))
}

pub async fn toggle(
    state: &Arc<DaemonState>,
    selector: BreakpointSelector,
    dry_run: bool,
) -> Result<Response> {
    let picked = state.store.select(&selector);
    let not_found = missing(&selector, &picked);

    if dry_run {
        // Show them as they *would* be, which is the only useful preview of a
        // toggle — echoing the current state would answer a question nobody
        // asked.
        let flipped: Vec<Breakpoint> = picked
            .into_iter()
            .map(|breakpoint| Breakpoint {
                enabled: !breakpoint.enabled,
                ..breakpoint
            })
            .collect();
        return Ok(Response::Breakpoints(BreakpointReport {
            action: BreakpointAction::Toggled,
            dry_run: true,
            breakpoints: decorate(state, flipped),
            not_found,
            applied_to_session: false,
        }));
    }

    let toggled = state.store.toggle(&selector);
    let applied = apply(state, &sources_of(&toggled)).await?;

    Ok(Response::Breakpoints(BreakpointReport {
        action: BreakpointAction::Toggled,
        dry_run: false,
        breakpoints: decorate(state, toggled),
        not_found,
        applied_to_session: applied,
    }))
}

/// Tell the live session about every breakpoint in each of `sources`.
///
/// The whole file each time, because `setBreakpoints` *replaces* a source's
/// list rather than adding to it: sending only what changed would silently
/// delete everything else in that file. Disabled breakpoints are left out —
/// that is what disabling one means.
///
/// Answers whether a session was told, so the caller can say "recorded, and
/// it will apply next launch" rather than implying it is live now.
async fn apply(state: &Arc<DaemonState>, sources: &[PathBuf]) -> Result<bool> {
    let Some(session) = state.active_session() else {
        return Ok(false);
    };
    if !session.state().is_live() || sources.is_empty() {
        return Ok(false);
    }

    for source in sources {
        let enabled: Vec<Breakpoint> = state
            .store
            .breakpoints_in(source)
            .into_iter()
            .filter(|breakpoint| breakpoint.enabled)
            .collect();

        let applied = session
            .adapter()
            .set_breakpoints(source, &enabled)
            .await
            .map_err(AdapterError::into_ipc)?;
        session.record_breakpoints(&applied);

        tracing::debug!(
            target: "daemon.session",
            session_id = %session.id,
            source = %source.display(),
            count = enabled.len(),
            "applied breakpoints",
        );
    }
    Ok(true)
}

/// Ids the selector named that matched nothing.
///
/// Only meaningful for an id selector: a file with no breakpoints in it is not
/// a mistake, but an id that no longer exists usually is — it means a script
/// is holding a stale one.
fn missing(selector: &BreakpointSelector, picked: &[Breakpoint]) -> Vec<BreakpointId> {
    let BreakpointSelector::Ids(asked) = selector else {
        return Vec::new();
    };
    let found: Vec<BreakpointId> = picked.iter().map(|breakpoint| breakpoint.id).collect();
    asked
        .iter()
        .filter(|id| !found.contains(id))
        .copied()
        .collect()
}

/// The distinct sources a set of breakpoints touches.
fn sources_of(breakpoints: &[Breakpoint]) -> Vec<PathBuf> {
    breakpoints
        .iter()
        .map(|breakpoint| breakpoint.source.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Dress persisted breakpoints in whatever the live session knows about them.
fn decorate(state: &Arc<DaemonState>, breakpoints: Vec<Breakpoint>) -> Vec<BreakpointStatus> {
    match state.active_session() {
        Some(session) => session.decorate(breakpoints),
        None => breakpoints
            .into_iter()
            .map(BreakpointStatus::unverified)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::tests::state;
    use lazydap_protocol::{Request, Response};

    fn new_breakpoint(source: &str, line: u32) -> NewBreakpoint {
        NewBreakpoint {
            source: PathBuf::from(source),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }
    }

    async fn report(state: &Arc<DaemonState>, request: Request) -> BreakpointReport {
        match crate::handlers::dispatch(state, request).await.expect("ok") {
            Response::Breakpoints(report) => report,
            other => unreachable!("expected a breakpoint report, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_breakpoint_set_without_a_session_is_recorded_for_the_next_one() {
        let state = state();
        let report = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 19),
                dry_run: false,
            },
        )
        .await;

        assert_eq!(report.action, BreakpointAction::Added);
        assert_eq!(report.breakpoints.len(), 1);
        assert!(
            !report.applied_to_session,
            "there is no session, and claiming otherwise would be a lie",
        );
        assert!(
            !report.breakpoints[0].verified,
            "nothing has checked that the line exists yet",
        );
    }

    #[tokio::test]
    async fn a_dry_run_add_records_nothing() {
        let state = state();
        let report = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 19),
                dry_run: true,
            },
        )
        .await;

        assert!(report.dry_run);
        assert!(
            state.store.breakpoints().is_empty(),
            "a preview that wrote to the state file would not be a preview",
        );
    }

    #[tokio::test]
    async fn a_dry_run_remove_previews_exactly_what_the_removal_takes() {
        let state = state();
        for line in [10, 20] {
            report(
                &state,
                Request::BreakpointAdd {
                    breakpoint: new_breakpoint("/p/main.c", line),
                    dry_run: false,
                },
            )
            .await;
        }
        report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/other.c", 1),
                dry_run: false,
            },
        )
        .await;

        let selector = BreakpointSelector::Source(PathBuf::from("/p/main.c"));
        let preview = report(
            &state,
            Request::BreakpointRemove {
                selector: selector.clone(),
                dry_run: true,
            },
        )
        .await;
        let real = report(
            &state,
            Request::BreakpointRemove {
                selector,
                dry_run: false,
            },
        )
        .await;

        let previewed: Vec<u32> = preview
            .breakpoints
            .iter()
            .map(|status| status.breakpoint.line)
            .collect();
        let removed: Vec<u32> = real
            .breakpoints
            .iter()
            .map(|status| status.breakpoint.line)
            .collect();

        assert_eq!(previewed, removed, "the preview promised something else");
        assert_eq!(state.store.breakpoints().len(), 1, "the other file stayed");
    }

    #[tokio::test]
    async fn removing_an_id_that_is_gone_says_which_one() {
        // What a `break --list --format ids | xargs` pipeline hits when the
        // list it captured has moved on.
        let state = state();
        let report = report(
            &state,
            Request::BreakpointRemove {
                selector: BreakpointSelector::Ids(vec![BreakpointId(42)]),
                dry_run: false,
            },
        )
        .await;

        assert_eq!(report.not_found, vec![BreakpointId(42)]);
        assert!(report.breakpoints.is_empty());
    }

    #[tokio::test]
    async fn a_dry_run_toggle_shows_the_state_it_would_leave_behind() {
        let state = state();
        let added = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 19),
                dry_run: false,
            },
        )
        .await;
        let id = added.breakpoints[0].breakpoint.id;

        let preview = report(
            &state,
            Request::BreakpointToggle {
                selector: BreakpointSelector::Ids(vec![id]),
                dry_run: true,
            },
        )
        .await;

        assert!(
            !preview.breakpoints[0].breakpoint.enabled,
            "a preview echoing the current state answers nothing",
        );
        assert!(
            state.store.breakpoints()[0].enabled,
            "and it must not have actually toggled anything",
        );
    }

    #[tokio::test]
    async fn setting_the_same_line_twice_does_not_make_two_breakpoints() {
        let state = state();
        for _ in 0..2 {
            report(
                &state,
                Request::BreakpointAdd {
                    breakpoint: new_breakpoint("/p/main.c", 19),
                    dry_run: false,
                },
            )
            .await;
        }
        assert_eq!(state.store.breakpoints().len(), 1);
    }

    #[tokio::test]
    async fn listing_reports_every_breakpoint_in_the_project() {
        let state = state();
        for line in [1, 2, 3] {
            report(
                &state,
                Request::BreakpointAdd {
                    breakpoint: new_breakpoint("/p/main.c", line),
                    dry_run: false,
                },
            )
            .await;
        }

        let listed = report(&state, Request::BreakpointList).await;
        assert_eq!(listed.action, BreakpointAction::Listed);
        assert_eq!(listed.breakpoints.len(), 3);
        assert!(!listed.dry_run);
    }
}
