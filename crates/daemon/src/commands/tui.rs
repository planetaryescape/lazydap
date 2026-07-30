//! Entering the TUI.
//!
//! The whole client half is two calls: make sure there is a daemon, then hand
//! the terminal to `lazydap-tui`. That split is what keeps the boundary honest
//! — the TUI crate cannot depend on the daemon, so anything that needs a
//! *process* happens on this side of the call and is handed over as a path.

use crate::auto_spawn::ensure_daemon_running;
use crate::error::{CliError, Result};
use crate::instance::Instance;
use std::sync::Arc;

/// Start a daemon if there isn't one, then hand over the terminal.
pub async fn run(instance: &Instance) -> Result<()> {
    // The same path every subcommand takes (D003), including the part that
    // replaces a daemon from an older build. Its connection is dropped
    // immediately: the TUI needs a long-lived one of its own, with the event
    // stream attached, and `DaemonClient` is built for one question at a time.
    let _ = ensure_daemon_running(instance).await?;

    lazydap_tui::run(&instance.socket, starter(instance.clone()))
        .await
        .map_err(CliError::general)
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
