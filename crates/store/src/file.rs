//! Reading and writing `.lazydap/state.toml`.
//!
//! The on-disk shape is separate from the in-memory one for one reason: paths.
//! On disk they are relative to the project root wherever possible, so the
//! file means the same thing after a `git clone` into a different directory.
//! In memory they are absolute, because that is what gets handed to an
//! adapter.

use super::{Result, StoreError};
use lazydap_core::{Breakpoint, LaunchConfig, LaunchConfigSource, LaunchKind, Watch};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The schema version written into the file. Bumping it means old lazydaps
/// must be able to read the new file or refuse it loudly; nothing has needed
/// that yet.
const SCHEMA_VERSION: u32 = 1;

/// The file, exactly as TOML sees it.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Document {
    #[serde(default)]
    pub version: u32,
    /// Persisted so an id is never reused after its breakpoint is removed: a
    /// script holding a stale id must not silently start hitting a different
    /// breakpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_breakpoint_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breakpoints: Vec<Breakpoint>,
    /// The watch counter, persisted for the reason the breakpoint one is: a
    /// removed watch must not have its id handed to a different expression
    /// later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_watch_id: Option<u32>,
    /// The project's watch expressions (M16). Modelled rather than left in
    /// [`Self::unknown`], because lazydap *writes* these — which is the line
    /// between the two: what this build writes it models, what it only reads it
    /// leaves where it found it (see [`launch_configs`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watches: Vec<Watch>,
    /// Anything a newer lazydap wrote that this one does not model — launch
    /// configs, adapter settings, preferences — is carried through a rewrite
    /// untouched rather than deleted. Losing a colleague's launch configs
    /// because they run a newer build would be a nasty way to find out about
    /// this file.
    #[serde(flatten)]
    pub unknown: toml::Table,
}

/// What the file says, in the shapes the store keeps in memory.
///
/// A struct rather than the tuple this used to be. Two counters, two lists and
/// the unmodelled remainder is past the point where positional returns can be
/// read at the call site — and a `(Vec<Breakpoint>, u32, Vec<Watch>, u32,
/// Table)` is exactly the shape in which somebody eventually passes the watch
/// counter as the breakpoint one.
pub struct Contents {
    pub breakpoints: Vec<Breakpoint>,
    /// The id the next breakpoint should take.
    pub next_breakpoint_id: u32,
    pub watches: Vec<Watch>,
    /// The id the next watch should take.
    pub next_watch_id: u32,
    /// The sections this build does not model, which the caller has to hold on
    /// to and hand back, or the next write deletes them.
    pub unknown: toml::Table,
}

impl Document {
    /// Absolute paths, the ids the next breakpoint and watch should take, and
    /// whatever sections this build does not model.
    pub fn into_memory(self, root: &Path) -> Contents {
        let breakpoints: Vec<Breakpoint> = self
            .breakpoints
            .into_iter()
            .map(|mut breakpoint| {
                breakpoint.source = absolutise(&breakpoint.source, root);
                breakpoint
            })
            .collect();

        let next_breakpoint_id = next_id(
            self.next_breakpoint_id,
            breakpoints.iter().map(|breakpoint| breakpoint.id.0),
        );
        let next_watch_id = next_id(
            self.next_watch_id,
            self.watches.iter().map(|watch| watch.id.0),
        );

        Contents {
            breakpoints,
            next_breakpoint_id,
            watches: self.watches,
            next_watch_id,
            unknown: self.unknown,
        }
    }

