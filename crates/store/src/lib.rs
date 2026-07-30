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

mod file;
mod selector;

use lazydap_core::{Breakpoint, BreakpointId};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::Notify;

pub use selector::BreakpointSelector;

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
    /// What the file looked like when we last read or wrote it, so an edit
    /// made behind our back is noticed rather than silently overwritten.
    seen_mtime: Option<SystemTime>,
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
        let (document, seen_mtime) = file::read(&path)?;
        let (breakpoints, next_id) = document.into_memory(&root);

        tracing::debug!(
            target: "daemon.store",
            path = %path.display(),
            breakpoints = breakpoints.len(),
            "loaded project state",
        );

        Ok(Arc::new(Self {
            root,
            path,
            state: Mutex::new(State {
                breakpoints,
                next_id,
                seen_mtime,
            }),
            dirty: AtomicBool::new(false),
            changed: Notify::new(),
        }))
    }

    /// Where this store persists to. `lazydap doctor` prints it.
    pub fn path(&self) -> &Path {
        &self.path
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

    /// Which breakpoints a selector picks out.
    ///
    /// The one place selection is decided, so `--dry-run` and the real
    /// mutation cannot drift apart (non-negotiable #4). Both call this.
    pub fn select(&self, selector: &BreakpointSelector) -> Vec<Breakpoint> {
        let state = lock(&self.state);
        selector.pick(&state.breakpoints)
    }

    /// Add a breakpoint, or return the existing one at that location.
    ///
    /// Setting the same line twice is something a script does by accident all
    /// the time; making it a duplicate rather than a no-op would mean two
    /// entries the user has to remove separately for one visible breakpoint.
    pub fn add(&self, new: NewBreakpoint) -> Breakpoint {
        let mut state = lock(&self.state);

        if let Some(existing) = state
            .breakpoints
            .iter()
            .find(|breakpoint| breakpoint.source == new.source && breakpoint.line == new.line)
        {
            return existing.clone();
        }

        let id = BreakpointId(state.next_id);
        state.next_id += 1;
        let breakpoint = Breakpoint {
            id,
            source: new.source,
            line: new.line,
            column: new.column,
            condition: new.condition,
            hit_condition: new.hit_condition,
            log_message: new.log_message,
            enabled: new.enabled,
        };
        state.breakpoints.push(breakpoint.clone());
        drop(state);

        self.touch();
        breakpoint
    }

    /// Remove everything the selector picks. Returns what went.
    pub fn remove(&self, selector: &BreakpointSelector) -> Vec<Breakpoint> {
        let mut state = lock(&self.state);
        let doomed = selector.pick(&state.breakpoints);
        if doomed.is_empty() {
            return doomed;
        }

        let removing: Vec<BreakpointId> = doomed.iter().map(|breakpoint| breakpoint.id).collect();
        state
            .breakpoints
            .retain(|breakpoint| !removing.contains(&breakpoint.id));
        drop(state);

        self.touch();
        doomed
    }

    /// Flip `enabled` on everything the selector picks. Returns them as they
    /// are now.
    pub fn toggle(&self, selector: &BreakpointSelector) -> Vec<Breakpoint> {
        let mut state = lock(&self.state);
        let picked: Vec<BreakpointId> = selector
            .pick(&state.breakpoints)
            .iter()
            .map(|breakpoint| breakpoint.id)
            .collect();
        if picked.is_empty() {
            return Vec::new();
        }

        let mut toggled = Vec::with_capacity(picked.len());
        for breakpoint in &mut state.breakpoints {
            if picked.contains(&breakpoint.id) {
                breakpoint.enabled = !breakpoint.enabled;
                toggled.push(breakpoint.clone());
            }
        }
        drop(state);

        self.touch();
        toggled
    }

    /// Write now, whatever the debounce window says. Called on daemon
    /// shutdown, and by tests that would otherwise have to sleep.
    pub fn flush_now(&self) -> Result<()> {
        if !self.dirty.swap(false, Ordering::SeqCst) {
            return Ok(());
        }

        let mut state = lock(&self.state);
        self.adopt_external_edits(&mut state)?;

        let document = file::Document::from_memory(&state.breakpoints, state.next_id, &self.root);
        let mtime = file::write(&self.path, &document)?;
        state.seen_mtime = mtime;

        tracing::debug!(
            target: "daemon.store",
            path = %self.path.display(),
            breakpoints = state.breakpoints.len(),
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

    /// Fold in breakpoints somebody added by editing the file themselves.
    ///
    /// The file is documented as hand-editable (D006), so an edit that lands
    /// between our load and our write must not be silently reverted. Entries
    /// only in the file are adopted; an id in both keeps *our* version,
    /// because ours is what the live adapter has already been told about — a
    /// file that loses a tie is one `lazydap break` away from being right
    /// again, whereas an adapter disagreeing with the file is invisible.
    fn adopt_external_edits(&self, state: &mut State) -> Result<()> {
        let (document, mtime) = file::read(&self.path)?;
        if mtime == state.seen_mtime {
            return Ok(());
        }

        let (on_disk, disk_next_id) = document.into_memory(&self.root);
        let known: Vec<BreakpointId> = state
            .breakpoints
            .iter()
            .map(|breakpoint| breakpoint.id)
            .collect();

        let mut adopted = 0;
        for breakpoint in on_disk {
            if !known.contains(&breakpoint.id) {
                state.next_id = state.next_id.max(breakpoint.id.0 + 1);
                state.breakpoints.push(breakpoint);
                adopted += 1;
            }
        }
        state.next_id = state.next_id.max(disk_next_id);

        if adopted > 0 {
            tracing::info!(
                target: "daemon.store",
                path = %self.path.display(),
                adopted,
                "adopted breakpoints added by editing the state file",
            );
        }
        Ok(())
    }
}

/// A breakpoint that does not have an id yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBreakpoint {
    /// Absolute, resolved by the client.
    pub source: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
    pub enabled: bool,
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
        let added = store.add(new_breakpoint(&source, 19));
        store.flush_now().expect("flush");

        let reloaded = project.store();
        let breakpoints = reloaded.breakpoints();
        assert_eq!(breakpoints.len(), 1);
        assert_eq!(breakpoints[0].id, added.id, "ids are stable across restarts");
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

        let first = store.add(new_breakpoint(&source, 19));
        let second = store.add(new_breakpoint(&source, 19));

        assert_eq!(first.id, second.id);
        assert_eq!(store.breakpoints().len(), 1, "one line, one breakpoint");
    }

    #[test]
    fn ids_keep_climbing_after_a_removal_so_a_stale_id_is_never_reused() {
        let project = TempProject::new("ids");
        let source = project.root.join("main.c");
        let store = project.store();

        let first = store.add(new_breakpoint(&source, 1));
        store.remove(&BreakpointSelector::Ids(vec![first.id]));
        let second = store.add(new_breakpoint(&source, 2));

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
        let added = store.add(new_breakpoint(&source, 19));

        let selector = BreakpointSelector::Ids(vec![added.id]);
        let off = store.toggle(&selector);
        assert!(!off[0].enabled);
        let on = store.toggle(&selector);
        assert!(on[0].enabled);
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
        let previewed = store.select(&selector);
        let removed = store.remove(&selector);

        assert_eq!(previewed, removed);
        assert_eq!(store.breakpoints().len(), 1, "the other file is untouched");
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
    fn an_adopted_id_pushes_the_counter_past_it_so_the_next_add_is_unique() {
        let project = TempProject::new("adoptid");
        std::fs::create_dir_all(project.root.join(STATE_DIR)).expect("create .lazydap");
        std::fs::write(
            project.state_file(),
            "version = 1\n\n[[breakpoints]]\nid = 41\nsource = \"a.c\"\nline = 1\n",
        )
        .expect("write");

        let store = project.store();
        let added = store.add(new_breakpoint(&project.root.join("b.c"), 2));
        assert_eq!(added.id, BreakpointId(42), "ids continue past what is there");
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
