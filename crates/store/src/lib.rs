//! Per-project state: `.lazydap/state.toml`.
//!
//! TOML rather than a database (D006). The file is small, human-readable, and
//! meant to be committable — a team can share breakpoints by checking it in,
//! or ignore it and not. That makes two things load-bearing here:
//!
//! - **Paths are stored relative to the project root** when they are inside
//!   it, so the file means the same thing on another machine. In memory they
//!   are always absolute, because that is what an adapter needs.
//! - **Writes are debounced and atomic** (500ms, write-then-rename). Setting
//!   twenty breakpoints in a script should cost one write, and a crash
//!   mid-write must not leave half a file where the state used to be.
//!
//! # One writer per project, assumed
//!
//! This store assumes it is the only process writing its file. That normally
//! holds by construction: a daemon instance is keyed to a project root (D010,
//! D024), so one project means one daemon means one writer.
//!
//! It is an assumption rather than a guarantee. `LAZYDAP_INSTANCE` and
//! `--instance` override the instance name, so two daemons under different
//! names can be started in the same directory and both claim the same
//! `.lazydap/state.toml`. Nothing here stops them.
//!
//! What is defended today: each write goes to a temporary file named per
//! process before being renamed into place, so concurrent writers cannot
//! interleave bytes — the loser's update is lost, but the file is never
//! corrupt. External edits are noticed by comparing the file's bytes with the
//! ones we last wrote, and merged on the way past.
//!
//! What is not: there is no interprocess lock, so a lost update is possible.
//! Recorded as a follow-up on M6 rather than built now — file locking that is
//! correct across platforms and network filesystems is a real piece of work,
//! and the failure it prevents needs a deliberately unusual setup to reach.

mod file;

use lazydap_core::{
    Breakpoint, BreakpointId, BreakpointSelector, LaunchConfig, NewBreakpoint, NewWatch, Watch,
    WatchId, WatchSelector,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;

/// How long to wait for a burst of mutations to finish before writing.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// The directory lazydap keeps per-project state in.
pub const STATE_DIR: &str = ".lazydap";
/// The file inside it.
pub const STATE_FILE: &str = "state.toml";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{path} is not valid lazydap state: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("cannot serialise the project state: {0}")]
    Serialise(#[from] toml::ser::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// What a watch removal did: the ones that went, and the ids that matched
/// nothing.
///
/// One value rather than two calls, because the two have to be decided under
/// the same lock — see [`ProjectStore::remove_watches`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub watches: Vec<Watch>,
    pub not_found: Vec<WatchId>,
}

/// What a breakpoint removal or toggle did: the ones it changed, and the ids
/// that matched nothing.
///
/// One value rather than two calls, for the same reason as [`Removed`] — the
/// two have to be decided under the same lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub breakpoints: Vec<Breakpoint>,
    pub not_found: Vec<BreakpointId>,
}

/// What [`ProjectStore::add`] did with a location, and the breakpoint that is
/// there now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Addition {
    pub breakpoint: Breakpoint,
    pub outcome: AddOutcome,
}

/// The three things setting a location can mean.
///
/// Distinguished because "added" was reported for all three, which made
/// `lazydap break x.c:5 --condition 'i > 5'` on a line that already had a
/// breakpoint look like it had taken when the modifiers were being dropped
/// (D-WP4-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// Nothing was there; a new id was minted.
    Created,
    /// One was there and its modifiers now differ.
    Updated,
    /// One was there and the request asked for exactly what it already says.
    Unchanged,
}

/// What setting `new` at its location means, given whatever is already there.
///
/// The one place that decision is made, so [`ProjectStore::add`] and
/// [`ProjectStore::preview_add`] cannot disagree about it (non-negotiable #4).
/// `new_id` is used only when there is nothing there to keep the id of.
fn decide(existing: Option<&Breakpoint>, new_id: BreakpointId, new: NewBreakpoint) -> Addition {
    let breakpoint = Breakpoint {
        id: existing.map_or(new_id, |breakpoint| breakpoint.id),
        source: new.source,
        line: new.line,
        column: new.column,
        condition: new.condition,
        hit_condition: new.hit_condition,
        log_message: new.log_message,
        enabled: new.enabled,
    };
    let outcome = match existing {
        None => AddOutcome::Created,
        Some(existing) if *existing == breakpoint => AddOutcome::Unchanged,
        Some(_) => AddOutcome::Updated,
    };
    Addition {
        breakpoint,
        outcome,
    }
}

