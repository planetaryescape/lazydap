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

use crate::panes::input::TextInput;
use crate::panes::repl::ReplView;
use crate::panes::scopes::ScopesView;
use crate::panes::source::SourceView;
use crate::panes::stack::StackView;
use crate::panes::watches::WatchesView;
use lazydap_core::{BreakpointStatus, PauseReason, SessionId, SessionState, WatchId};
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
    /// The project's watch expressions, and what each came to at this stop
    /// (M16). The expressions outlive the session; the values do not.
    pub(crate) watches: WatchesView,
    /// Ad-hoc expressions and their answers (M17).
    pub(crate) repl: ReplView,
    /// A prompt that has taken the keyboard, if one is open.
    ///
    /// While it is `Some`, every printable key belongs to it — which is the
    /// whole point. Without that, typing an expression containing `q` would
    /// quit the TUI.
    pub(crate) modal: Option<Modal>,
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
    /// The `d` of `dd`, which removes the selected watch (M16). Separate from
    /// [`Self::awaiting_g`] so that `gd` and `dg` are both nothing rather than
    /// one arming the other.
    pub(crate) awaiting_d: bool,
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
    /// Which watch each in-flight evaluation is for, and which round it
    /// belongs to.
    ///
    /// [`lazydap_protocol::Response::Evaluated`] is a bare value with nothing
    /// in it saying what was asked — not the expression, not the frame. The id
    /// is the only thing that can tell one watch's answer from another's, or
    /// from a REPL submission's, since all three come back as the same variant.
    pub(crate) pending_watches: BTreeMap<u64, PendingWatch>,
    /// Which REPL entry each in-flight evaluation belongs to, by the entry's
    /// own id rather than its position: the scrollback is trimmed from the
    /// front, so a position is not a stable name for an entry.
    pub(crate) pending_repl: BTreeMap<u64, u64>,
    /// The `WatchList` currently in flight, if there is one.
    ///
    /// At most one, ever. A local mutation is answered *and* announced — the
    /// daemon tells every subscriber, including the client that asked — so
    /// removing three watches used to produce four full refreshes, each one
    /// re-evaluating every remaining expression, all queued to the single
    /// adapter ahead of whatever `continue` the user pressed next.
    pub(crate) pending_watch_list: Option<u64>,
    /// Whether something asked for the list while one was already in flight.
    ///
    /// Consumed when that one lands, which collapses any number of overlapping
    /// announcements into exactly one more request — and still cannot miss a
    /// change, because the flag is only cleared by a fetch that started after
    /// the change was seen.
    pub(crate) watch_list_dirty: bool,
    /// Whether the daemon is reachable, and how the attempts to get it back
    /// are going (M19).
    pub(crate) connection: Connection,
    /// Which rung of the reconnection ladder the TUI is on, kept across a
    /// connection that came up and did not last (D-WP6-1).
    ///
    /// Zero while the daemon has been reachable long enough to be trusted, so
    /// the next time it goes away the retries start at 250ms again.
    pub(crate) reconnect_attempt: u32,
    /// How many ticks the current connection has lasted, up to the point where
    /// it has proved itself. Meaningless while reconnecting.
    pub(crate) connected_for: u32,
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
    Watches,
    Repl,
}

impl Focus {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Source => Self::Stack,
            Self::Stack => Self::Scopes,
            Self::Scopes => Self::Watches,
            Self::Watches => Self::Repl,
            Self::Repl => Self::Source,
        }
    }

    pub(crate) fn previous(self) -> Self {
        match self {
            Self::Source => Self::Repl,
            Self::Stack => Self::Source,
            Self::Scopes => Self::Stack,
            Self::Watches => Self::Scopes,
            Self::Repl => Self::Watches,
        }
    }

    /// Whether this pane takes typed text, and therefore whether an ordinary
    /// key means a character rather than a command.
    pub(crate) fn is_typing(self) -> bool {
        self == Self::Repl
    }
}

/// A prompt that has taken the keyboard.
///
/// One variant so far. It is an enum rather than a bare `Option<TextInput>`
/// because the next one — a confirmation before removing every watch, say — has
/// to be told apart from this one when `<CR>` is pressed, and the shape that
/// makes that a match arm is worth having before there are two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Modal {
    /// Typing the expression for a new watch (M16).
    AddWatch(TextInput),
}

/// A watch evaluation the TUI is waiting on, and the round it belongs to.
///
/// The round is what the id alone cannot give. Selecting a caller and then its
/// callee puts two batches in flight for the same watches, and the first
/// batch's answers describe a frame the pane has stopped showing — the right
/// expression against the wrong frame, which is exactly the class of mistake
/// D040 was written about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingWatch {
    pub(crate) generation: u64,
    pub(crate) watch: WatchId,
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
