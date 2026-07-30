use crate::client::DaemonClient;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use lazydap_protocol::{IpcConnection, IpcMessage, IpcPayload, LAZYDAP_PROTOCOL_VERSION, Request};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, SystemTime};
use tokio::net::UnixStream;

/// How long to wait for a freshly spawned daemon to bind its socket.
///
/// Ten seconds is generous for a process whose startup is "bind a socket", and
/// costs nothing when things are healthy: the poll below returns the moment
/// the daemon answers. It is the ceiling that matters — on a machine busy
/// enough that `exec` itself takes seconds, a tighter deadline gives up on a
/// daemon that was about to come up, and the user gets a spurious failure.
const SPAWN_DEADLINE: Duration = Duration::from_secs(10);
/// Gap between connect attempts while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(50);
/// A lock file older than this belonged to a client that died mid-spawn.
const STALE_LOCK_AGE: Duration = Duration::from_secs(30);
/// How long to give an outgoing daemon to release its socket.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(5);

/// Connect to this project's daemon, starting one if there isn't a usable one.
///
/// The interesting case is two commands racing from a cold start. Both find no
/// socket, both would spawn, and the second daemon would unlink the first
/// one's socket out from under it — leaving a live daemon nobody can reach. So
/// spawning happens under a lock file, and the loser of the race waits for the
/// winner's daemon rather than starting its own.
pub async fn ensure_daemon_running(instance: &Instance) -> Result<DaemonClient> {
    match DaemonClient::connect(&instance.socket).await {
        Ok(client) => return Ok(client),
        Err(error) if error.label == "VersionMismatch" => {
            // A daemon from another build. Upgrading is cheap: ask it to go,
            // then start one of ours.
            let peer_version = error
                .peer_protocol_version()
                .unwrap_or(LAZYDAP_PROTOCOL_VERSION);
            tracing::info!(
                target: "daemon.spawn",
                peer_version,
                "restarting a daemon from another build",
            );
            shut_down_other_daemon(&instance.socket, peer_version).await?;
        }
        Err(_) => {}
    }

    spawn_and_wait(instance).await
}

async fn spawn_and_wait(instance: &Instance) -> Result<DaemonClient> {
    let deadline = tokio::time::Instant::now() + SPAWN_DEADLINE;

    let Some(_lock) = SpawnLock::acquire(&instance.lock)? else {
        // Somebody else is already starting one. Waiting is both simpler and
        // more correct than starting a second.
        tracing::debug!(target: "daemon.spawn", "another client is starting the daemon; waiting");
        return connect_until(&instance.socket, deadline).await;
    };

    // Re-check under the lock: the winner of the race may have finished while
    // we were waiting for it.
    if let Ok(client) = DaemonClient::connect(&instance.socket).await {
        return Ok(client);
    }

    // Nothing answered, so any socket file here is left over from a daemon
    // that is gone. Removing it is safe *because we hold the lock*.
    if instance.socket.exists() {
        tracing::info!(
            target: "daemon.spawn",
            socket = %instance.socket.display(),
            "removing a stale socket",
        );
        std::fs::remove_file(&instance.socket).map_err(CliError::general)?;
    }

    spawn_detached(instance)?;
    connect_until(&instance.socket, deadline).await
}

/// Start `lazydap daemon` as a detached process that outlives this command.
fn spawn_detached(instance: &Instance) -> Result<()> {
    let exe = std::env::current_exe().map_err(CliError::general)?;
    let log = open_log(&instance.log)?;

    let mut command = std::process::Command::new(&exe);
    command
        .arg("daemon")
        .arg("--instance")
        .arg(&instance.name)
        .stdin(Stdio::null())
        // The daemon logs to stderr; pointing both streams at the log file is
        // what makes those logs land somewhere readable (D015).
        .stdout(Stdio::from(log.try_clone().map_err(CliError::general)?))
        .stderr(Stdio::from(log))
        // Its own process group, so the SIGHUP that arrives when the user
        // closes the terminal this command ran in does not take the daemon
        // with it.
        .process_group(0);

    command.spawn().map_err(|source| {
        CliError::unreachable(anyhow::anyhow!(
            "could not start the daemon ({}): {source}",
            exe.display()
        ))
    })?;

    tracing::info!(
        target: "daemon.spawn",
        instance = %instance.name,
        log = %instance.log.display(),
        "started the daemon",
    );
    Ok(())
}