/// Which of the breakpoint ids a selector named matched nothing. The
/// [`unmatched`] rule, for the other kind of selector.
fn unmatched_breakpoints(
    selector: &BreakpointSelector,
    picked: &[Breakpoint],
) -> Vec<BreakpointId> {
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

/// Which of the ids a selector named matched nothing.
///
/// Only meaningful for an id selector: every other kind describes a set, and a
/// set that turns out to be empty is an answer rather than a mistake.
fn unmatched(selector: &WatchSelector, picked: &[Watch]) -> Vec<WatchId> {
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

/// One project's persisted state, cached in memory.
pub struct ProjectStore {
    root: PathBuf,
    path: PathBuf,
    state: Mutex<State>,
    dirty: AtomicBool,
    /// Woken by every mutation; the flusher waits on it.
    changed: Notify,
}

/// The in-memory shape. Paths here are absolute.
struct State {
    breakpoints: Vec<Breakpoint>,
    next_id: u32,
    /// The project's watch expressions (M16).
    ///
    /// Only the expressions. What they evaluate to belongs to a stop, not to
    /// the project, and writing it here would mean a file that says `pos` was
    /// `4` long after the program that made it true has exited.
    watches: Vec<Watch>,
    next_watch_id: u32,
    /// Sections of the file this build does not model — launch configs,
    /// adapter settings, anything a newer lazydap wrote. Held so they can be
    /// written back untouched; dropping them would mean adding one breakpoint
    /// deletes a colleague's configuration.
    unknown: toml::Table,
    /// The file's exact text as we last read or wrote it, so an edit made
    /// behind our back is noticed — and so we can tell what that edit *did*,
    /// which is what separates a hand-added breakpoint from a hand-removed
    /// one.
    seen_text: Option<String>,
}

impl ProjectStore {
    /// Read `root/.lazydap/state.toml`, or start empty if there isn't one.
    ///
    /// A missing file is the normal case — most projects never write one — so
    /// it is not an error. A *malformed* file is: silently starting empty
    /// would look exactly like losing every breakpoint the user had.
    pub fn load(root: impl Into<PathBuf>) -> Result<Arc<Self>> {
        let root = root.into();
        let path = root.join(STATE_DIR).join(STATE_FILE);
        let (document, seen_text) = file::read(&path)?;
        let contents = document.into_memory(&root);

        tracing::debug!(
            target: "daemon.store",
            path = %path.display(),
            breakpoints = contents.breakpoints.len(),
            watches = contents.watches.len(),
            "loaded project state",
        );

        Ok(Arc::new(Self {
            root,
            path,
            state: Mutex::new(State {
                breakpoints: contents.breakpoints,
                next_id: contents.next_breakpoint_id,
                watches: contents.watches,
                next_watch_id: contents.next_watch_id,
                unknown: contents.unknown,
                seen_text,
            }),
            dirty: AtomicBool::new(false),
            changed: Notify::new(),
        }))
    }

    /// Where this store persists to. `lazydap doctor` prints it.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `[[launch_configs]]` in this project's state file, and anything
    /// that could not be read.
    ///
    /// Read-only. Nothing writes launch configs yet — they arrive by being
    /// typed into `state.toml` — so there is no `add` to go with this. They
    /// survive a rewrite because they are carried through `unknown` (see
    /// [`file::launch_configs`]).
    pub fn launch_configs(&self) -> (Vec<LaunchConfig>, Vec<String>) {
        let state = lock(&self.state);
        file::launch_configs(&state.unknown, &self.root)
    }

    pub fn breakpoints(&self) -> Vec<Breakpoint> {
        lock(&self.state).breakpoints.clone()
    }

    /// Every breakpoint in one file, in line order. The unit `setBreakpoints`
    /// works in: codelldb replaces a source's whole breakpoint list on every
    /// call, so the caller always needs all of them at once.
    pub fn breakpoints_in(&self, source: &Path) -> Vec<Breakpoint> {
        let mut found: Vec<Breakpoint> = lock(&self.state)
            .breakpoints
            .iter()
            .filter(|breakpoint| breakpoint.source == source)
            .cloned()
            .collect();
        found.sort_by_key(|breakpoint| breakpoint.line);
        found
    }

    /// Every source that has at least one breakpoint, in a stable order.
    pub fn sources(&self) -> Vec<PathBuf> {
        let mut sources: Vec<PathBuf> = lock(&self.state)
            .breakpoints
            .iter()
            .map(|breakpoint| breakpoint.source.clone())
            .collect();
        sources.sort();
        sources.dedup();
        sources
    }

    /// Which breakpoints a selector picks out, and which of the ids it named
    /// matched nothing.
    ///
    /// The one place selection is decided, so `--dry-run` and the real
    /// mutation cannot drift apart (non-negotiable #4). The preview calls this
    /// directly, the mutation through [`Self::remove`] and [`Self::toggle`],
    /// which run it inside the lock they mutate under.
    pub fn select(&self, selector: &BreakpointSelector) -> (Vec<Breakpoint>, Vec<BreakpointId>) {
        let state = lock(&self.state);
        let picked = selector.pick(&state.breakpoints);
        let not_found = unmatched_breakpoints(selector, &picked);
        (picked, not_found)
    }

    /// Set a breakpoint at a location, whether or not one is already there.
    ///
    /// A location holds at most one breakpoint: setting the same line twice is
    /// something a script does by accident all the time, and two entries for
    /// one visible breakpoint are two things to remove. So the second call
    /// *edits* the first rather than adding beside it, keeping its id — ids
    /// are never reused (D031), and a re-set location is the same breakpoint
    /// with different modifiers rather than a new one (D-WP4-1).
    ///
    /// The whole request wins, including the parts that were not given: a
    /// `--condition` that is absent means "no condition", the same way it does
    /// on the first call. Which is why this can also report that nothing
    /// changed — see [`AddOutcome`].
    pub fn add(&self, new: NewBreakpoint) -> Addition {
        let mut state = lock(&self.state);

        let existing = state
            .breakpoints
            .iter()
            .position(|breakpoint| breakpoint.source == new.source && breakpoint.line == new.line);

        let addition = match existing {
            Some(index) => {
                let existing = &state.breakpoints[index];
                let addition = decide(Some(existing), existing.id, new);
                if addition.outcome == AddOutcome::Unchanged {
                    return addition;
                }
                state.breakpoints[index] = addition.breakpoint.clone();
                addition
            }
            None => {
                let id = BreakpointId(state.next_id);
                state.next_id += 1;
                let addition = decide(None, id, new);
                state.breakpoints.push(addition.breakpoint.clone());
                addition
            }
        };
        drop(state);

        self.touch();
        addition
    }

    /// What [`Self::add`] would do, without doing it.
    ///
    /// The same decision from the same rule, so `--dry-run` cannot promise
    /// something the mutation does not deliver (non-negotiable #4). Both call
    /// [`decide`].
    pub fn preview_add(&self, new: NewBreakpoint) -> Addition {
        let state = lock(&self.state);
        let existing = state
            .breakpoints
            .iter()
            .find(|breakpoint| breakpoint.source == new.source && breakpoint.line == new.line);
        // Id `0` is deliberately not a real one: nothing has been allocated,
        // and printing the id the breakpoint *would* get would be a promise a
        // preview has no way to keep.
        decide(existing, BreakpointId(0), new)
    }

    /// Remove everything the selector picks, and say what it did *not* find.
    ///
    /// Both under one lock, for the reason spelled out on
    /// [`Self::remove_watches`]: a `not_found` derived from a selection made
    /// under an earlier lock lets the loser of a race report success for work
    /// it did not do.
    pub fn remove(&self, selector: &BreakpointSelector) -> Changed {
        let mut state = lock(&self.state);
        let doomed = selector.pick(&state.breakpoints);
        let not_found = unmatched_breakpoints(selector, &doomed);
        if doomed.is_empty() {
            return Changed {
                breakpoints: doomed,
                not_found,
            };
        }

        let removing: Vec<BreakpointId> = doomed.iter().map(|breakpoint| breakpoint.id).collect();
        state
            .breakpoints
            .retain(|breakpoint| !removing.contains(&breakpoint.id));
        drop(state);

        self.touch();
        Changed {
            breakpoints: doomed,
            not_found,
        }
    }

    /// Flip `enabled` on everything the selector picks. Returns them as they
    /// are now, and the ids that matched nothing — decided under the same lock
    /// as the flip, for the reason [`Self::remove`] gives.
    pub fn toggle(&self, selector: &BreakpointSelector) -> Changed {
        let mut state = lock(&self.state);
        let picked = selector.pick(&state.breakpoints);
        let not_found = unmatched_breakpoints(selector, &picked);
        if picked.is_empty() {
            return Changed {
                breakpoints: picked,
                not_found,
            };
        }

        let picked: Vec<BreakpointId> = picked.iter().map(|breakpoint| breakpoint.id).collect();
        let mut toggled = Vec::with_capacity(picked.len());
        for breakpoint in &mut state.breakpoints {
            if picked.contains(&breakpoint.id) {
                breakpoint.enabled = !breakpoint.enabled;
                toggled.push(breakpoint.clone());
            }
        }
        drop(state);

        self.touch();
        Changed {
            breakpoints: toggled,
            not_found,
        }
    }

    // --- Watches (M16) ------------------------------------------------------
    //
    // The same discipline as the breakpoints above, in the same file, behind
    // the same debounce: an expression somebody wants shown at every stop is
    // project state exactly as a breakpoint is, and both are lost the same way
    // if this file is written carelessly.

    pub fn watches(&self) -> Vec<Watch> {
        lock(&self.state).watches.clone()
    }

    /// Which watches a selector picks out, and which of the ids it named
    /// matched nothing.
    ///
    /// The one place watch selection is decided, so `--dry-run` and the real
    /// removal cannot drift apart (non-negotiable #4). Both call this — the
    /// preview directly, the removal through [`Self::remove_watches`], which
    /// runs it inside the lock it mutates under.
    pub fn select_watches(&self, selector: &WatchSelector) -> (Vec<Watch>, Vec<WatchId>) {
        let state = lock(&self.state);
        let picked = selector.pick(&state.watches);
        let not_found = unmatched(selector, &picked);
        (picked, not_found)
    }

    /// Add a watch, or return the existing one with that expression.
    ///
    /// Deduped on the expression for the reason a breakpoint is deduped on its
    /// location: asking twice is something a script does by accident, and two
    /// identical rows in the pane are two things to remove for one thing the
    /// user can see. The label is *not* part of the key — relabelling is an
    /// edit of the same watch, not a second one.
    pub fn add_watch(&self, new: NewWatch) -> Watch {
        let mut state = lock(&self.state);

        if let Some(existing) = state
            .watches
            .iter()
            .find(|watch| watch.expression == new.expression)
        {
            return existing.clone();
        }

        let id = WatchId(state.next_watch_id);
        state.next_watch_id += 1;
        let watch = Watch {
            id,
            expression: new.expression,
            label: new.label,
        };
        state.watches.push(watch.clone());
        drop(state);

        self.touch();
        watch
    }

    /// Remove every watch the selector picks. Returns what went.
    /// Remove every watch the selector picks, and say what it did *not* find.
    ///
    /// Both under one lock, and that is the point. Selecting in one lock and
    /// mutating in another lets two clients removing the same id both see it
    /// there: the winner removes it, and the loser removes nothing while
    /// reporting an empty `not_found` — success, for work it did not do. A
    /// caller that piped ids in would be told every one of them was removed by
    /// whichever call happened to lose.
    pub fn remove_watches(&self, selector: &WatchSelector) -> Removed {
        let mut state = lock(&self.state);
        let doomed = selector.pick(&state.watches);
        // Derived from what this call is actually about to remove, not from a
        // snapshot somebody else may already have changed.
        let not_found = unmatched(selector, &doomed);

        if doomed.is_empty() {
            return Removed {
                watches: doomed,
                not_found,
            };
        }

        let removing: Vec<WatchId> = doomed.iter().map(|watch| watch.id).collect();
        state.watches.retain(|watch| !removing.contains(&watch.id));
        drop(state);

        self.touch();
        Removed {
            watches: doomed,
            not_found,
        }
    }

    /// Write now, whatever the debounce window says. Called on daemon
    /// shutdown, and by tests that would otherwise have to sleep.
    pub fn flush_now(&self) -> Result<()> {
        if !self.dirty.load(Ordering::SeqCst) {
            return Ok(());
        }

        let mut state = lock(&self.state);
        self.adopt_external_edits(&mut state)?;

        let document = file::Document::from_memory(
            &file::Contents {
                breakpoints: state.breakpoints.clone(),
                next_breakpoint_id: state.next_id,
                watches: state.watches.clone(),
                next_watch_id: state.next_watch_id,
                unknown: state.unknown.clone(),
            },
            &self.root,
        );
        let written = file::write(&self.path, &document)?;

        // Cleared only now. Clearing it up front would mean a transient
        // failure — a full disk, a directory that vanished — leaves the store
        // believing it is clean: the flusher never retries, the flush on
        // shutdown is a no-op, and the breakpoint is simply gone.
        self.dirty.store(false, Ordering::SeqCst);
        state.seen_text = Some(written);

        tracing::debug!(
            target: "daemon.store",
            path = %self.path.display(),
            breakpoints = state.breakpoints.len(),
            watches = state.watches.len(),
            "wrote project state",
        );
        Ok(())
    }

    /// Write dirty state to disk, at most once per debounce window.
    ///
    /// Spawn one of these per store. It never returns: the daemon drops it
    /// when the runtime goes away, having called [`flush_now`](Self::flush_now)
    /// on the way out.
    pub async fn run_flusher(self: Arc<Self>) {
        loop {
            self.changed.notified().await;
            // Coalesce the rest of the burst. A script setting twenty
            // breakpoints should cost one write, not twenty.
            tokio::time::sleep(DEBOUNCE).await;
            if let Err(error) = self.flush_now() {
                tracing::warn!(
                    target: "daemon.store",
                    path = %self.path.display(),
                    %error,
                    "could not persist project state",
                );
            }
        }
    }

    fn touch(&self) {
        self.dirty.store(true, Ordering::SeqCst);
        self.changed.notify_one();
    }

    /// Fold in edits somebody made to the file themselves.
    ///
    /// The file is documented as hand-editable (D006), so an edit that lands
    /// between our load and our write must not be silently reverted — and
    /// "reverted" covers a deletion as much as an addition. Both are decided
    /// against a *baseline*: the text of the file as we last read or wrote it.
    /// See [`merge_by_id`] for the rule, and D084 for what it does not
    /// promise.
    fn adopt_external_edits(&self, state: &mut State) -> Result<()> {
        let (document, text) = file::read(&self.path)?;
        if text == state.seen_text {
            return Ok(());
        }
        // A file that is gone or empty is not somebody deleting every
        // breakpoint they own. It is `rm -rf .lazydap` as a reset, or an
        // editor between its truncate and its write, or a disk that filled up
        // — and reading it as a deletion means the next flush, 500ms later,
        // makes it permanent. Keep what we have; the flush that follows
        // rewrites the file.
        if text.as_deref().is_none_or(|text| text.trim().is_empty()) {
            tracing::warn!(
                target: "daemon.store",
                path = %self.path.display(),
                "the state file is missing or empty; keeping what is in memory and rewriting it",
            );
            return Ok(());
        }

        // Whatever we last saw. A baseline that will not parse can only come
        // from bytes we ourselves wrote, so it is not a case to design for —
        // and treating it as empty degrades to adopting everything rather than
        // to deleting anything.
        let baseline = state
            .seen_text
            .as_deref()
            .and_then(|text| toml::from_str::<file::Document>(text).ok())
            .unwrap_or_default()
            .into_memory(&self.root);
        let contents = document.into_memory(&self.root);
        // The file is the authority on the parts we do not model: a newer
        // lazydap may have written launch configs since we loaded.
        state.unknown = contents.unknown;

        let breakpoints = merge_by_id(
            &mut state.breakpoints,
            &baseline.breakpoints,
            contents.breakpoints,
            |breakpoint| breakpoint.id,
        );
        state.next_id = state
            .next_id
            .max(contents.next_breakpoint_id)
            .max(one_past_highest(state.breakpoints.iter().map(|b| b.id.0)));

        // Watches, by exactly the same rule. Typing an expression into the file
        // is the other half of `lazydap watch add` being documented as
        // hand-editable, and one that vanished on the next write would make the
        // file look like a cache rather than the record it is.
        let watches = merge_by_id(
            &mut state.watches,
            &baseline.watches,
            contents.watches,
            |watch| watch.id,
        );
        state.next_watch_id = state
            .next_watch_id
            .max(contents.next_watch_id)
            .max(one_past_highest(state.watches.iter().map(|w| w.id.0)));

        if breakpoints.happened() || watches.happened() {
            tracing::info!(
                target: "daemon.store",
                path = %self.path.display(),
                adopted = breakpoints.adopted,
                dropped = breakpoints.dropped,
                adopted_watches = watches.adopted,
                dropped_watches = watches.dropped,
                "took over edits made to the state file by hand",
            );
        }
        Ok(())
    }
}

/// What a merge did, for the log line.
struct Edits {
    adopted: usize,
    dropped: usize,
}

impl Edits {
    fn happened(&self) -> bool {
        self.adopted + self.dropped > 0
    }
}

/// One past the highest id in use, or 1 for an empty list.
fn one_past_highest(ids: impl Iterator<Item = u32>) -> u32 {
    ids.map(|id| id + 1).max().unwrap_or(1)
}

/// Fold the file's version of a list into ours, by id.
///
/// Three groups, decided against `baseline` — the file as we last read or
/// wrote it:
///
/// - **In the file, not in the baseline.** Added by hand since; adopted.
/// - **In the baseline, not in the file.** Deleted by hand since; dropped from
///   ours too, rather than written straight back.
/// - **In both.** Ours wins, because ours is what a live adapter has already
///   been told about — a file that loses a tie is one `lazydap break` away
///   from being right again, whereas an adapter disagreeing with the file is
///   invisible.
///
/// The one asymmetry: an id in the baseline that *we* no longer hold is a
/// removal of our own that has not been flushed yet, so it is not adopted back
/// even though the file still lists it. Ours is the newer edit.
fn merge_by_id<T, I>(
    ours: &mut Vec<T>,
    baseline: &[T],
    from_file: Vec<T>,
    id_of: impl Fn(&T) -> I,
) -> Edits
where
    I: Copy + Eq + std::hash::Hash,
{
    let was: HashSet<I> = baseline.iter().map(&id_of).collect();
    let now: HashSet<I> = from_file.iter().map(&id_of).collect();

    let before = ours.len();
    ours.retain(|item| {
        let id = id_of(item);
        now.contains(&id) || !was.contains(&id)
    });
    let dropped = before - ours.len();

    let known: HashSet<I> = ours.iter().map(&id_of).collect();
    let mut adopted = 0;
    for item in from_file {
        let id = id_of(&item);
        if !known.contains(&id) && !was.contains(&id) {
            ours.push(item);
            adopted += 1;
        }
    }

    Edits { adopted, dropped }
}

/// A poisoned lock here means a panic left a `Vec<Breakpoint>` mid-push, not a
/// broken invariant. Refusing every later breakpoint over it would be the
/// worse failure.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempProject {
        root: PathBuf,
    }

    impl TempProject {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "lazydap-store-{label}-{}-{:?}",
                std::process::id(),
                std::thread::current().id(),
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("create the project root");
            Self { root }
        }

        fn store(&self) -> Arc<ProjectStore> {
            ProjectStore::load(&self.root).expect("load")
        }

        fn state_file(&self) -> PathBuf {
            self.root.join(STATE_DIR).join(STATE_FILE)
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn new_breakpoint(source: &Path, line: u32) -> NewBreakpoint {
        NewBreakpoint {
            source: source.to_path_buf(),
            line,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }
    }

    #[test]
    fn a_project_with_no_state_file_starts_empty_rather_than_failing() {
        let project = TempProject::new("empty");
        assert!(project.store().breakpoints().is_empty());
    }

    #[test]
    fn a_breakpoint_survives_a_reload() {
        let project = TempProject::new("reload");
        let source = project.root.join("main.c");

        let store = project.store();
        let added = store.add(new_breakpoint(&source, 19)).breakpoint;
        store.flush_now().expect("flush");

        let reloaded = project.store();
        let breakpoints = reloaded.breakpoints();
        assert_eq!(breakpoints.len(), 1);
        assert_eq!(
            breakpoints[0].id, added.id,
            "ids are stable across restarts"
        );
        assert_eq!(
            breakpoints[0].source, source,
            "paths come back absolute even though they are stored relative",
        );
    }

    #[test]
    fn a_path_inside_the_project_is_stored_relative_so_the_file_travels() {
        let project = TempProject::new("relative");
        let store = project.store();
        store.add(new_breakpoint(&project.root.join("src/main.c"), 42));
        store.flush_now().expect("flush");

        let written = std::fs::read_to_string(project.state_file()).expect("read");
        assert!(
            written.contains(r#"source = "src/main.c""#),
            "got: {written}",
        );
        assert!(
            !written.contains(&project.root.display().to_string()),
            "an absolute path would make the file machine-specific: {written}",
        );
    }

    #[test]
    fn a_path_outside_the_project_is_stored_absolute_because_it_has_to_be() {
        let project = TempProject::new("outside");
        let store = project.store();
        store.add(new_breakpoint(Path::new("/usr/src/vendor.c"), 3));
        store.flush_now().expect("flush");

        let written = std::fs::read_to_string(project.state_file()).expect("read");
        assert!(
            written.contains(r#"source = "/usr/src/vendor.c""#),
            "got: {written}",
        );
    }

    #[test]
    fn setting_the_same_line_twice_returns_the_breakpoint_that_is_already_there() {
        let project = TempProject::new("dup");
        let source = project.root.join("main.c");
        let store = project.store();

        let first = store.add(new_breakpoint(&source, 19)).breakpoint;
        let second = store.add(new_breakpoint(&source, 19)).breakpoint;

        assert_eq!(first.id, second.id);
        assert_eq!(store.breakpoints().len(), 1, "one line, one breakpoint");
    }

    #[test]
    fn re_adding_a_location_with_a_new_condition_updates_it_rather_than_dropping_it() {
        // The bug: the second call returned the breakpoint that was already
        // there, untouched, and the caller was told it had been added — so
        // `lazydap break x.c:10 --condition ...` reported success while the
        // debugger went on stopping every time (D-WP4-1).
        let project = TempProject::new("recondition");
        let source = project.root.join("main.c");
        let store = project.store();

        let first = store.add(new_breakpoint(&source, 10));
        assert_eq!(first.outcome, AddOutcome::Created);

        let second = store.add(NewBreakpoint {
            condition: Some("i > 5".to_string()),
            enabled: false,
            ..new_breakpoint(&source, 10)
        });

        assert_eq!(second.outcome, AddOutcome::Updated);
        assert_eq!(
            second.breakpoint.id, first.breakpoint.id,
            "it is the same breakpoint with different modifiers, not a new one",
        );
        let stored = store.breakpoints();
        assert_eq!(stored.len(), 1, "one line, one breakpoint");
        assert_eq!(stored[0].condition.as_deref(), Some("i > 5"));
        assert!(!stored[0].enabled);
    }

    #[test]
    fn setting_a_location_to_exactly_what_it_already_says_reports_no_change() {
        let project = TempProject::new("unchanged");
        let source = project.root.join("main.c");
        let store = project.store();

        store.add(new_breakpoint(&source, 10));
        let again = store.add(new_breakpoint(&source, 10));

        assert_eq!(again.outcome, AddOutcome::Unchanged);
    }

    #[test]
    fn a_dry_run_add_previews_the_change_the_add_makes() {
        // Non-negotiable #4 for the one mutation with no selector: both go
        // through `decide`, so the preview cannot promise an update the real
        // call would not make.
        let project = TempProject::new("preview-add");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 10));

        let asked = || NewBreakpoint {
            condition: Some("i == 3".to_string()),
            ..new_breakpoint(&source, 10)
        };
        let previewed = store.preview_add(asked());
        assert_eq!(
            store.breakpoints()[0].condition,
            None,
            "a preview that wrote to the state file would not be a preview",
        );

        let added = store.add(asked());
        assert_eq!(previewed, added, "the preview promised something else");
    }

    #[test]
    fn ids_keep_climbing_after_a_removal_so_a_stale_id_is_never_reused() {
        let project = TempProject::new("ids");
        let source = project.root.join("main.c");
        let store = project.store();

        let first = store.add(new_breakpoint(&source, 1)).breakpoint;
        store.remove(&BreakpointSelector::Ids(vec![first.id]));
        let second = store.add(new_breakpoint(&source, 2)).breakpoint;

        assert_ne!(
            first.id, second.id,
            "a script holding the old id must not silently hit the new breakpoint",
        );
    }

    #[test]
    fn breakpoints_in_a_file_come_back_in_line_order() {
        let project = TempProject::new("order");
        let source = project.root.join("main.c");
        let store = project.store();

        store.add(new_breakpoint(&source, 42));
        store.add(new_breakpoint(&source, 7));
        store.add(new_breakpoint(&project.root.join("other.c"), 1));

        let lines: Vec<u32> = store
            .breakpoints_in(&source)
            .iter()
            .map(|breakpoint| breakpoint.line)
            .collect();
        assert_eq!(lines, vec![7, 42]);
        assert_eq!(store.sources().len(), 2);
    }

    #[test]
    fn toggling_flips_enabled_and_toggling_again_flips_it_back() {
        let project = TempProject::new("toggle");
        let source = project.root.join("main.c");
        let store = project.store();
        let added = store.add(new_breakpoint(&source, 19)).breakpoint;

        let selector = BreakpointSelector::Ids(vec![added.id]);
        let off = store.toggle(&selector);
        assert!(!off.breakpoints[0].enabled);
        let on = store.toggle(&selector);
        assert!(on.breakpoints[0].enabled);
    }

    #[test]
    fn a_dry_run_selection_is_the_same_selection_the_removal_makes() {
        // Non-negotiable #4: `--dry-run` must not be able to disagree with the
        // mutation about what it is going to do.
        let project = TempProject::new("dry");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 1));
        store.add(new_breakpoint(&source, 2));
        store.add(new_breakpoint(&project.root.join("other.c"), 3));

        let selector = BreakpointSelector::Source(source.clone());
        let (previewed, previewed_missing) = store.select(&selector);
        let removed = store.remove(&selector);

        assert_eq!(previewed, removed.breakpoints);
        assert_eq!(previewed_missing, removed.not_found);
        assert_eq!(store.breakpoints().len(), 1, "the other file is untouched");
    }

    #[test]
    fn a_breakpoint_removal_reports_what_it_removed_rather_than_what_it_once_saw() {
        // Two clients racing on the same id. Selecting in one lock and
        // mutating in another let both see it there: the winner removed it,
        // and the loser removed nothing while reporting an empty `not_found` —
        // success, for work it did not do.
        let project = TempProject::new("bp-race");
        let source = project.root.join("main.c");
        let store = project.store();
        let doomed = store.add(new_breakpoint(&source, 19)).breakpoint;
        let selector = BreakpointSelector::Ids(vec![doomed.id]);

        let winner = store.remove(&selector);
        assert_eq!(winner.breakpoints, vec![doomed.clone()]);
        assert!(winner.not_found.is_empty());

        let loser = store.remove(&selector);
        assert!(loser.breakpoints.is_empty(), "it removed nothing");
        assert_eq!(
            loser.not_found,
            vec![doomed.id],
            "and says so, rather than reporting a removal it did not make",
        );
    }

    #[test]
    fn a_toggle_that_matches_nothing_names_the_id_it_was_given() {
        let project = TempProject::new("bp-toggle-missing");
        let store = project.store();

        let toggled = store.toggle(&BreakpointSelector::Ids(vec![BreakpointId(42)]));

        assert!(toggled.breakpoints.is_empty());
        assert_eq!(toggled.not_found, vec![BreakpointId(42)]);
    }

    #[test]
    fn a_malformed_state_file_is_an_error_not_a_silent_reset() {
        let project = TempProject::new("bad");
        std::fs::create_dir_all(project.root.join(STATE_DIR)).expect("create .lazydap");
        std::fs::write(project.state_file(), "this is not toml [[[").expect("write");

        let error = match ProjectStore::load(&project.root) {
            Err(error) => error,
            Ok(_) => unreachable!("a broken file must not read as an empty one"),
        };
        assert!(
            matches!(error, StoreError::Parse { .. }),
            "starting empty would look exactly like losing every breakpoint: {error}",
        );
    }

    #[test]
    fn a_breakpoint_added_by_hand_is_adopted_rather_than_overwritten() {
        let project = TempProject::new("adopt");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 19));
        store.flush_now().expect("flush");

        // Somebody edits the file while the daemon is up.
        let mut contents = std::fs::read_to_string(project.state_file()).expect("read");
        contents.push_str("\n[[breakpoints]]\nid = 99\nsource = \"hand.c\"\nline = 7\n");
        std::fs::write(project.state_file(), contents).expect("write");

        store.add(new_breakpoint(&source, 20));
        store.flush_now().expect("flush");

        let reloaded = project.store();
        let lines: Vec<u32> = reloaded
            .breakpoints()
            .iter()
            .map(|breakpoint| breakpoint.line)
            .collect();
        assert!(
            lines.contains(&7),
            "the hand-added breakpoint must survive our write: {lines:?}",
        );
        assert!(lines.contains(&19) && lines.contains(&20), "got: {lines:?}");
    }

    #[test]
    fn a_breakpoint_deleted_by_hand_stays_deleted() {
        // The other half of the file being hand-editable (D084). Deleting
        // an entry and watching it come back on the next write makes the file
        // look like a cache lazydap merely humours.
        let project = TempProject::new("handdel");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 19));
        store.add(new_breakpoint(&source, 20));
        store.flush_now().expect("flush");

        // Somebody opens the file and deletes the second entry.
        std::fs::write(
            project.state_file(),
            "version = 1\nnext_breakpoint_id = 3\n\n\
             [[breakpoints]]\nid = 1\nsource = \"main.c\"\nline = 19\nenabled = true\n",
        )
        .expect("write");

        store.add(new_breakpoint(&source, 21));
        store.flush_now().expect("flush");

        let lines: Vec<u32> = project
            .store()
            .breakpoints()
            .iter()
            .map(|breakpoint| breakpoint.line)
            .collect();
        assert!(
            !lines.contains(&20),
            "the hand-deleted breakpoint must not come back: {lines:?}",
        );
        assert!(lines.contains(&19) && lines.contains(&21), "got: {lines:?}");
    }

    #[test]
    fn a_state_file_that_vanished_is_not_a_deletion_of_everything() {
        // `rm -rf .lazydap` as a reset, or a backup tool mid-restore. Reading
        // an absent file as "the user deleted every breakpoint" makes it true
        // 500ms later, which is the whole state of the project gone.
        let project = TempProject::new("gonefile");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 19));
        store.add(new_breakpoint(&source, 20));
        store.flush_now().expect("flush");

        std::fs::remove_file(project.state_file()).expect("remove the state file");

        store.add(new_breakpoint(&source, 21));
        store.flush_now().expect("flush");

        let lines: Vec<u32> = project
            .store()
            .breakpoints()
            .iter()
            .map(|breakpoint| breakpoint.line)
            .collect();
        assert_eq!(lines.len(), 3, "nothing may be lost: {lines:?}");
        assert!(
            lines.contains(&19) && lines.contains(&20) && lines.contains(&21),
            "got: {lines:?}",
        );
    }

    #[test]
    fn a_truncated_state_file_is_not_a_deletion_of_everything() {
        // The window an editor that truncates before it writes leaves open,
        // and what `> state.toml` does on purpose.
        let project = TempProject::new("emptyfile");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 19));
        let watch = store.add_watch(NewWatch {
            expression: "counter".to_string(),
            label: None,
        });
        store.flush_now().expect("flush");

        std::fs::write(project.state_file(), "   \n\n").expect("truncate");

        store.add(new_breakpoint(&source, 20));
        store.flush_now().expect("flush");

        let reloaded = project.store();
        let lines: Vec<u32> = reloaded
            .breakpoints()
            .iter()
            .map(|breakpoint| breakpoint.line)
            .collect();
        assert!(lines.contains(&19) && lines.contains(&20), "got: {lines:?}");
        assert_eq!(
            reloaded.watches(),
            vec![watch],
            "a watch is state too, and the file was not a deletion",
        );
    }

    #[test]
    fn our_own_unflushed_removal_survives_somebody_else_editing_the_file() {
        // The other side of the three-way merge. A removal that has not
        // reached disk yet is still in the file, and adopting "everything the
        // file has that we do not" would undo it the moment anyone touched the
        // file for an unrelated reason.
        let project = TempProject::new("ourremoval");
        let source = project.root.join("main.c");
        let store = project.store();
        let first = store.add(new_breakpoint(&source, 19)).breakpoint;
        let doomed = store.add(new_breakpoint(&source, 20)).breakpoint;
        store.flush_now().expect("flush");

        store.remove(&BreakpointSelector::Ids(vec![doomed.id]));

        let mut contents = std::fs::read_to_string(project.state_file()).expect("read");
        contents.push_str("\n[[breakpoints]]\nid = 99\nsource = \"hand.c\"\nline = 7\n");
        std::fs::write(project.state_file(), contents).expect("write");

        store.flush_now().expect("flush");

        let ids: Vec<u32> = project
            .store()
            .breakpoints()
            .iter()
            .map(|breakpoint| breakpoint.id.0)
            .collect();
        assert!(
            !ids.contains(&doomed.id.0),
            "our removal must not be undone by an unrelated hand edit: {ids:?}",
        );
        assert!(
            ids.contains(&first.id.0) && ids.contains(&99),
            "and the hand-added one is still adopted: {ids:?}",
        );
    }

    #[test]
    fn our_own_unflushed_watch_removal_survives_an_edit_to_the_file() {
        let project = TempProject::new("ourwatchremoval");
        let store = project.store();
        let kept = store.add_watch(NewWatch {
            expression: "kept".to_string(),
            label: None,
        });
        let doomed = store.add_watch(NewWatch {
            expression: "doomed".to_string(),
            label: None,
        });
        store.flush_now().expect("flush");

        store.remove_watches(&WatchSelector::Ids(vec![doomed.id]));

        let mut contents = std::fs::read_to_string(project.state_file()).expect("read");
        contents.push_str("\n[[watches]]\nid = 99\nexpression = \"by_hand\"\n");
        std::fs::write(project.state_file(), contents).expect("write");

        store.flush_now().expect("flush");

        let expressions: Vec<String> = project
            .store()
            .watches()
            .iter()
            .map(|watch| watch.expression.clone())
            .collect();
        assert!(
            !expressions.contains(&"doomed".to_string()),
            "got: {expressions:?}",
        );
        assert!(
            expressions.contains(&kept.expression) && expressions.contains(&"by_hand".to_string()),
            "got: {expressions:?}",
        );
    }

    #[test]
    fn an_edit_made_in_the_same_moment_as_our_own_write_is_still_noticed() {
        // mtime has a resolution, and an edit landing inside the same tick as
        // our write used to be indistinguishable from no edit at all — so the
        // next flush silently reverted it.
        let project = TempProject::new("sametick");
        let source = project.root.join("main.c");
        let store = project.store();
        store.add(new_breakpoint(&source, 19));
        store.flush_now().expect("flush");

        let stamp = std::fs::metadata(project.state_file())
            .and_then(|metadata| metadata.modified())
            .expect("our write's mtime");
        let mut contents = std::fs::read_to_string(project.state_file()).expect("read");
        contents.push_str("\n[[breakpoints]]\nid = 99\nsource = \"hand.c\"\nline = 7\n");
        std::fs::write(project.state_file(), contents).expect("write");
        std::fs::File::options()
            .write(true)
            .open(project.state_file())
            .and_then(|file| file.set_modified(stamp))
            .expect("backdate the edit onto our own mtime");

        store.add(new_breakpoint(&source, 20));
        store.flush_now().expect("flush");

        let lines: Vec<u32> = project
            .store()
            .breakpoints()
            .iter()
            .map(|breakpoint| breakpoint.line)
            .collect();
        assert!(
            lines.contains(&7),
            "an edit sharing our mtime is still an edit: {lines:?}",
        );
    }

    #[test]
    fn an_abandoned_temporary_file_is_cleared_by_the_next_write() {
        // A crash between the write and the rename leaves one of these
        // forever; nothing else ever removes them.
        let project = TempProject::new("tmplitter");
        let store = project.store();
        store.add(new_breakpoint(&project.root.join("main.c"), 1));
        store.flush_now().expect("flush");

        let litter = project.state_file().with_extension("toml.tmp.999999");
        std::fs::write(&litter, b"half a file").expect("write the litter");
        let fresh = project.state_file().with_extension("toml.tmp.999998");
        std::fs::write(&fresh, b"a live writer's").expect("write the fresh one");
        std::fs::File::options()
            .write(true)
            .open(&litter)
            .and_then(|file| {
                file.set_modified(std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60))
            })
            .expect("backdate");

        store.add(new_breakpoint(&project.root.join("main.c"), 2));
        store.flush_now().expect("flush");

        assert!(!litter.exists(), "an abandoned temporary must be cleared");
        assert!(
            fresh.exists(),
            "a temporary young enough to belong to a live writer must be left alone",
        );
    }

    #[test]
    fn an_adopted_id_pushes_the_counter_past_it_so_the_next_add_is_unique() {
        let project = TempProject::new("adoptid");
        std::fs::create_dir_all(project.root.join(STATE_DIR)).expect("create .lazydap");
        std::fs::write(
            project.state_file(),
            "version = 1\n\n[[breakpoints]]\nid = 41\nsource = \"a.c\"\nline = 1\n",
        )
        .expect("write");

        let store = project.store();
        let added = store
            .add(new_breakpoint(&project.root.join("b.c"), 2))
            .breakpoint;
        assert_eq!(
            added.id,
            BreakpointId(42),
            "ids continue past what is there"
        );
    }

    #[test]
    fn a_write_that_fails_leaves_the_store_dirty_so_the_next_one_retries() {
        // Clearing `dirty` before the write means a transient failure — a full
        // disk, a directory that has gone — looks like success: the flusher
        // never retries, the flush on shutdown is a no-op, and the breakpoint
        // is simply lost.
        let project = TempProject::new("retry");
        let store = project.store();
        store.add(new_breakpoint(&project.root.join("main.c"), 19));

        // A directory where the state file goes: the rename cannot succeed.
        std::fs::create_dir_all(project.state_file()).expect("block the state file");
        assert!(store.flush_now().is_err(), "the write should have failed");

        // Unblock it, and the retry must still have something to write.
        std::fs::remove_dir_all(project.state_file()).expect("unblock");
        store.flush_now().expect("the retry writes");

        assert_eq!(
            project.store().breakpoints().len(),
            1,
            "the breakpoint survived a failed write",
        );
    }

    #[test]
    fn state_this_build_does_not_model_survives_a_breakpoint_being_added() {
        // A colleague on a newer build has sections this one has never heard
        // of, and adding one breakpoint must not delete them. This used to be
        // written with `[[watches]]`, which stopped being a fair test the
        // moment M16 gave them a typed field.
        let project = TempProject::new("preserve");
        std::fs::create_dir_all(project.root.join(STATE_DIR)).expect("create .lazydap");
        std::fs::write(
            project.state_file(),
            "version = 1\n\n[[data_breakpoints]]\nid = \"d1\"\naddress = \"0x7ffd\"\n",
        )
        .expect("write");

        let store = project.store();
        store.add(new_breakpoint(&project.root.join("main.c"), 19));
        store.flush_now().expect("flush");

        let written = std::fs::read_to_string(project.state_file()).expect("read");
        assert!(written.contains("data_breakpoints"), "got: {written}");
        assert!(written.contains("0x7ffd"), "got: {written}");
        assert!(written.contains("[[breakpoints]]"), "got: {written}");
    }

    // --- Watches (M16) ------------------------------------------------------

    fn new_watch(expression: &str) -> NewWatch {
        NewWatch {
            expression: expression.to_string(),
            label: None,
        }
    }

    #[test]
    fn a_watch_survives_a_reload() {
        let project = TempProject::new("watch-reload");
        let store = project.store();
        store.add_watch(new_watch("tokens[pos]"));
        store.flush_now().expect("flush");

        let watches = project.store().watches();
        assert_eq!(watches.len(), 1);
        assert_eq!(watches[0].expression, "tokens[pos]");
    }

    #[test]
    fn adding_the_same_expression_twice_returns_the_watch_that_is_already_there() {
        let project = TempProject::new("watch-dupe");
        let store = project.store();
        let first = store.add_watch(new_watch("counter"));
        let second = store.add_watch(new_watch("counter"));

        assert_eq!(first.id, second.id);
        assert_eq!(store.watches().len(), 1, "one row for one expression");
    }

    #[test]
    fn watch_ids_keep_climbing_after_a_removal_so_a_stale_id_is_never_reused() {
        // A script holding id 1 must not silently start addressing somebody
        // else's expression.
        let project = TempProject::new("watch-ids");
        let store = project.store();
        let first = store.add_watch(new_watch("a"));
        store.remove_watches(&WatchSelector::Ids(vec![first.id]));
        let second = store.add_watch(new_watch("b"));

        assert_ne!(first.id, second.id, "an id is never handed out twice");
    }

    #[test]
    fn removing_a_watch_returns_what_went() {
        let project = TempProject::new("watch-remove");
        let store = project.store();
        store.add_watch(new_watch("a"));
        let doomed = store.add_watch(new_watch("b"));

        let removed = store.remove_watches(&WatchSelector::Ids(vec![doomed.id]));
        assert_eq!(removed.watches, vec![doomed]);
        assert!(removed.not_found.is_empty());
        assert_eq!(store.watches().len(), 1);
    }

    #[test]
    fn a_removal_reports_what_it_removed_rather_than_what_it_once_saw() {
        // Two clients racing on the same id. Selecting in one lock and
        // mutating in another let both see it there: the winner removed it,
        // and the loser removed nothing while reporting an empty `not_found` —
        // success, for work it did not do.
        let project = TempProject::new("watch-race");
        let store = project.store();
        let doomed = store.add_watch(new_watch("a"));
        let selector = WatchSelector::Ids(vec![doomed.id]);

        let winner = store.remove_watches(&selector);
        assert_eq!(winner.watches, vec![doomed.clone()]);
        assert!(winner.not_found.is_empty());

        let loser = store.remove_watches(&selector);
        assert!(loser.watches.is_empty(), "it removed nothing");
        assert_eq!(
            loser.not_found,
            vec![doomed.id],
            "and says so, rather than reporting a removal it did not make",
        );
    }

    #[test]
    fn a_dry_run_watch_selection_is_the_same_selection_the_removal_makes() {
        // Non-negotiable #4: the preview and the mutation share one `pick`, so
        // they cannot disagree about what is about to go.
        let project = TempProject::new("watch-dry");
        let store = project.store();
        store.add_watch(new_watch("a"));
        store.add_watch(new_watch("b"));

        let selector = WatchSelector::Expression("b".to_string());
        let (previewed, previewed_missing) = store.select_watches(&selector);
        assert_eq!(store.watches().len(), 2, "a preview changes nothing");

        let removed = store.remove_watches(&selector);
        assert_eq!(previewed, removed.watches);
        assert_eq!(previewed_missing, removed.not_found);
    }

    #[test]
    fn a_watch_added_by_hand_is_adopted_rather_than_overwritten() {
        // The other half of the file being hand-editable (D006): an expression
        // typed in while the daemon was running must survive the next write.
        let project = TempProject::new("watch-adopt");
        let store = project.store();
        store.add_watch(new_watch("ours"));
        store.flush_now().expect("flush");

        let existing = std::fs::read_to_string(project.state_file()).expect("read");
        std::fs::write(
            project.state_file(),
            format!("{existing}\n[[watches]]\nid = 40\nexpression = \"theirs\"\n"),
        )
        .expect("hand-edit");

        store.add_watch(new_watch("later"));
        store.flush_now().expect("flush");

        let expressions: Vec<String> = project
            .store()
            .watches()
            .into_iter()
            .map(|watch| watch.expression)
            .collect();
        assert!(expressions.contains(&"ours".to_string()), "{expressions:?}");
        assert!(
            expressions.contains(&"theirs".to_string()),
            "the hand-written one was adopted, not reverted: {expressions:?}",
        );
        assert!(
            expressions.contains(&"later".to_string()),
            "{expressions:?}"
        );
    }

    #[test]
    fn an_adopted_watch_id_pushes_the_counter_past_it_so_the_next_add_is_unique() {
        let project = TempProject::new("watch-adopt-ids");
        std::fs::create_dir_all(project.root.join(STATE_DIR)).expect("create .lazydap");
        std::fs::write(
            project.state_file(),
            "version = 1\n\n[[watches]]\nid = 41\nexpression = \"theirs\"\n",
        )
        .expect("write");

        let added = project.store().add_watch(new_watch("ours"));
        assert_eq!(added.id, WatchId(42), "ids continue past what is there");
    }

    #[test]
    fn watches_and_breakpoints_share_one_file_without_disturbing_each_other() {
        // They are two lists in one document with two independent counters.
        // Getting that wrong shows up as a watch taking a breakpoint's id.
        let project = TempProject::new("watch-both");
        let store = project.store();
        store.add(new_breakpoint(&project.root.join("main.c"), 19));
        let watch = store.add_watch(new_watch("counter"));
        store.flush_now().expect("flush");

        assert_eq!(
            watch.id,
            WatchId(1),
            "the watch counter starts at 1 regardless of the breakpoints",
        );

        let reloaded = project.store();
        assert_eq!(reloaded.breakpoints().len(), 1);
        assert_eq!(reloaded.watches().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn the_flusher_writes_a_burst_of_changes_once() {
        let project = TempProject::new("debounce");
        let source = project.root.join("main.c");
        let store = project.store();

        let flusher = tokio::spawn(Arc::clone(&store).run_flusher());

        for line in 1..=20 {
            store.add(new_breakpoint(&source, line));
        }
        assert!(
            !project.state_file().exists(),
            "nothing should be written while the burst is still arriving",
        );

        tokio::time::sleep(DEBOUNCE * 2).await;
        tokio::task::yield_now().await;

        let written = std::fs::read_to_string(project.state_file()).expect("the flusher wrote");
        assert_eq!(
            written.matches("[[breakpoints]]").count(),
            20,
            "all twenty landed in the one write: {written}",
        );
        flusher.abort();
    }
}
