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
//! the mutation would, or `store.preview_add` with the same rule `store.add`
//! follows, so the two cannot disagree about what is about to happen.
//!
//! The order of those two things is fixed: **store, announce, then adapter.**
//! The store is what the project actually holds, so a subscriber has to hear
//! about the change whether or not the adapter accepts it — see
//! [`recorded_anyway`].

use super::Result;
use crate::adapter::AdapterError;
use crate::state::DaemonState;
use lazydap_core::{
    AdapterBreakpoint, Breakpoint, BreakpointSelector, BreakpointStatus, NewBreakpoint,
};
use lazydap_protocol::{BreakpointAction, BreakpointReport, Event, IpcError, Response};
use lazydap_store::AddOutcome;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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
    let new = NewBreakpoint {
        source: canonical(&new.source),
        ..new
    };

    if dry_run {
        // Adding is the one mutation whose preview cannot come from a
        // selector: there is nothing to select yet. `preview_add` answers the
        // question the caller is actually asking — would this make a
        // breakpoint, change one, or do nothing? — from the same rule the
        // mutation follows.
        let preview = state.store.preview_add(new);
        return Ok(Response::Breakpoints(BreakpointReport {
            action: action_for(preview.outcome),
            dry_run: true,
            breakpoints: decorate(state, vec![preview.breakpoint]),
            not_found: Vec::new(),
            applied_to_session: false,
        }));
    }

    let source = new.source.clone();
    let added = state.store.add(new);
    let changed = added.outcome != AddOutcome::Unchanged;
    // Nothing to announce yet when nothing moved: a subscriber redrawing on a
    // no-op learns nothing, and would be told a change happened that did not.
    if changed {
        announce(state, std::slice::from_ref(&added.breakpoint));
    }
    // Sent even when unchanged, so that re-running the command is how you
    // retry a breakpoint the adapter refused the first time.
    let applied = apply(state, &[source])
        .await
        .map_err(|error| recorded_anyway(error, changed.then_some(&added.breakpoint)))?;
    // A retry that did reach the adapter is news after all: the adapter can
    // have verified the breakpoint at last, or moved it to the next line with
    // code, and `record_breakpoints` emits nothing of its own. The project
    // event says "the list is not what you last read", which a late bind makes
    // true.
    if !changed && applied {
        announce(state, std::slice::from_ref(&added.breakpoint));
    }

    Ok(Response::Breakpoints(BreakpointReport {
        action: action_for(added.outcome),
        dry_run: false,
        breakpoints: decorate(state, vec![added.breakpoint]),
        not_found: Vec::new(),
        applied_to_session: applied,
    }))
}

/// How a store outcome is reported on the wire.
fn action_for(outcome: AddOutcome) -> BreakpointAction {
    match outcome {
        AddOutcome::Created => BreakpointAction::Added,
        AddOutcome::Updated => BreakpointAction::Updated,
        AddOutcome::Unchanged => BreakpointAction::Unchanged,
    }
}

pub async fn remove(
    state: &Arc<DaemonState>,
    selector: BreakpointSelector,
    dry_run: bool,
) -> Result<Response> {
    let selector = canonical_selector(selector);

    if dry_run {
        let (picked, not_found) = state.store.select(&selector);
        return Ok(Response::Breakpoints(BreakpointReport {
            action: BreakpointAction::Removed,
            dry_run: true,
            breakpoints: decorate(state, picked),
            not_found,
            applied_to_session: false,
        }));
    }

    // Selection and mutation under one lock, with `not_found` derived from
    // what this call actually removed. Selecting first and mutating after let
    // two clients removing the same id both see it there: the winner removed
    // it, and the loser removed nothing while reporting an empty `not_found` —
    // success, for work it did not do.
    let removed = state.store.remove(&selector);
    announce(state, &removed.breakpoints);
    let applied = apply(state, &sources_of(&removed.breakpoints))
        .await
        .map_err(|error| recorded_anyway(error, &removed.breakpoints))?;

    Ok(Response::Breakpoints(BreakpointReport {
        action: BreakpointAction::Removed,
        dry_run: false,
        // Deliberately what was removed, not what is left: a caller that
        // piped ids in wants to know which of them went.
        breakpoints: removed
            .breakpoints
            .into_iter()
            .map(BreakpointStatus::unverified)
            .collect(),
        not_found: removed.not_found,
        applied_to_session: applied,
    }))
}

