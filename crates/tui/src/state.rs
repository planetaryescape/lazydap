//! Everything the TUI knows.
//!
//! One struct, owned by the loop and handed to the reducer by value. Fields
//! are crate-visible rather than public: the reducer is the only thing that
//! writes them (D012), and the crate boundary is what enforces that.
//!
//! Nothing here is a parallel vocabulary. The session's state is the protocol's
//! [`SessionState`], the stop reason is its [`PauseReason`], the id is its
//! [`SessionId`] — a TUI-shaped copy of any of them would be one more thing to
//! keep in step with the daemon, and the daemon is the source of truth.

use crate::panes::source::SourceView;
use lazydap_core::{PauseReason, SessionId, SessionState};
use lazydap_protocol::SessionSummary;
use std::path::PathBuf;

#[derive(Default)]
pub struct AppState {
    /// The file on screen, once one has loaded.
    pub(crate) source: Option<SourceView>,
    /// A message that replaces the usual position readout in the status row.
    ///
    /// For things the user has to be told and cannot infer from the screen — a
    /// file that would not open, a request the daemon refused, a daemon that
    /// went away.
    pub(crate) notice: Option<String>,
    /// The first half of a two-key sequence, i.e. the `g` of `gg`.
    ///
    /// In the state rather than in the loop because it *is* state: whether the
    /// next `g` means "go to the top" depends on what came before it.
    pub(crate) awaiting_g: bool,
    /// The daemon's session, as far as the TUI has been told.
    pub(crate) session: Option<SessionSnapshot>,
    /// Where the program is stopped, straight from the daemon.
    ///
    /// Kept separately from the source pane's marker because the two can
    /// legitimately disagree for a moment: the daemon can say "line 19 of
    /// main.c" before main.c has finished loading. This is the truth; the
    /// marker is what has been drawn of it.
    pub(crate) location: Option<Location>,
    /// The id of the most recent file read asked for.
    ///
    /// Reads finish in whatever order the filesystem manages. Two stops in
    /// quick succession can have the *first* file arrive last, and without a
    /// way to tell them apart the older answer wins and the pane shows a file
    /// the program has already left. Only an answer carrying this id is
    /// still wanted.
    pub(crate) latest_load: u64,
}

/// As much of the session as the TUI has been told about.
#[derive(Debug, Clone)]
pub(crate) struct SessionSnapshot {
    pub(crate) id: SessionId,
    pub(crate) state: SessionState,
    /// The thread that stopped last. `None` means "whichever one" — which the
    /// daemon resolves, exactly as it does for `lazydap continue` with no
    /// `--thread`.
    pub(crate) thread_id: Option<i64>,
    pub(crate) reason: Option<PauseReason>,
}

impl SessionSnapshot {
    /// What a `Status` reply knows: the session exists and is in some state.
    /// The thread and the reason arrive with the next stop.
    pub(crate) fn from_summary(summary: &SessionSummary) -> Self {
        Self {
            id: summary.session_id,
            state: summary.state,
            thread_id: None,
            reason: None,
        }
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.state == SessionState::Paused
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Location {
    pub(crate) path: PathBuf,
    pub(crate) line: u32,
}