    pub fn from_memory(contents: &Contents, root: &Path) -> Self {
        let mut breakpoints: Vec<Breakpoint> = contents
            .breakpoints
            .iter()
            .map(|breakpoint| Breakpoint {
                source: relativise(&breakpoint.source, root),
                ..breakpoint.clone()
            })
            .collect();
        // A stable order keeps the file diffable, which is the point of it
        // being TOML at all (D006).
        breakpoints.sort_by(|a, b| a.source.cmp(&b.source).then(a.line.cmp(&b.line)));

        // Watches sort by id rather than by expression: the id is the order
        // they were added in, which is the order they are read in on screen,
        // and sorting by text would shuffle the pane whenever one was renamed.
        let mut watches = contents.watches.clone();
        watches.sort_by_key(|watch| watch.id);

        Self {
            version: SCHEMA_VERSION,
            next_breakpoint_id: Some(contents.next_breakpoint_id),
            breakpoints,
            next_watch_id: Some(contents.next_watch_id),
            watches,
            // Carried straight back out. These are the sections this build does
            // not model — launch configs, adapter settings, or whatever a newer
            // lazydap put there. Rewriting the file to add one breakpoint must
            // not delete a colleague's launch configs because they run a newer
            // build.
            unknown: contents.unknown.clone(),
        }
    }
}

/// The next id to hand out: whatever the file said, but never one that is
/// already in use.
///
/// A stale counter is the ordinary case for a hand-edited file, and handing
/// back a live id would silently point a script holding it at somebody else's
/// breakpoint.
fn next_id(counter: Option<u32>, in_use: impl Iterator<Item = u32>) -> u32 {
    let highest = in_use.map(|id| id + 1).max().unwrap_or(1);
    counter.unwrap_or(1).max(highest)
}

/// Read the state file, returning the empty document if there isn't one.
///
/// The mtime comes back with it so a caller can tell whether the file changed
/// under them between two reads.
pub fn read(path: &Path) -> Result<(Document, Option<SystemTime>)> {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Document::default(), None));
        }
        Err(source) => {
            return Err(StoreError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    let document = toml::from_str(&contents).map_err(|source| StoreError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    Ok((document, mtime(path)))
}

/// Write the state file atomically, creating `.lazydap/` if it is missing.
///
/// Write-then-rename, so a crash or a full disk mid-write leaves the previous
/// state intact rather than half a file where a user's breakpoints used to be.
pub fn write(path: &Path, document: &Document) -> Result<Option<SystemTime>> {
    let serialised = toml::to_string_pretty(document)?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| StoreError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    // Same directory as the target, because `rename` is only atomic within a
    // filesystem — and named per process, because two daemons pointed at one
    // project (see the single-writer note in `lib.rs`) would otherwise write
    // the *same* temporary file at the same time and rename each other's
    // half-written bytes into place. Unique names make the worst case a lost
    // update rather than a corrupt file.
    let temporary = path.with_extension(format!("toml.tmp.{}", std::process::id()));
    std::fs::write(&temporary, serialised).map_err(|source| StoreError::Write {
        path: temporary.clone(),
        source,
    })?;
    std::fs::rename(&temporary, path).map_err(|source| {
        let _ = std::fs::remove_file(&temporary);
        StoreError::Write {
            path: path.to_path_buf(),
            source,
        }
    })?;

    Ok(mtime(path))
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
}

/// A path inside the project becomes relative to it; anything else stays as it
/// is. Vendored sources and system headers legitimately live elsewhere, and
/// `../../../usr/src/x.c` would be both ugly and wrong the moment the project
/// moves.
fn relativise(source: &Path, root: &Path) -> PathBuf {
    source
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| source.to_path_buf())
}

/// The `[[launch_configs]]` a hand-written state file may carry.
///
/// Read-only, and read out of [`Document::unknown`] rather than modelled as a
/// field. Nothing in lazydap *writes* launch configs yet, and a typed field
/// that serialises as empty would delete the ones somebody typed by hand the
/// first time a breakpoint was added. Leaving them in `unknown` keeps the
/// round-trip that already protects them, and this function reads a copy.
///
/// An entry that does not parse costs only itself, and says so: a state file
/// is hand-edited, and one typo should not hide the four configurations
/// underneath it.
pub fn launch_configs(unknown: &toml::Table, root: &Path) -> (Vec<LaunchConfig>, Vec<String>) {
    let Some(entries) = unknown
        .get("launch_configs")
        .and_then(toml::Value::as_array)
    else {
        return (Vec::new(), Vec::new());
    };

    let mut configs = Vec::new();
    let mut warnings = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        match entry.clone().try_into::<StoredLaunchConfig>() {
            Ok(stored) => configs.push(stored.into_memory(root)),
            Err(error) => warnings.push(format!(
                "launch config {index} in state.toml could not be read: {error}",
            )),
        }
    }
    (configs, warnings)
}