pub async fn toggle(
    state: &Arc<DaemonState>,
    selector: BreakpointSelector,
    dry_run: bool,
) -> Result<Response> {
    let selector = canonical_selector(selector);

    if dry_run {
        // Show them as they *would* be, which is the only useful preview of a
        // toggle — echoing the current state would answer a question nobody
        // asked.
        let (picked, not_found) = state.store.select(&selector);
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

    // Under one lock, and `not_found` from what was actually flipped — see
    // [`remove`].
    let toggled = state.store.toggle(&selector);
    announce(state, &toggled.breakpoints);
    let applied = apply(state, &sources_of(&toggled.breakpoints))
        .await
        .map_err(|error| recorded_anyway(error, &toggled.breakpoints))?;

    Ok(Response::Breakpoints(BreakpointReport {
        action: BreakpointAction::Toggled,
        dry_run: false,
        breakpoints: decorate(state, toggled.breakpoints),
        not_found: toggled.not_found,
        applied_to_session: applied,
    }))
}

/// The spelling of a source path the daemon stores and selects on.
///
/// A file has more than one true name — `/tmp/x.c` and `/private/tmp/x.c` are
/// the same file on macOS — and two spellings of one line are two breakpoints
/// in a list that only ever compares paths for equality. The CLI resolves its
/// own paths before it sends them (`commands::resolve_source`) and the TUI does
/// too (D097), but that is each client's courtesy; the daemon is the only place
/// that can make it the project's rule, and a client that skips it should not
/// end up with a second breakpoint on a line the CLI already has one on
/// (D-WP9-1).
///
/// A path that will not canonicalise is kept exactly as it arrived. That is
/// almost always a file which is not there yet — generated, or on a branch not
/// checked out — and a breakpoint waiting for it is a reasonable thing to hold.
/// Refusing it here would turn a breakpoint the store persisted quite happily
/// into an error on the next `toggle`. The CLI still refuses a missing file at
/// the point where the user typed it, which is where the better message is.
fn canonical(source: &Path) -> PathBuf {
    source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf())
}

/// The same, for the selectors that name a file.
fn canonical_selector(selector: BreakpointSelector) -> BreakpointSelector {
    match selector {
        BreakpointSelector::Location { source, line } => BreakpointSelector::Location {
            source: canonical(&source),
            line,
        },
        BreakpointSelector::Source(source) => BreakpointSelector::Source(canonical(&source)),
        other => other,
    }
}