/// Open the daemon's log, owner-only.
///
/// Failing to open it stops the daemon from starting at all, so it belongs to
/// the same class as any other "could not start or contact the daemon"
/// failure: exit 3, not a generic 1.
///
/// `mode` applies only when the file is created, so an existing log written by
/// an older lazydap keeps its old permissions until the explicit tighten
/// below. Debug logs carry the paths of programs being debugged and whatever
/// they printed.
fn open_log(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| {
            CliError::unreachable(anyhow::anyhow!(
                "cannot open the daemon log at {}: {source}",
                path.display()
            ))
        })?;
    // Not fatal — a readable log beats no daemon — but not silent either: on a
    // filesystem that will not take a chmod, the operator should know the log
    // is readable by others before it fills up with their program's output.
    if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
        tracing::warn!(
            target: "daemon.spawn",
            log = %path.display(),
            %error,
            "could not restrict the daemon log to this user; it may be world-readable",
        );
    }
    Ok(file)
}

/// Retry until the daemon answers or the deadline passes.
async fn connect_until(socket: &Path, deadline: tokio::time::Instant) -> Result<DaemonClient> {
    let mut last_error = None;
    while tokio::time::Instant::now() < deadline {
        match DaemonClient::connect(socket).await {
            Ok(client) => return Ok(client),
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    Err(CliError::unreachable(anyhow::anyhow!(
        "the daemon did not come up within {}s{}",
        SPAWN_DEADLINE.as_secs(),
        last_error
            .map(|error| format!(" ({error})"))
            .unwrap_or_default(),
    )))
}

/// Ask whatever is listening to shut down, and wait for it to let go.
///
/// Used when the running daemon speaks a different protocol version. The
/// request goes out without a handshake — the whole point is that the
/// handshake failed — and the reply is not read, because we may not be able to
/// parse it. The daemon answers `Shutdown` regardless of version for exactly
/// this reason (see `server::handle_message`).
pub(crate) async fn shut_down_other_daemon(socket: &Path, peer_version: u32) -> Result<()> {
    if let Ok(stream) = UnixStream::connect(socket).await {
        let mut connection = IpcConnection::new(stream);
        // Stamped with the *daemon's* version, not ours. A daemon old enough
        // to predate the rule that `Shutdown` is version-exempt will reject
        // anything carrying a version it does not know — including the request
        // that is supposed to replace it — and the first real upgrade would
        // stall. A request wearing its own version number gets through either
        // way.
        let _ = connection
            .send(IpcMessage {
                version: peer_version,
                id: 1,
                payload: IpcPayload::Request(Request::Shutdown { dry_run: false }),
            })
            .await;
    }

    let deadline = tokio::time::Instant::now() + SHUTDOWN_DEADLINE;
    while tokio::time::Instant::now() < deadline {
        if UnixStream::connect(socket).await.is_err() {
            return Ok(());
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }

    Err(CliError::unreachable(anyhow::anyhow!(
        "the running daemon speaks a different protocol version and did not stop when asked; \
         run `lazydap shutdown` or kill it, then try again"
    )))
}

/// Exclusive spawn permission, held for as long as the guard lives.
///
/// `create_new` is `O_EXCL`: exactly one client can create the file, and the
/// rest see `AlreadyExists`. That is all the mutual exclusion a local
/// filesystem needs, and it costs no dependency.
struct SpawnLock {
    path: std::path::PathBuf,
}

impl SpawnLock {
    fn acquire(path: &Path) -> Result<Option<Self>> {
        match Self::try_create(path) {
            Ok(lock) => Ok(Some(lock)),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                if !is_stale(path) {
                    // Somebody else is spawning right now. Wait for them.
                    return Ok(None);
                }

                // The client that held this died before releasing it. Clear it
                // and take it in the same breath: reporting "held" here would
                // send this client off to wait the full deadline for a daemon
                // that nobody is starting, fail, and only succeed on a retry.
                //
                // Claim it by renaming rather than unlinking. Two contenders
                // can both decide the lock is stale, and with `remove_file`
                // the slower one deletes the *fresh* lock the faster one has
                // just created — leaving two spawners again, which is the
                // whole thing this lock exists to prevent. Exactly one rename
                // of a given path can succeed; the loser gets `NotFound` and
                // goes back to waiting, which is now the right answer because
                // the winner is spawning.
                let claimed = path.with_extension(format!("stale-{}", std::process::id()));
                if std::fs::rename(path, &claimed).is_err() {
                    return Ok(None);
                }
                tracing::warn!(
                    target: "daemon.spawn",
                    lock = %path.display(),
                    "took over a stale spawn lock",
                );
                let _ = std::fs::remove_file(&claimed);

                Ok(Self::try_create(path).ok())
            }
            Err(source) => Err(CliError::general(anyhow::anyhow!(
                "cannot take the spawn lock at {}: {source}",
                path.display()
            ))),
        }
    }

    fn try_create(path: &Path) -> std::io::Result<Self> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map(|_| Self {
                path: path.to_path_buf(),
            })
    }
}

impl Drop for SpawnLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .map_err(|_| std::io::Error::other("clock went backwards"))
        })
        .map(|age| age > STALE_LOCK_AGE)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lazydap-spawn-{label}-{}", std::process::id()))
    }

    #[test]
    fn only_one_client_can_hold_the_spawn_lock() {
        let path = temp_path("exclusive");
        let _ = std::fs::remove_file(&path);

        let first = SpawnLock::acquire(&path).expect("acquire");
        assert!(first.is_some(), "the first client should win the race");

        let second = SpawnLock::acquire(&path).expect("acquire");
        assert!(second.is_none(), "the second client must wait, not spawn");

        drop(first);
        let third = SpawnLock::acquire(&path).expect("acquire");
        assert!(third.is_some(), "releasing the lock frees the next client");
    }

    #[test]
    fn a_lock_left_by_a_dead_client_is_cleared_and_taken_at_once() {
        let path = temp_path("stale");
        std::fs::write(&path, b"").expect("write lock");
        // Backdate it past the staleness cutoff.
        set_modified(&path, SystemTime::now() - STALE_LOCK_AGE * 2);

        let lock = SpawnLock::acquire(&path).expect("acquire");
        assert!(
            lock.is_some(),
            "clearing a stale lock and then waiting for a daemon nobody is \
             starting would burn the whole spawn deadline",
        );

        drop(lock);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_second_contender_leaves_the_taken_over_lock_alone() {
        // The dangerous interleaving — two clients that have *both* already
        // classified the lock as stale, where the slower one deletes the fresh
        // lock the faster one just created — needs thread interleaving to
        // reproduce, and is what the rename in `acquire` rules out: only one
        // rename of a path can succeed. What is testable here is the
        // observable consequence: a contender arriving after a takeover finds
        // a live lock and leaves it alone.
        let path = temp_path("contended");
        std::fs::write(&path, b"").expect("write lock");
        set_modified(&path, SystemTime::now() - STALE_LOCK_AGE * 2);

        let winner = SpawnLock::acquire(&path).expect("acquire");
        assert!(winner.is_some(), "the first contender takes the lock over");

        // The second contender is already past its staleness check by now.
        let loser = SpawnLock::acquire(&path).expect("acquire");
        assert!(loser.is_none(), "the second contender must wait");
        assert!(path.exists(), "the winner's fresh lock must still be there",);

        drop(winner);
        let _ = std::fs::remove_file(&path);
    }

    /// `std::fs` cannot set mtime before Rust 1.75's `File::set_modified`,
    /// which is stable and available here.
    fn set_modified(path: &Path, when: SystemTime) {
        let file = OpenOptions::new().write(true).open(path).expect("open");
        file.set_modified(when).expect("set mtime");
    }
}
