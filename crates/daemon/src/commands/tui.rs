//! Entering the TUI.
//!
//! The whole client half is two calls: make sure there is a daemon, then hand
//! the terminal to `lazydap-tui`. That split is what keeps the boundary honest
//! — the TUI crate cannot depend on the daemon, so anything that needs a
//! *process* happens on this side of the call and is handed over as a path.

use crate::auto_spawn::{ensure_daemon_running, open_log};
use crate::error::{CliError, Result};
use crate::instance::Instance;
use std::sync::Arc;

/// Start a daemon if there isn't one, then hand over the terminal.
pub async fn run(instance: &Instance) -> Result<()> {
    // Before anything can log: the TUI is about to take this terminal over, and
    // a log line written to it lands *on the panes*.
    send_logs_to_the_file(instance);

    // The same path every subcommand takes (D003), including the part that
    // replaces a daemon from an older build. Its connection is dropped
    // immediately: the TUI needs a long-lived one of its own, with the event
    // stream attached, and `DaemonClient` is built for one question at a time.
    let _ = ensure_daemon_running(instance).await?;

    lazydap_tui::run(&instance.socket, starter(instance.clone()))
        .await
        .map_err(CliError::general)
}

/// Point this process's logs at the instance log file instead of stderr.
///
/// Every other subcommand prints and exits, so stderr is exactly where its
/// warnings belong. The TUI is the one command that *takes the terminal over*,
/// and stderr still goes to that terminal — through the alternate screen,
/// straight over whatever is drawn there. Found in a pseudo-terminal run of
/// M17: a mistyped expression is a refused request, the refusal is logged at
/// `warn`, and the log line lands across the panes. An out-of-scope watch does
/// the same thing on every single step, which is the ordinary case rather than
/// the exceptional one.
///
/// The lines are kept rather than dropped: `lazydap logs` already reads this
/// file, so a TUI that misbehaves can still be diagnosed. If the file cannot be
/// opened the subscriber is simply not installed — losing the logs is a great
/// deal better than writing them onto the user's screen.
fn send_logs_to_the_file(instance: &Instance) {
    use tracing_subscriber::EnvFilter;

    let Ok(file) = open_log(&instance.log) else {
        return;
    };
    let filter = EnvFilter::try_from_env(crate::LOG_ENV)
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("warn"));

    // `Arc<File>` is a `MakeWriter` because `&File` is a `Write`: every event
    // appends, and the file is shared rather than reopened per line.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Arc::new(file))
        // Never: this is a file, and escape codes in it would make
        // `lazydap logs` unreadable.
        .with_ansi(false)
        .try_init();
}

/// The same "make sure there is a daemon" the TUI can run again later (M19).
///
/// Handed over as a callback because `lazydap-tui` may not depend on this
/// crate (`ARCHITECTURE.md`), and spawning a daemon is a *process* operation
/// that lives on this side of that line. A daemon shut down from another
/// terminal is therefore started again by exactly the code that started the
/// first one — including its spawn lock, so a TUI and a CLI command racing to
/// revive one do not end up with two.
fn starter(instance: Instance) -> lazydap_tui::EnsureDaemon {
    Arc::new(move || {
        let instance = instance.clone();
        Box::pin(async move {
            ensure_daemon_running(&instance)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    })
}