/// Tell every subscriber that the project's breakpoints are not what they last
/// read.
///
/// On *every* mutation, not only the ones no session saw. A live session's
/// adapter reports its opinion of a breakpoint that was added, but nothing
/// reports one that was removed — an adapter is told the new list for a file
/// and simply says nothing about what is no longer in it. So a client watching
/// only the adapter's events keeps drawing a breakpoint that is gone, and a
/// client watching between sessions sees nothing at all.
///
/// Scoped to the project (`session_id: None`), because that is what it is: the
/// verification fields carry no opinion and a client that applied them would be
/// inventing a claim nobody made. See [`Event::BreakpointUpdated`].
fn announce(state: &Arc<DaemonState>, changed: &[Breakpoint]) {
    for breakpoint in changed {
        state.emit_project(Event::BreakpointUpdated {
            session_id: None,
            breakpoint: AdapterBreakpoint {
                id: Some(breakpoint.id),
                adapter_id: None,
                verified: false,
                line: Some(breakpoint.line),
                message: None,
            },
        });
    }
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

        // What the adapter made of them is deliberately not recorded here: the
        // pump has already recorded this same answer as it went past, which is
        // what stops a `breakpoint` event arriving right behind it being
        // reported under an id nobody can match (D099). Recording it again
        // would be a whole-entry overwrite of a map that event has since
        // refreshed — putting the adapter's first word back over its second.
        session
            .adapter()
            .set_breakpoints(source, &enabled)
            .await
            .map_err(AdapterError::into_ipc)?;

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

/// Say that the project's list changed even though the adapter would not take
/// it.
///
/// **The store is deliberately not rolled back.** A breakpoint list belongs to
/// the project, not to whichever session happens to be running (D006), and an
/// adapter that refuses `setBreakpoints` — usually because it has just died —
/// has not made the user's intent untrue: the list is re-sent in full at the
/// next launch. Undoing the edit would lose it for a failure that is about the
/// session.
///
/// What that leaves is a caller holding an error for a change that did happen,
/// which is why the announcement goes out *before* the adapter is told (so a
/// subscriber is never left drawing the old list — the D043 bug) and why the
/// error names what was recorded rather than reading as "nothing happened".
fn recorded_anyway<'a>(
    mut error: IpcError,
    changed: impl IntoIterator<Item = &'a Breakpoint>,
) -> IpcError {
    let ids: Vec<u32> = changed
        .into_iter()
        .map(|breakpoint| breakpoint.id.0)
        .collect();
    // A command that wrote nothing — a re-set of a location that already said
    // exactly this — has nothing to claim beyond the adapter's own failure.
    if ids.is_empty() {
        return error;
    }

    let mut details = match std::mem::take(&mut error.details) {
        serde_json::Value::Object(details) => details,
        // Nothing to preserve: a non-object `details` has no keys, and these
        // two are what a caller needs here.
        _ => serde_json::Map::new(),
    };
    details.insert(
        "recorded_breakpoint_ids".to_string(),
        serde_json::json!(ids),
    );
    details.insert("applied_to_session".to_string(), serde_json::json!(false));
    error.details = serde_json::Value::Object(details);

    error.message = format!(
        "{} — the change to the project's breakpoints is recorded and will \
         apply to the next `lazydap launch`",
        error.message,
    );
    error
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
    use crate::adapter::AdapterHandle;
    use crate::handlers::tests::state;
    use crate::state::Session;
    use lazydap_core::{AdapterKind, BreakpointId, SessionId, SessionState};
    use lazydap_protocol::{ErrorCode, Request, Response};
    use std::sync::atomic::AtomicU64;

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
        match crate::handlers::dispatch(state, request, None)
            .await
            .expect("ok")
        {
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
    async fn two_spellings_of_one_file_are_one_breakpoint() {
        // The CLI canonicalises before it sends and the TUI does too, so a
        // client that does neither is the case this covers: without the
        // daemon canonicalising, `/tmp/…/main.c` and `/private/tmp/…/main.c`
        // are two entries on one line of one file, and removing "the"
        // breakpoint leaves the other behind.
        //
        // The symlink is built here rather than borrowed from macOS's
        // `/tmp` → `/private/tmp`, which Linux does not have.
        let state = state();
        let real = std::env::temp_dir().join(format!("lazydap-canon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&real);
        std::fs::create_dir_all(&real).expect("create the real directory");
        std::fs::write(real.join("main.c"), "int main(void) { return 0; }\n").expect("write");

        let linked = real.with_extension("link");
        let _ = std::fs::remove_file(&linked);
        std::os::unix::fs::symlink(&real, &linked).expect("symlink the directory");

        for source in [real.join("main.c"), linked.join("main.c")] {
            let source = source.to_str().expect("a utf-8 temporary directory");
            report(
                &state,
                Request::BreakpointAdd {
                    breakpoint: new_breakpoint(source, 19),
                    dry_run: false,
                },
            )
            .await;
        }

        let breakpoints = state.store.breakpoints();
        assert_eq!(breakpoints.len(), 1, "got: {breakpoints:?}");
        assert_eq!(
            breakpoints[0].source,
            real.join("main.c").canonicalize().expect("canonicalise"),
        );

        // And the selectors agree with the spelling the store settled on, so
        // the same client can take its breakpoint back off again.
        let removed = report(
            &state,
            Request::BreakpointRemove {
                selector: BreakpointSelector::Location {
                    source: linked.join("main.c"),
                    line: 19,
                },
                dry_run: false,
            },
        )
        .await;
        assert_eq!(removed.breakpoints.len(), 1, "got: {removed:?}");
        assert!(state.store.breakpoints().is_empty());

        let _ = std::fs::remove_file(&linked);
        let _ = std::fs::remove_dir_all(&real);
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
    async fn a_re_set_that_changed_nothing_does_not_claim_to_have_recorded_anything() {
        // The adapter failure is real; the claim that a change went with it
        // would not be. Nothing was written — the location already said
        // exactly this.
        let state = state();
        report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 19),
                dry_run: false,
            },
        )
        .await;
        live_session_with_a_dead_adapter(&state);

        let error = crate::handlers::dispatch(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 19),
                dry_run: false,
            },
            None,
        )
        .await
        .expect_err("the adapter is gone, so re-applying it cannot succeed");

        assert_eq!(error.code, ErrorCode::AdapterCrashed, "got: {error}");
        assert!(
            !error.message.contains("recorded"),
            "nothing was written, so nothing was recorded: {error}",
        );
        assert!(
            error.details.get("recorded_breakpoint_ids").is_none(),
            "got: {error}",
        );
    }

    #[tokio::test]
    async fn two_removals_of_the_same_id_do_not_both_report_success() {
        // What two clients racing on one id reach. The loser used to answer
        // with an empty `breakpoints` *and* an empty `not_found` under exit 0
        // — success, for work it did not do — because `not_found` came from a
        // selection made before the lock the removal took.
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
        let remove = || Request::BreakpointRemove {
            selector: BreakpointSelector::Ids(vec![id]),
            dry_run: false,
        };

        let winner = report(&state, remove()).await;
        let loser = report(&state, remove()).await;

        assert_eq!(winner.breakpoints.len(), 1);
        assert!(winner.not_found.is_empty());
        assert!(loser.breakpoints.is_empty(), "it removed nothing");
        assert_eq!(loser.not_found, vec![id], "and has to say so");
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

    /// A live session whose adapter has already gone — every request to it
    /// fails, which is the shape of an adapter refusing `setBreakpoints`.
    fn live_session_with_a_dead_adapter(state: &Arc<DaemonState>) {
        let session_id = SessionId::new();
        let reservation = state.reserve(session_id).expect("the slot is free");
        reservation.promote(Arc::new(Session::new(
            session_id,
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            AdapterHandle::detached(),
            state.events(),
            Arc::new(AtomicU64::new(0)),
        )));
    }

    #[tokio::test]
    async fn re_setting_a_location_with_a_condition_updates_it_and_keeps_its_id() {
        let state = state();
        let added = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 10),
                dry_run: false,
            },
        )
        .await;
        assert_eq!(added.action, BreakpointAction::Added);

        let conditional = NewBreakpoint {
            condition: Some("i > 5".to_string()),
            enabled: false,
            ..new_breakpoint("/p/main.c", 10)
        };
        let preview = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: conditional.clone(),
                dry_run: true,
            },
        )
        .await;
        let updated = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: conditional,
                dry_run: false,
            },
        )
        .await;

        assert_eq!(
            preview.action,
            BreakpointAction::Updated,
            "the preview must not promise an add it will not make",
        );
        assert_eq!(updated.action, BreakpointAction::Updated);
        assert_eq!(
            updated.breakpoints[0].breakpoint.id, added.breakpoints[0].breakpoint.id,
            "a re-set location is the same breakpoint, not a second one",
        );
        assert_eq!(
            updated.breakpoints[0].breakpoint.condition.as_deref(),
            Some("i > 5"),
            "the modifiers used to be dropped silently",
        );
        assert!(!updated.breakpoints[0].breakpoint.enabled);
    }

    #[tokio::test]
    async fn setting_a_location_to_what_it_already_says_reports_no_change() {
        let state = state();
        for _ in 0..2 {
            report(
                &state,
                Request::BreakpointAdd {
                    breakpoint: new_breakpoint("/p/main.c", 10),
                    dry_run: false,
                },
            )
            .await;
        }
        let again = report(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 10),
                dry_run: false,
            },
        )
        .await;

        assert_eq!(again.action, BreakpointAction::Unchanged);
    }

    #[tokio::test]
    async fn a_rejected_set_breakpoints_is_reported_and_announced() {
        // The store and the adapter are not one transaction: the breakpoint
        // list belongs to the project and survives the session that would not
        // take it. What must not survive is a subscriber drawing the old list
        // (D043), so the change is announced before the adapter is told.
        let state = state();
        live_session_with_a_dead_adapter(&state);
        let mut events = state.events().subscribe();

        let error = crate::handlers::dispatch(
            &state,
            Request::BreakpointAdd {
                breakpoint: new_breakpoint("/p/main.c", 19),
                dry_run: false,
            },
            None,
        )
        .await
        .expect_err("the adapter is gone, so applying it cannot succeed");

        assert_eq!(error.code, ErrorCode::AdapterCrashed, "got: {error}");
        assert!(
            error.message.contains("recorded"),
            "an error reading as `nothing happened` would be a lie: {error}",
        );
        assert_eq!(error.details["recorded_breakpoint_ids"][0], 1);
        assert_eq!(error.details["applied_to_session"], false);

        assert_eq!(
            state.store.breakpoints().len(),
            1,
            "the project keeps what the user asked for; it applies at the next launch",
        );
        let announced = std::iter::from_fn(|| events.try_recv().ok())
            .any(|seq| matches!(seq.event, Event::BreakpointUpdated { .. }));
        assert!(
            announced,
            "without the announcement a TUI goes on drawing the old list",
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
