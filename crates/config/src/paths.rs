//! File locations, and the instance name that keys them.
//!
//! One daemon per project (D010). The project is identified by walking up from
//! the working directory; the resulting instance name goes into every path so
//! two projects never share a socket.

use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Environment override for the instance name, per D010.
pub const INSTANCE_ENV: &str = "LAZYDAP_INSTANCE";

/// Environment overrides for the two directories. Tests and the odd sandboxed
/// deployment need to put lazydap's files somewhere other than the user's real
/// directories, and an env var beats a config file nobody can write yet.
pub const RUNTIME_DIR_ENV: &str = "LAZYDAP_RUNTIME_DIR";
pub const DATA_DIR_ENV: &str = "LAZYDAP_DATA_DIR";

/// Markers that identify a project root, in priority order (O01/D024).
///
/// Each tier is searched all the way up before the next one is tried, so an
/// explicit `.lazydap/` anywhere above the working directory beats a nearer
/// `.git/` — which is what makes it usable as an override in a monorepo or a
/// submodule.
const ROOT_MARKERS: [&[Marker]; 3] = [
    &[Marker {
        name: ".lazydap",
        shape: Shape::Directory,
        at_home: true,
    }],
    &[Marker {
        name: ".git",
        shape: Shape::Either,
        at_home: false,
    }],
    &[
        Marker {
            name: "Cargo.toml",
            shape: Shape::Either,
            at_home: false,
        },
        Marker {
            name: "package.json",
            shape: Shape::Either,
            at_home: false,
        },
        Marker {
            name: "pyproject.toml",
            shape: Shape::Either,
            at_home: false,
        },
    ],
];

/// One thing whose presence in a directory makes it a project root.
struct Marker {
    name: &'static str,
    shape: Shape,
    /// Whether finding it in the *home directory* counts.
    ///
    /// For everything but `.lazydap/` it does not. Plenty of people keep their
    /// dotfiles in a git repository, or a stray `Cargo.toml` in `~`, and
    /// letting either mark `$HOME` as a project makes every unmarked directory
    /// beneath it — which is most of them — share one root, one daemon and one
    /// `~/.lazydap/state.toml`. Asking for `$HOME` explicitly with a
    /// `.lazydap/` directory still works.
    at_home: bool,
}

/// What has to be at the marker's path for it to count.
enum Shape {
    /// A directory and nothing else. A *file* called `.lazydap` is not
    /// lazydap's state directory, and treating it as one puts the project root
    /// somewhere the state file can never be written.
    Directory,
    /// A file or a directory: `.git` is a plain file in a worktree or a
    /// submodule, and the manifests are files.
    Either,
}

impl Marker {
    fn found_in(&self, dir: &Path) -> bool {
        let path = dir.join(self.name);
        match self.shape {
            Shape::Directory => path.is_dir(),
            Shape::Either => path.exists(),
        }
    }
}

/// Longest socket path we will attempt to bind.
///
/// `sun_path` is 104 bytes on macOS and 108 on Linux, and the failure mode for
/// overrunning it is a truncated path that binds somewhere unexpected. Refuse
/// early, with the length in the message.
const MAX_SOCKET_PATH_BYTES: usize = 100;

/// Instance names end up in a filename, so keep them short and boring.
const MAX_SLUG_CHARS: usize = 16;
const HASH_HEX_CHARS: usize = 12;

#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    #[error("cannot locate the user's home directory")]
    NoHomeDirectory,

    #[error("cannot locate a data directory for lazydap")]
    NoDataDirectory,

    #[error("directory {path} is owned by uid {owner}, not by uid {expected}")]
    RuntimeDirNotOurs {
        path: PathBuf,
        owner: u32,
        expected: u32,
    },

    #[error("refusing to use {path}: {reason}")]
    UnsafeDirectory { path: PathBuf, reason: String },

    #[error("socket path is {len} bytes, over the {MAX_SOCKET_PATH_BYTES}-byte limit: {path}")]
    SocketPathTooLong { len: usize, path: PathBuf },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PathsError>;

