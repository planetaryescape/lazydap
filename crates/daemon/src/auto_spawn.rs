use crate::client::DaemonClient;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use lazydap_protocol::{IpcConnection, IpcMessage, Request};
use std::fs::{File, OpenOptions};
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
            tracing::info!(target: "daemon.spawn", "restarting a daemon from another build");
            shut_down_other_daemon(&instance.socket).await?;
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

fn open_log(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|source| {
            CliError::general(anyhow::anyhow!(
                "cannot open the daemon log at {}: {source}",
                path.display()
            ))
        })
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
/// parse it.
async fn shut_down_other_daemon(socket: &Path) -> Result<()> {
    if let Ok(stream) = UnixStream::connect(socket).await {
        let mut connection = IpcConnection::new(stream);
        let _ = connection
            .send(IpcMessage::request(1, Request::Shutdown))
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
        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_) => Ok(Some(Self {
                path: path.to_path_buf(),
            })),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                if is_stale(path) {
                    // The client that held this died before releasing it.
                    tracing::warn!(
                        target: "daemon.spawn",
                        lock = %path.display(),
                        "removing a stale spawn lock",
                    );
                    let _ = std::fs::remove_file(path);
                }
                Ok(None)
            }
            Err(source) => Err(CliError::general(anyhow::anyhow!(
                "cannot take the spawn lock at {}: {source}",
                path.display()
            ))),
        }
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
    fn a_lock_left_by_a_dead_client_is_cleared() {
        let path = temp_path("stale");
        std::fs::write(&path, b"").expect("write lock");
        // Backdate it past the staleness cutoff.
        let old = SystemTime::now() - STALE_LOCK_AGE * 2;
        set_modified(&path, old);

        assert!(
            SpawnLock::acquire(&path).expect("acquire").is_none(),
            "the first attempt reports the lock as held and clears it",
        );
        assert!(
            SpawnLock::acquire(&path).expect("acquire").is_some(),
            "the next attempt gets the freed lock",
        );
        let _ = std::fs::remove_file(&path);
    }

    /// `std::fs` cannot set mtime before Rust 1.75's `File::set_modified`,
    /// which is stable and available here.
    fn set_modified(path: &Path, when: SystemTime) {
        let file = OpenOptions::new().write(true).open(path).expect("open");
        file.set_modified(when).expect("set mtime");
    }
}
