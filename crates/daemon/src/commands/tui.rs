//! Entering the TUI.
//!
//! The whole client half is two calls: make sure there is a daemon, then hand
//! the terminal to `lazydap-tui`. That split is what keeps the boundary honest
//! — the TUI crate cannot depend on the daemon, so anything that needs a
//! *process* happens on this side of the call and is handed over as a path.

use crate::auto_spawn::ensure_daemon_running;
use crate::error::{CliError, Result};
use crate::instance::Instance;

/// Start a daemon if there isn't one, then hand over the terminal.
pub async fn run(instance: &Instance) -> Result<()> {
    // The same path every subcommand takes (D003), including the part that
    // replaces a daemon from an older build. Its connection is dropped
    // immediately: the TUI needs a long-lived one of its own, with the event
    // stream attached, and `DaemonClient` is built for one question at a time.
    let _ = ensure_daemon_running(instance).await?;

    lazydap_tui::run(&instance.socket)
        .await
        .map_err(CliError::general)
}