/// One `[[launch_configs]]` entry, exactly as TOML sees it.
///
/// `id` is accepted and ignored: the blueprint's schema has one, and lookup
/// here is by name.
#[derive(Debug, Deserialize)]
struct StoredLaunchConfig {
    name: String,
    #[serde(default = "default_adapter")]
    adapter: String,
    /// Typed, unlike `adapter`: an adapter this build does not ship is a
    /// configuration lazydap cannot run, and a `kind` that is neither of these
    /// is a typo. Reading it as `launch` would start something the file did
    /// not ask for.
    #[serde(default)]
    kind: StoredLaunchKind,
    program: Option<PathBuf>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<PathBuf>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    stop_on_entry: bool,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredLaunchKind {
    #[default]
    Launch,
    Attach,
}

fn default_adapter() -> String {
    lazydap_core::AdapterKind::default().as_str().to_string()
}

impl StoredLaunchConfig {
    fn into_memory(self, root: &Path) -> LaunchConfig {
        LaunchConfig {
            name: self.name,
            adapter: self.adapter.parse().ok(),
            adapter_type: self.adapter,
            kind: match self.kind {
                StoredLaunchKind::Launch => LaunchKind::Launch,
                StoredLaunchKind::Attach => LaunchKind::Attach,
            },
            program: self.program.map(|program| absolutise(&program, root)),
            args: self.args,
            cwd: self.cwd.map(|cwd| absolutise(&cwd, root)),
            env: self.env,
            stop_on_entry: self.stop_on_entry,
            // Not spelled in lazydap's own file. `[adapter.<kind>] command`
            // in the user config is where a pinned adapter binary goes here,
            // and it applies to every launch rather than one of them.
            adapter_command: None,
            source: LaunchConfigSource::ProjectState,
            // Nothing substitutes variables in lazydap's own file: it is
            // lazydap's, so a path in it means what it says.
            unresolved: Vec::new(),
            blocked: None,
        }
    }
}

fn absolutise(source: &Path, root: &Path) -> PathBuf {
    if source.is_absolute() {
        source.to_path_buf()
    } else {
        root.join(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::BreakpointId;

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

    fn watch(id: u32, expression: &str) -> Watch {
        Watch {
            id: lazydap_core::WatchId(id),
            expression: expression.to_string(),
            label: None,
        }
    }

    /// The contents of a file holding exactly these breakpoints and watches,
    /// with both counters one past the highest id.
    fn contents(breakpoints: Vec<Breakpoint>, watches: Vec<Watch>) -> Contents {
        Contents {
            next_breakpoint_id: next_id(None, breakpoints.iter().map(|b| b.id.0)),
            next_watch_id: next_id(None, watches.iter().map(|w| w.id.0)),
            breakpoints,
            watches,
            unknown: toml::Table::new(),
        }
    }

    #[test]
    fn the_file_is_written_in_a_stable_order_so_it_diffs_cleanly() {
        let document = Document::from_memory(
            &contents(
                vec![
                    breakpoint(1, "/p/z.c", 5),
                    breakpoint(2, "/p/a.c", 20),
                    breakpoint(3, "/p/a.c", 2),
                ],
                Vec::new(),
            ),
            Path::new("/p"),
        );

        let sources: Vec<String> = document
            .breakpoints
            .iter()
            .map(|breakpoint| format!("{}:{}", breakpoint.source.display(), breakpoint.line))
            .collect();
        assert_eq!(sources, vec!["a.c:2", "a.c:20", "z.c:5"]);
    }

    #[test]
    fn watches_are_written_in_the_order_they_were_added_rather_than_alphabetically() {
        // The pane reads top to bottom in the order they were set up. Sorting
        // by text would shuffle it whenever an expression was edited.
        let document = Document::from_memory(
            &contents(
                Vec::new(),
                vec![watch(2, "zebra"), watch(1, "alpha"), watch(3, "middle")],
            ),
            Path::new("/p"),
        );

        // Ids 1, 2, 3 were added in that order and hold expressions that sort
        // the other way, so a file ordered alphabetically would read
        // "alpha, middle, zebra" instead.
        let expressions: Vec<&str> = document
            .watches
            .iter()
            .map(|watch| watch.expression.as_str())
            .collect();
        assert_eq!(expressions, vec!["alpha", "zebra", "middle"]);
    }

    #[test]
    fn a_file_with_no_next_id_derives_one_past_the_highest_it_has() {
        // Hand-written files will not have the counters in them.
        let document: Document = toml::from_str(
            "version = 1\n\n[[breakpoints]]\nid = 7\nsource = \"a.c\"\nline = 1\n\n\
             [[watches]]\nid = 4\nexpression = \"counter\"\n",
        )
        .expect("parse");
        let contents = document.into_memory(Path::new("/p"));

        assert_eq!(contents.next_breakpoint_id, 8);
        assert_eq!(contents.next_watch_id, 5);
    }

    #[test]
    fn a_stale_counter_never_hands_back_an_id_that_is_in_use() {
        let document: Document = toml::from_str(
            "version = 1\nnext_breakpoint_id = 2\nnext_watch_id = 2\n\n\
             [[breakpoints]]\nid = 9\nsource = \"a.c\"\nline = 1\n\n\
             [[watches]]\nid = 6\nexpression = \"counter\"\n",
        )
        .expect("parse");
        let contents = document.into_memory(Path::new("/p"));

        assert_eq!(
            contents.next_breakpoint_id, 10,
            "the counter cannot go backwards past a live id",
        );
        assert_eq!(contents.next_watch_id, 7, "nor can the watch one");
    }

    #[test]
    fn a_watch_survives_a_write_and_a_read() {
        let document = Document::from_memory(
            &contents(Vec::new(), vec![watch(1, "tokens[pos]")]),
            Path::new("/p"),
        );
        let written = toml::to_string_pretty(&document).expect("serialise");

        let read: Document = toml::from_str(&written).expect("parse");
        let contents = read.into_memory(Path::new("/p"));

        assert_eq!(contents.watches, vec![watch(1, "tokens[pos]")]);
        assert!(
            contents.unknown.is_empty(),
            "a modelled section must not also land in the unmodelled remainder: {:?}",
            contents.unknown,
        );
    }

    #[test]
    fn state_a_newer_lazydap_wrote_survives_a_rewrite_by_this_one() {
        // Deliberately a section this build models *nothing* of. It used to be
        // `[[watches]]`, which stopped being a fair test the moment M16 gave
        // them a typed field of their own.
        let document: Document = toml::from_str(
            "version = 1\n\n[[data_breakpoints]]\nid = \"d1\"\naddress = \"0x7ffd\"\n",
        )
        .expect("parse");
        assert!(
            document.unknown.contains_key("data_breakpoints"),
            "unmodelled sections must be captured, not dropped: {:?}",
            document.unknown,
        );

        let round_tripped = toml::to_string_pretty(&document).expect("serialise");
        assert!(
            round_tripped.contains("data_breakpoints"),
            "got: {round_tripped}",
        );
    }

    #[test]
    fn a_hand_written_launch_config_is_read_with_its_paths_made_absolute() {
        let document: Document = toml::from_str(
            r#"
            version = 1

            [[launch_configs]]
            id = "lc-01"
            name = "main"
            adapter = "codelldb"
            kind = "launch"
            program = "build/hello"
            args = ["--verbose"]
            cwd = "."
            stop_on_entry = true

            [launch_configs.env]
            RUST_LOG = "debug"
            "#,
        )
        .expect("parse");

        let (configs, warnings) = launch_configs(&document.unknown, Path::new("/p"));

        assert!(warnings.is_empty(), "got: {warnings:?}");
        let config = &configs[0];
        assert_eq!(config.name, "main");
        assert_eq!(config.adapter, Some(lazydap_core::AdapterKind::Codelldb));
        assert_eq!(config.program, Some(PathBuf::from("/p/build/hello")));
        assert_eq!(config.cwd, Some(PathBuf::from("/p/.")));
        assert_eq!(config.env["RUST_LOG"], "debug");
        assert!(config.stop_on_entry);
        assert_eq!(config.source, LaunchConfigSource::ProjectState);
        assert_eq!(config.not_runnable(), None);
    }

    #[test]
    fn a_launch_config_this_build_cannot_run_is_still_read() {
        // `delve` rather than `debugpy`, which this build gained at M18. The
        // point of the test is the adapter nobody here can drive, and it needs
        // a name that is still one.
        let document: Document = toml::from_str(
            "[[launch_configs]]\nname = \"api\"\nadapter = \"delve\"\nprogram = \"main.go\"\n",
        )
        .expect("parse");

        let (configs, _) = launch_configs(&document.unknown, Path::new("/p"));
        assert_eq!(configs[0].adapter, None);
        assert_eq!(configs[0].adapter_type, "delve");
        assert!(configs[0].not_runnable().is_some());
    }

    #[test]
    fn a_python_launch_config_is_read_and_runnable() {
        let document: Document = toml::from_str(
            "[[launch_configs]]\nname = \"api\"\nadapter = \"debugpy\"\nprogram = \"app.py\"\n",
        )
        .expect("parse");

        let (configs, _) = launch_configs(&document.unknown, Path::new("/p"));
        assert_eq!(configs[0].adapter, Some(lazydap_core::AdapterKind::Debugpy));
        assert_eq!(configs[0].not_runnable(), None);
    }

    #[test]
    fn one_unreadable_launch_config_does_not_hide_the_others() {
        // A hand-edited file. One typo should cost one entry.
        let document: Document = toml::from_str(
            "[[launch_configs]]\nname = \"bad\"\nkind = \"lunch\"\n\
             \n[[launch_configs]]\nname = \"good\"\nprogram = \"app\"\n",
        )
        .expect("parse");

        let (configs, warnings) = launch_configs(&document.unknown, Path::new("/p"));
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].name, "good");
        assert_eq!(warnings.len(), 1, "and the typo is reported: {warnings:?}");
    }

    #[test]
    fn launch_configs_survive_a_rewrite_that_only_touched_breakpoints() {
        // The reason they are read out of `unknown` rather than modelled: a
        // typed field that serialises as empty would delete them the first
        // time somebody set a breakpoint.
        let document: Document =
            toml::from_str("[[launch_configs]]\nname = \"main\"\nprogram = \"app\"\n")
                .expect("parse");
        let contents = document.into_memory(Path::new("/p"));

        let rewritten = toml::to_string_pretty(&Document::from_memory(&contents, Path::new("/p")))
            .expect("serialise");

        assert!(rewritten.contains("launch_configs"), "got: {rewritten}");
        assert!(rewritten.contains("main"), "got: {rewritten}");
    }

    #[test]
    fn a_write_that_cannot_be_renamed_leaves_no_temporary_behind() {
        let root = std::env::temp_dir().join(format!("lazydap-file-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create");

        // A directory where the file should go: the rename cannot succeed.
        let path = root.join("state.toml");
        std::fs::create_dir(&path).expect("create the blocking directory");

        let error = write(&path, &Document::default()).expect_err("renaming onto a directory");
        assert!(matches!(error, StoreError::Write { .. }), "got: {error}");
        assert!(
            !path.with_extension("toml.tmp").exists(),
            "a failed write must not leave litter next to the state file",
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