/// The project root for `start`, per O01/D024: `.lazydap/`, then `.git/`, then
/// a language manifest, then `start` itself.
///
/// The walk never climbs past the home directory. See [`Marker::at_home`].
pub fn project_root(start: &Path) -> PathBuf {
    project_root_below(start, dirs::home_dir().as_deref())
}

/// [`project_root`] with the ceiling passed in, so it can be tested without
/// moving the real `$HOME`.
fn project_root_below(start: &Path, home: Option<&Path>) -> PathBuf {
    for markers in ROOT_MARKERS {
        if let Some(root) = nearest_ancestor_containing(start, markers, home) {
            return root;
        }
    }
    start.to_path_buf()
}

/// The instance name for `start`, in precedence order: an explicit override
/// (the `--instance` flag), then `LAZYDAP_INSTANCE`, then the project root.
pub fn instance_name(start: &Path, explicit: Option<&str>) -> String {
    if let Some(name) = explicit.filter(|name| !name.trim().is_empty()) {
        return slug(name);
    }
    match std::env::var(INSTANCE_ENV) {
        Ok(name) if !name.trim().is_empty() => slug(&name),
        _ => instance_for_root(&project_root(start)),
    }
}

/// A stable, filesystem-safe name for one project root.
///
/// The readable half is for humans reading `ps` output; the hash half is what
/// actually keeps two projects apart.
pub fn instance_for_root(root: &Path) -> String {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "root".to_string());
    let readable: String = slug(&name).chars().take(MAX_SLUG_CHARS).collect();
    format!(
        "{readable}-{:0width$x}",
        fnv1a64(root.as_os_str().as_encoded_bytes()) & hash_mask(),
        width = HASH_HEX_CHARS,
    )
}

/// The directory holding sockets and lock files, created 0700 if missing.
///
/// `dirs::runtime_dir()` returns `None` on macOS (there is no
/// `XDG_RUNTIME_DIR`), so fall back to a per-uid directory under `/tmp`.
///
/// Deliberately `/tmp` rather than `std::env::temp_dir()`: on macOS the latter
/// is a per-user path some fifty characters long, which on its own eats half
/// the 104-byte budget a Unix socket path gets. `/tmp` is short, always
/// present, and the per-uid subdirectory below is created 0700 and
/// ownership-checked, so sharing the parent with everyone else costs nothing.
pub fn runtime_dir() -> Result<PathBuf> {
    let dir = match env_path(RUNTIME_DIR_ENV) {
        Some(dir) => dir,
        None => match dirs::runtime_dir() {
            Some(dir) => dir.join("lazydap"),
            None => PathBuf::from("/tmp").join(format!("lazydap-{}", current_uid()?)),
        },
    };
    ensure_private_dir(&dir)?;
    Ok(dir)
}

