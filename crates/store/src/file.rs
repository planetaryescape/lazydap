//! Reading and writing `.lazydap/state.toml`.
//!
//! The on-disk shape is separate from the in-memory one for one reason: paths.
//! On disk they are relative to the project root wherever possible, so the
//! file means the same thing after a `git clone` into a different directory.
//! In memory they are absolute, because that is what gets handed to an
//! adapter.

use super::{Result, StoreError};
use lazydap_core::Breakpoint;
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
    /// Anything a newer lazydap wrote that this one does not model — watches,
    /// launch configs — is carried through a rewrite untouched rather than
    /// deleted. Losing a colleague's launch configs because they run a newer
    /// build would be a nasty way to find out about this file.
    #[serde(flatten)]
    pub unknown: toml::Table,
}

impl Document {
    /// Absolute paths, and the id the next breakpoint should take.
    pub fn into_memory(self, root: &Path) -> (Vec<Breakpoint>, u32) {
        let breakpoints: Vec<Breakpoint> = self
            .breakpoints
            .into_iter()
            .map(|mut breakpoint| {
                breakpoint.source = absolutise(&breakpoint.source, root);
                breakpoint
            })
            .collect();

        let highest = breakpoints
            .iter()
            .map(|breakpoint| breakpoint.id.0 + 1)
            .max()
            .unwrap_or(1);
        let next_id = self.next_breakpoint_id.unwrap_or(1).max(highest);

        (breakpoints, next_id)
    }

    pub fn from_memory(breakpoints: &[Breakpoint], next_id: u32, root: &Path) -> Self {
        let mut breakpoints: Vec<Breakpoint> = breakpoints
            .iter()
            .map(|breakpoint| Breakpoint {
                source: relativise(&breakpoint.source, root),
                ..breakpoint.clone()
            })
            .collect();
        // A stable order keeps the file diffable, which is the point of it
        // being TOML at all (D006).
        breakpoints.sort_by(|a, b| a.source.cmp(&b.source).then(a.line.cmp(&b.line)));

        Self {
            version: SCHEMA_VERSION,
            next_breakpoint_id: Some(next_id),
            breakpoints,
            unknown: toml::Table::new(),
        }
    }
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
    // filesystem.
    let temporary = path.with_extension("toml.tmp");
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

    #[test]
    fn the_file_is_written_in_a_stable_order_so_it_diffs_cleanly() {
        let root = Path::new("/p");
        let document = Document::from_memory(
            &[
                breakpoint(1, "/p/z.c", 5),
                breakpoint(2, "/p/a.c", 20),
                breakpoint(3, "/p/a.c", 2),
            ],
            4,
            root,
        );

        let sources: Vec<String> = document
            .breakpoints
            .iter()
            .map(|breakpoint| format!("{}:{}", breakpoint.source.display(), breakpoint.line))
            .collect();
        assert_eq!(sources, vec!["a.c:2", "a.c:20", "z.c:5"]);
    }

    #[test]
    fn a_file_with_no_next_id_derives_one_past_the_highest_it_has() {
        // Hand-written files will not have the counter in them.
        let document: Document =
            toml::from_str("version = 1\n\n[[breakpoints]]\nid = 7\nsource = \"a.c\"\nline = 1\n")
                .expect("parse");
        let (_, next_id) = document.into_memory(Path::new("/p"));
        assert_eq!(next_id, 8);
    }

    #[test]
    fn a_stale_counter_never_hands_back_an_id_that_is_in_use() {
        let document: Document = toml::from_str(
            "version = 1\nnext_breakpoint_id = 2\n\n\
             [[breakpoints]]\nid = 9\nsource = \"a.c\"\nline = 1\n",
        )
        .expect("parse");
        let (_, next_id) = document.into_memory(Path::new("/p"));
        assert_eq!(
            next_id, 10,
            "the counter cannot go backwards past a live id"
        );
    }

    #[test]
    fn state_a_newer_lazydap_wrote_survives_a_rewrite_by_this_one() {
        let document: Document = toml::from_str(
            "version = 1\n\n[[watches]]\nid = \"w1\"\nexpression = \"tokens[pos]\"\n",
        )
        .expect("parse");
        assert!(
            document.unknown.contains_key("watches"),
            "unmodelled sections must be captured, not dropped: {:?}",
            document.unknown,
        );

        let round_tripped = toml::to_string_pretty(&document).expect("serialise");
        assert!(round_tripped.contains("watches"), "got: {round_tripped}");
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
