//! Entering the TUI.
//!
//! The whole client half is one call: `lazydap-tui` owns the terminal, and
//! this crate owns the process. That split is what keeps the boundary honest —
//! the TUI crate cannot depend on the daemon, so anything that needs a process
//! (spawning one, resolving an instance) happens on this side of the call and
//! is handed over as data.

use crate::error::{CliError, Result};

/// Hand the terminal to the TUI.
pub async fn run() -> Result<()> {
    lazydap_tui::run().await.map_err(CliError::general)
}