/// The directory holding PID and log files.
///
/// Owner-only for the same reason as the runtime directory: daemon logs record
/// the paths of programs being debugged and whatever they printed, which is
/// nobody else's business.
pub fn data_dir() -> Result<PathBuf> {
    let dir = match env_path(DATA_DIR_ENV) {
        Some(dir) => dir,
        None => dirs::data_dir()
            .ok_or(PathsError::NoDataDirectory)?
            .join("lazydap"),
    };
    ensure_private_dir(&dir)?;
    Ok(dir)
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

/// The Unix socket the daemon binds and clients connect to.
pub fn socket_path(instance: &str) -> Result<PathBuf> {
    let path = runtime_dir()?.join(format!("lazydap-{instance}.sock"));
    let len = path.as_os_str().as_encoded_bytes().len();
    if len > MAX_SOCKET_PATH_BYTES {
        return Err(PathsError::SocketPathTooLong { len, path });
    }
    Ok(path)
}

/// The file clients race for before spawning a daemon.
pub fn lock_path(instance: &str) -> Result<PathBuf> {
    Ok(runtime_dir()?.join(format!("lazydap-{instance}.lock")))
}

/// Where the running daemon records its PID (D003).
pub fn pid_path(instance: &str) -> Result<PathBuf> {
    Ok(data_dir()?.join(format!("lazydap-{instance}.pid")))
}

/// Where a backgrounded daemon's logs go (D015).
pub fn log_path(instance: &str) -> Result<PathBuf> {
    Ok(data_dir()?.join(format!("lazydap-{instance}.log")))
}

/// Nearest ancestor of `start` (inclusive) that contains any of `markers`,
/// stopping at the home directory.
///
/// Nothing above `$HOME` is ever a project root: `~/Documents` and
/// `~/code/whatever` have nothing in common but the user they belong to, and
/// the first marker found above home would make them one project.
fn nearest_ancestor_containing(
    start: &Path,
    markers: &[Marker],
    home: Option<&Path>,
) -> Option<PathBuf> {
    for dir in start.ancestors() {
        let at_home = home == Some(dir);
        if markers
            .iter()
            .any(|marker| (!at_home || marker.at_home) && marker.found_in(dir))
        {
            return Some(dir.to_path_buf());
        }
        if at_home {
            return None;
        }
    }
    None
}

/// Create `dir` owner-only, and refuse to use anything that is not already a
/// directory we own privately.
///
/// The threat is concrete. `/tmp` is world-writable, so anyone on the machine
/// can create `/tmp/lazydap-$UID` before we do. If they make it a *symlink* to
/// somewhere they control, a check that follows the link sees a directory we
/// own with the right mode and passes — and lazydap then binds its control
/// socket inside a directory the attacker can swap out from under it. A fake
/// daemon on that socket accepts `launch`, which is arbitrary code execution
/// under the user's own uid.
///
/// So: `symlink_metadata` (lstat, does not follow), and reject a symlink
/// outright rather than inspecting its target. There is still a TOCTOU gap
/// between this check and the bind; closing it properly needs `openat`-style
/// directory handles, which is not worth a dependency for a local-only socket
/// whose parent an attacker must already share.
fn ensure_private_dir(dir: &Path) -> Result<()> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) => validate_private_dir(dir, &metadata),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            // Create with the mode set, not chmod-after-create: the window
            // between the two would leave the directory briefly world-readable.
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;

            // Check again. Between the lstat above and this create, an
            // attacker can win the race and leave a symlink-to-directory in
            // place — which `create_dir_all` accepts without complaint,
            // because the path does resolve to a directory. Re-validating
            // afterwards does not close the race, but it does turn it from
            // exploitable into detected: whatever is at this path now has to
            // pass the same checks as a directory we found already there.
            let metadata = fs::symlink_metadata(dir)?;
            validate_private_dir(dir, &metadata)
        }
        Err(source) => Err(source.into()),
    }
}

