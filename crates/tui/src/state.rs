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

use crate::panes::scopes::ScopesView;
use crate::panes::source::SourceView;
use crate::panes::stack::StackView;
use lazydap_core::{BreakpointStatus, PauseReason, SessionId, SessionState};
use lazydap_protocol::SessionSummary;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// The highest id that belongs to the connection rather than to the reducer.
///
/// The handshake sends `1` from [`crate::ipc_client`] before the reducer has
/// run at all. Overlapping with it would let a `Pong` be mistaken for the
/// answer to whatever the reducer numbered first.
pub(crate) const RESERVED_IDS: u64 = 1;

#[derive(Default)]
pub struct AppState {
    /// The file on screen, once one has loaded.
    pub(crate) source: Option<SourceView>,
    /// The call stack of the paused program (M12).
    pub(crate) stack: StackView,
    /// The selected frame's scopes and their variables (M13).
    pub(crate) scopes: ScopesView,
    /// The project's breakpoints, as far as the TUI has been told (M14).
    ///
    /// The daemon's copy is the real one; this is what the gutter draws from.
    /// It is kept in step by the answer to every breakpoint request and by
    /// `BreakpointUpdated` events, never by guessing.
    pub(crate) breakpoints: Vec<BreakpointStatus>,
    /// Which pane the keys go to.
    pub(crate) focus: Focus,
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

    /// The id the last request carried. See [`RESERVED_IDS`].
    pub(crate) next_request: u64,
    /// The newest `StackTrace` asked for. Older answers are dropped.
    ///
    /// The same discipline as [`Self::latest_load`], and for a sharper reason:
    /// a stack trace names *frame ids*, and the adapter only keeps those valid
    /// until the program moves. An overtaken one would populate the pane with
    /// handles that no longer address anything, so the next expansion would
    /// fail rather than merely show the wrong thing.
    pub(crate) latest_stack: u64,
    /// The newest `Scopes` asked for, for the same reason.
    pub(crate) latest_scopes: u64,
    /// Which node each in-flight `Variables` request is expanding, since
    /// [`lazydap_protocol::Response::Variables`] does not say.
    pub(crate) pending_variables: BTreeMap<u64, PendingExpansion>,
    /// Breakpoint requests waiting for an answer.
    ///
    /// A refused one leaves the gutter showing an intent the daemon did not
    /// carry out, so a failure with an id in here is answered by asking for
    /// the whole list again rather than by hoping.
    pub(crate) pending_breakpoints: BTreeSet<u64>,
    /// Whether the daemon is reachable, and how the attempts to get it back
    /// are going (M19).
    pub(crate) connection: Connection,
}

impl AppState {
    /// Take the next request id.
    ///
    /// Every request goes through here, which is what makes the ids monotonic
    /// and therefore what makes "this answer has been overtaken" decidable.
    pub(crate) fn next_request_id(&mut self) -> u64 {
        self.next_request = self.next_request.max(RESERVED_IDS) + 1;
        self.next_request
    }

    /// Whether `attempt` is the reconnection currently being waited on.
    ///
    /// Read by the loop before it installs a connection an attempt handed back.
    /// One that lost its race would otherwise replace a working connection with
    /// an unsubscribed one, and every request after that would go somewhere
    /// nobody is listening.
    pub(crate) fn is_awaiting(&self, attempt: u32) -> bool {
        self.connection == Connection::Reconnecting { attempt }
    }

    /// Whether a request stands any chance of reaching the daemon.
    pub(crate) fn is_reachable(&self) -> bool {
        self.connection == Connection::Connected
    }
}

/// Which pane has the keys.
///
/// `Tab` cycles in this order and `BackTab` the other way. The source pane is
/// first because it is where a session starts and where `b` works.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Focus {
    #[default]
    Source,
    Stack,
    Scopes,
}

impl Focus {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Source => Self::Stack,
            Self::Stack => Self::Scopes,
            Self::Scopes => Self::Source,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::Source => Self::Scopes,
            Self::Stack => Self::Source,
            Self::Scopes => Self::Stack,
        }
    }
}

/// How the TUI is getting on with the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Connection {
    #[default]
    Connected,
    /// The daemon went away and the TUI is trying to get it back. `attempt`
    /// counts from 1 and is what the backoff is computed from.
    ///
    /// There is no state past this one, on purpose. Every attempt can *start* a
    /// daemon rather than only wait for one, so "cannot reach it" is never
    /// final — the machine it would run on is the one the TUI is already on.
    /// Giving up after a fixed number of tries meant a daemon that became
    /// startable a minute later was never reached, on a screen the user was
    /// still sitting in front of.
    Reconnecting { attempt: u32 },
}

/// An expansion the TUI is waiting on, and the tree it was asked about.
///
/// The path alone is not enough. A path addresses a *position* — "the fourth
/// child of the first scope" — and the whole tree under it is replaced whenever
/// the selected frame changes. Without the generation, an expansion issued
/// against the caller's tree and answered after the callee's tree had arrived
/// would fill the callee's node with the caller's values: the same position, a
/// different frame, and nothing on screen to say so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingExpansion {
    /// The id of the `Scopes` request whose answer built the tree this path was
    /// resolved against.
    pub(crate) scopes: u64,
    pub(crate) path: Vec<usize>,
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