fn validate_private_dir(dir: &Path, metadata: &fs::Metadata) -> Result<()> {
    if metadata.file_type().is_symlink() {
        return Err(PathsError::UnsafeDirectory {
            path: dir.to_path_buf(),
            reason: "it is a symlink, and following it would let whoever created it \
                     choose where lazydap puts its socket"
                .to_string(),
        });
    }
    if !metadata.is_dir() {
        return Err(PathsError::UnsafeDirectory {
            path: dir.to_path_buf(),
            reason: "it exists but is not a directory".to_string(),
        });
    }

    let expected = current_uid()?;
    let owner = metadata.uid();
    if owner != expected {
        return Err(PathsError::RuntimeDirNotOurs {
            path: dir.to_path_buf(),
            owner,
            expected,
        });
    }
    if metadata.permissions().mode() & 0o777 != 0o700 {
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// This process's user id.
///
/// `std` has no `getuid()` and libc is not in the dependency budget, but the
/// home directory is ours by construction, so its owner is us.
fn current_uid() -> Result<u32> {
    let home = dirs::home_dir().ok_or(PathsError::NoHomeDirectory)?;
    Ok(fs::metadata(home)?.uid())
}

/// Lower-case and filesystem-safe.
///
/// Deliberately idempotent — `slug(slug(x)) == slug(x)`. A client resolves the
/// instance name and passes it to the daemon it spawns, which slugs it again;
/// anything lossy here (truncation, say) would hand the two processes different
/// names and different sockets.
fn slug(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed
    }
}

fn hash_mask() -> u64 {
    // Four bits per hex digit; HASH_HEX_CHARS is well under 16 so this cannot
    // overflow a u64.
    u64::MAX >> (64 - HASH_HEX_CHARS * 4)
}

/// FNV-1a, 64-bit.
///
/// Not `DefaultHasher`: its output is explicitly not stable across std
/// releases, and an instance name that changes when the toolchain does would
/// orphan a running daemon behind a socket nobody looks for any more.
fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut hash = OFFSET_BASIS;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A directory under the system temp dir that deletes itself.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "lazydap-paths-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn mkdirs(&self, relative: &str) -> PathBuf {
            let path = self.0.join(relative);
            fs::create_dir_all(&path).expect("create dir");
            path
        }

        fn touch(&self, relative: &str) {
            let path = self.0.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(&path, b"").expect("write file");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_lazydap_directory_wins_over_a_nearer_git_directory() {
        let temp = TempDir::new("markers");
        temp.mkdirs(".lazydap");
        let nested = temp.mkdirs("crates/inner");
        temp.mkdirs("crates/inner/.git");

        assert_eq!(project_root(&nested), temp.path());
    }

    #[test]
    fn a_git_directory_wins_over_a_language_manifest() {
        let temp = TempDir::new("git");
        temp.mkdirs(".git");
        let nested = temp.mkdirs("crates/inner");
        temp.touch("crates/inner/Cargo.toml");

        assert_eq!(project_root(&nested), temp.path());
    }

    #[test]
    fn the_nearest_manifest_wins_when_there_is_no_repository() {
        let temp = TempDir::new("manifest");
        temp.touch("package.json");
        let nested = temp.mkdirs("services/api");
        temp.touch("services/api/pyproject.toml");

        assert_eq!(project_root(&nested), nested);
    }

    #[test]
    fn an_unmarked_directory_is_its_own_root() {
        let temp = TempDir::new("bare");
        let nested = temp.mkdirs("nothing/here");

        assert_eq!(project_root(&nested), nested);
    }

    #[test]
    fn a_file_called_lazydap_is_not_a_project_root() {
        // `.lazydap` is a directory with a state file in it. A file of that
        // name is somebody's note, and stopping the walk there would name a
        // root the state file can never be written under.
        let temp = TempDir::new("lazydapfile");
        temp.mkdirs(".git");
        let nested = temp.mkdirs("src/inner");
        temp.touch("src/.lazydap");

        assert_eq!(project_root(&nested), temp.path());
    }

    #[test]
    fn a_git_repository_of_dotfiles_does_not_make_home_everyone_s_project() {
        // The reported shape: `~/.git` from a dotfiles repository, and an
        // unmarked directory below it. Sharing one root means sharing one
        // daemon and one `~/.lazydap/state.toml` across everything the user
        // does.
        let temp = TempDir::new("dotfiles");
        let home = temp.mkdirs("home");
        fs::create_dir_all(home.join(".git")).expect("create dotfiles repo");
        let scratch = temp.mkdirs("home/scratch/notes");

        assert_eq!(project_root_below(&scratch, Some(&home)), scratch);
    }

    #[test]
    fn home_is_a_project_root_when_it_is_asked_for_by_name() {
        let temp = TempDir::new("homelazydap");
        let home = temp.mkdirs("home");
        fs::create_dir_all(home.join(".lazydap")).expect("create the marker");
        let nested = temp.mkdirs("home/scratch");

        assert_eq!(project_root_below(&nested, Some(&home)), home);
    }

    #[test]
    fn the_walk_never_climbs_above_home() {
        let temp = TempDir::new("ceiling");
        // A marker *above* the home directory, which nothing under home may
        // adopt: two projects that share only `/Users` are not one project.
        temp.touch("Cargo.toml");
        let home = temp.mkdirs("home");
        let nested = temp.mkdirs("home/code/thing");

        assert_eq!(project_root_below(&nested, Some(&home)), nested);
        assert_eq!(project_root_below(&home, Some(&home)), home);
    }

    #[test]
    fn a_marked_project_under_home_is_still_found() {
        let temp = TempDir::new("underhome");
        let home = temp.mkdirs("home");
        let project = temp.mkdirs("home/code/thing");
        temp.touch("home/code/thing/Cargo.toml");
        let nested = temp.mkdirs("home/code/thing/src/deep");

        assert_eq!(project_root_below(&nested, Some(&home)), project);
    }

    #[test]
    fn instance_names_are_short_readable_and_unique_per_root() {
        let first = instance_for_root(Path::new("/Users/someone/code/lazydap"));
        let second = instance_for_root(Path::new("/Users/someone/other/lazydap"));

        assert!(first.starts_with("lazydap-"), "got: {first}");
        assert_eq!(
            first.len(),
            "lazydap-".len() + HASH_HEX_CHARS,
            "got: {first}"
        );
        assert_ne!(
            first, second,
            "same basename, different path: the hash must separate them",
        );
        assert_eq!(
            first,
            instance_for_root(Path::new("/Users/someone/code/lazydap")),
            "the same root must always produce the same instance",
        );
    }

    #[test]
    fn resolving_an_instance_name_twice_is_a_no_op() {
        // The client resolves a name, then passes it to the daemon it spawns,
        // which resolves it again. If that is lossy they bind different sockets.
        let once = instance_for_root(Path::new("/Users/someone/code/a-very-long-project-name"));
        let twice = instance_name(Path::new("/nowhere"), Some(&once));
        assert_eq!(once, twice, "got: {once} then {twice}");
    }

    #[test]
    fn awkward_directory_names_still_produce_a_usable_instance() {
        let instance = instance_for_root(Path::new("/tmp/My Project (v2)!"));
        assert!(
            instance
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "got: {instance}",
        );
    }

    #[test]
    fn the_socket_path_carries_the_instance_name() {
        let path = socket_path("lazydap-abcdef123456").expect("socket path");
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("lazydap-lazydap-abcdef123456.sock"),
            "got: {}",
            path.display(),
        );
    }

    #[test]
    fn an_over_long_socket_path_is_refused_rather_than_truncated() {
        let err = socket_path(&"x".repeat(MAX_SOCKET_PATH_BYTES))
            .expect_err("an over-long instance must not bind");
        assert!(
            matches!(err, PathsError::SocketPathTooLong { .. }),
            "got: {err}",
        );
    }

    #[test]
    fn a_symlinked_directory_is_refused_rather_than_followed() {
        // Somebody else got to /tmp first and pointed our directory at one
        // they control. Following it would put the daemon's control socket
        // wherever they like.
        let temp = TempDir::new("symlink");
        let target = temp.mkdirs("theirs");
        let link = temp.path().join("ours");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let err = ensure_private_dir(&link).expect_err("a symlink must not be accepted");
        assert!(
            matches!(err, PathsError::UnsafeDirectory { .. }),
            "got: {err}",
        );
    }

    #[test]
    fn a_file_where_the_directory_should_be_is_refused() {
        let temp = TempDir::new("notdir");
        temp.touch("ours");

        let err = ensure_private_dir(&temp.path().join("ours"))
            .expect_err("a plain file must not be accepted");
        assert!(
            matches!(err, PathsError::UnsafeDirectory { .. }),
            "got: {err}",
        );
    }

    #[test]
    fn a_directory_created_from_scratch_is_owner_only_immediately() {
        let temp = TempDir::new("fresh");
        let dir = temp.path().join("nested/runtime");

        ensure_private_dir(&dir).expect("create");

        let mode = fs::metadata(&dir).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "got mode: {:o}", mode & 0o777);
    }

    #[test]
    fn loose_permissions_on_our_own_directory_are_tightened() {
        let temp = TempDir::new("loose");
        let dir = temp.mkdirs("runtime");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).expect("loosen");

        ensure_private_dir(&dir).expect("accept our own directory");

        let mode = fs::metadata(&dir).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "got mode: {:o}", mode & 0o777);
    }

    #[test]
    fn the_runtime_directory_is_owner_only() {
        let dir = runtime_dir().expect("runtime dir");
        let mode = fs::metadata(&dir).expect("metadata").permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "got mode: {:o}", mode & 0o777);
    }
}
