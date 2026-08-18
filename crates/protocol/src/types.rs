use lazydap_core::{
    AdapterBreakpoint, AdapterKind, BreakpointId, BreakpointSelector, BreakpointStatus, EndReason,
    EvalContext, EvalResult, NewBreakpoint, NewWatch, OutputChunk, PauseReason, Scope, SessionId,
    SessionState, StackFrame, StepKind, ThreadInfo, ThreadUpdate, Variable, VariableFilter,
    WaitOutcome, Watch, WatchId, WatchSelector,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The version every message in this build is written against.
///
/// Bumping it is a breaking change to a product surface and needs an entry in
/// `docs/blueprint/15-decision-log.md`. Clients and daemon compare versions on
/// the first exchange; see [`Request::Ping`].
///
/// v2 (M6, D032): the stepping, inspection and breakpoint requests. A v1
/// daemon cannot decode any of them, so the bump is what turns "old daemon
/// still running" into a clean `VersionMismatch` the client resolves by
/// restarting it, rather than a `BadRequest` for a command that plainly
/// exists.
///
/// v3 (M14/M19, D043): `BreakpointUpdated` carries `Option<SessionId>` so that
/// a `lazydap break` with nothing running can be announced as a change to the
/// project rather than as an adapter's opinion. Both shapes would otherwise
/// claim to be v2 and fail to decode each other's events — the exact hazard the
/// version exists to turn into a clean restart.
///
/// v4 (M15, D050): `LaunchRequest` carries the adapter binary the *client*
/// resolved. The field is optional and an older daemon would ignore it
/// silently — which is precisely why this is a version bump rather than an
/// additive field. Ignoring it means falling back to the daemon's own
/// resolution, under the daemon's environment, and quietly launching a
/// different codelldb than the one the caller pinned. A `VersionMismatch` that
/// `lazydap shutdown` clears is a far better failure than a debugger that
/// obeys the wrong configuration without saying so.
///
/// v5 (M16, D056): the watch requests and [`Event::WatchUpdated`]. A new
/// `Request` variant is not additive in either direction — `Request` is an
/// externally-tagged enum with no fallback, so an older daemon fails to
/// deserialise the *whole envelope* and never reaches the `version` field it
/// would have refused on. What the caller gets instead is a `BadRequest` on id
/// `0` and a closed connection, which the client cannot match to its request
/// and reports as "the daemon closed the connection before answering". The bump
/// is what turns that into the `VersionMismatch` `lazydap shutdown` clears.
///
/// v6 (M22, D061): a third [`AdapterKind`], `Delve`. Unlike every bump above
/// this one adds no request and no field — it adds a *variant* to an enum that
/// already crosses the wire, which is the subtler break. A `LaunchRequest`
/// carrying `adapter: "delve"` is written by a v6 client and cannot be
/// deserialised by a v5 daemon: `AdapterKind` is externally tagged with no
/// fallback, so the unknown variant fails the whole envelope. Without the bump
/// that v5 daemon passes the version handshake — its version *is* 5 — and then
/// closes the connection on the first Go launch, which the client reports as a
/// dropped connection rather than the `VersionMismatch` that would have
/// restarted it. The bump moves the failure back to the handshake, where
/// `lazydap shutdown` clears it. codelldb and debugpy launches were decodable
/// by a v5 daemon and are the reason this was easy to miss.
///
/// v7 (D065, D066, D067, D069): four changes to what the daemon reports about
/// a stop and about a variable. [`ThreadInfo::name`] became optional,
/// [`Event::Stopped`] and [`StableState`] gained `adapter_thread_id`,
/// [`AdapterCapabilities`] gained `supports_variable_paging`, and [`Variable`]
/// gained `evaluate_name`. None of them is a new request, so a v6 daemon
/// decodes everything a v7 client sends — and then answers `threads` in a shape
/// this build's `ThreadInfo` cannot read at all, and reports a stepped thread
/// the way D066 says not to. The bump is what turns a silently wrong answer
/// back into the `VersionMismatch` `lazydap shutdown` clears.
///
/// v8 (D075–D080): what a stop reports, and what a handle means.
/// [`StableState`] gained `user_frame` and `locals` (D078), [`Response::Continued`]
/// gained `already_running` (D076), [`Response::Variables`] became a struct with
/// a `truncated` flag (D080), and [`ErrorCode`] gained `StaleHandle` (D075).
/// Three of those are new *variants or shapes* on the daemon's side of the wire,
/// which a v7 client cannot decode at all — an added `ErrorCode` variant fails
/// the whole envelope, exactly as D061's `AdapterKind` did. The subtler one is
/// D075: `frame_id` and `variables_reference` are no longer the adapter's own
/// numbers but handles lazydap mints per stop, so a v7 client that decoded them
/// anyway would be sending back integers this daemon reads against a different
/// table. Both failures are silent without the bump, and a `VersionMismatch`
/// `lazydap shutdown` clears is the better one.
///
/// v9 (D086): [`BreakpointAction`] gained `Updated` and `Unchanged`, so that
/// setting a location that already has a breakpoint can say which of the three
/// things it did. Two more variants on the daemon's side of the wire, and a v8
/// client fails the whole envelope on either of them — the same break D061's
/// `AdapterKind` and D075's `ErrorCode` had, and the same reason for a bump:
/// the failure belongs at the handshake, where `lazydap shutdown` clears it.
pub const LAZYDAP_PROTOCOL_VERSION: u32 = 9;

/// The envelope. Every frame on the socket is exactly one of these.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcMessage {
    /// Always [`LAZYDAP_PROTOCOL_VERSION`] for messages we write. Read it
    /// before trusting anything else in the envelope.
    pub version: u32,
    /// Correlates a response with its request. Monotonic per connection.
    /// Events use `0`, because nobody asked for them.
    pub id: u64,
    pub payload: IpcPayload,
}

impl IpcMessage {
    pub fn request(id: u64, request: Request) -> Self {
        Self::new(id, IpcPayload::Request(request))
    }

    pub fn response(id: u64, response: Response) -> Self {
        Self::new(id, IpcPayload::Response(response))
    }

    pub fn error(id: u64, error: IpcError) -> Self {
        Self::new(id, IpcPayload::Error(error))
    }

    /// Events are unsolicited, so they carry id `0` rather than borrowing the
    /// id of whichever request happened to be in flight.
    pub fn event(event: Event) -> Self {
        Self::new(0, IpcPayload::Event(event))
    }

    fn new(id: u64, payload: IpcPayload) -> Self {
        Self {
            version: LAZYDAP_PROTOCOL_VERSION,
            id,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IpcPayload {
    Request(Request),
    Response(Response),
    Event(Event),
    Error(IpcError),
}

/// What a client can ask for.
///
/// Bucketed per `ARCHITECTURE.md`: diagnostics, then session control, then
/// project state. The full schema, including the parts still unimplemented,
/// is `docs/blueprint/04-protocol.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    // --- Diagnostics ---
    /// Version handshake and liveness probe. The first thing every client
    /// sends: the daemon answers [`Response::Pong`] with its own version, and
    /// a mismatch means the client is talking to a daemon from another build.
    Ping,
    Status,
    /// Ask the daemon to end every session and exit.
    ///
    /// **This variant is frozen. Do not give it fields.**
    ///
    /// `Shutdown` is the escape hatch a new client uses to stop a daemon from
    /// an older build before starting its own, so it is a wire-compatibility
    /// contract with every version lazydap has ever shipped — not with the
    /// current schema. It is version-exempt on the server precisely so it
    /// works across a mismatch, and that exemption is useless if the frame
    /// cannot be *deserialised* in the first place: adding a field turns
    /// `"Shutdown"` into `{"Shutdown":{...}}`, which an older daemon rejects
    /// before it ever reaches the exemption. That has now broken twice.
    ///
    /// Anything that looks like it wants a field here belongs on the client.
    /// `lazydap shutdown --dry-run` is built from a `Status` call for exactly
    /// this reason: a preview changes nothing, so it never needs to be a
    /// `Shutdown` at all.
    Shutdown,
    Version,
    Doctor {
        check_adapters: bool,
        check_state: bool,
    },

    // --- Session lifecycle ---
    Launch(LaunchRequest),
    Disconnect {
        session_id: SessionId,
        /// Kill the debuggee too, rather than leaving it running detached.
        terminate: bool,
        dry_run: bool,
    },

    // --- Stepping ---
    /// Resume. With [`WaitMode::Wait`] the daemon answers once the program
    /// reaches a stable state, and the answer carries everything that happened
    /// on the way (`docs/blueprint/10-async-to-sync.md`).
    Continue {
        session_id: SessionId,
        /// `None` means whichever thread is stopped.
        thread_id: Option<i64>,
        wait: WaitMode,
        /// Wait for *every* thread to stop rather than returning on the first.
        all_threads: bool,
    },
    Step {
        session_id: SessionId,
        thread_id: Option<i64>,
        kind: StepKind,
        wait: WaitMode,
    },
    Pause {
        session_id: SessionId,
        thread_id: Option<i64>,
        wait: WaitMode,
    },

    // --- Inspection. Only meaningful in a stable state. ---
    Threads {
        session_id: SessionId,
    },
    StackTrace {
        session_id: SessionId,
        thread_id: Option<i64>,
        start_frame: Option<u32>,
        levels: Option<u32>,
    },
    Scopes {
        session_id: SessionId,
        /// `None` means the top frame, which is what a caller almost always
        /// wants and would otherwise have to fetch first.
        frame_id: Option<i64>,
    },
    Variables {
        session_id: SessionId,
        variables_reference: i64,
        filter: VariableFilter,
        start: Option<u32>,
        count: Option<u32>,
        /// Most to answer with. `None` takes the daemon's default, `Some(0)`
        /// lifts the cap the way `Some(0)` lifts a wait's timeout (D080).
        max: Option<u32>,
    },
    Eval {
        session_id: SessionId,
        expression: String,
        frame_id: Option<i64>,
        context: EvalContext,
    },
    /// Debuggee output the daemon has buffered. A read, not a drain: calling
    /// it twice shows the same thing.
    Output {
        session_id: SessionId,
        /// Only chunks at or after this Unix-epoch millisecond.
        since_ms: Option<u64>,
    },

    // --- Breakpoints. Project state: they exist without a session. ---
    BreakpointList,
    BreakpointAdd {
        breakpoint: NewBreakpoint,
        dry_run: bool,
    },
    BreakpointRemove {
        selector: BreakpointSelector,
        dry_run: bool,
    },
    BreakpointToggle {
        selector: BreakpointSelector,
        dry_run: bool,
    },

    // --- Watches. Project state too: the expressions exist without a session,
    // and what they evaluate to does not (M16). ---
    WatchList,
    WatchAdd {
        watch: NewWatch,
        dry_run: bool,
    },
    WatchRemove {
        selector: WatchSelector,
        dry_run: bool,
    },

    /// Push every event of these kinds down this connection as it happens.
    ///
    /// Answered with [`Response::Status`], not a variant of its own, and that
    /// is deliberate (D038): the snapshot is taken at the moment the
    /// subscription starts, so there is no gap between "what is the state?"
    /// and "tell me when it changes" for an event to fall into. Nothing
    /// buffered is replayed — the snapshot already accounts for it, and
    /// [`Request::Output`] reads the debuggee's earlier output without
    /// draining it.
    ///
    /// Sending it again replaces the set of kinds rather than adding to it.
    /// Events arrive as ordinary event frames (id `0`), interleaved with the
    /// replies to whatever else the client asks for.
    Subscribe {
        channels: Vec<EventKind>,
    },
}

/// Whether a stepping request returns immediately or blocks until the program
/// settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WaitMode {
    /// Fire and forget. The TUI wants this; agents almost never do.
    NoWait,
    /// Block until paused, exited, terminated — or the timeout.
    Wait {
        /// `None` uses the daemon's default (30s). `Some(0)` means no
        /// timeout at all, and makes the caller responsible for a program
        /// that never stops.
        timeout_ms: Option<u32>,
    },
}

impl WaitMode {
    pub fn is_waiting(&self) -> bool {
        matches!(self, Self::Wait { .. })
    }
}

/// Everything needed to start a debuggee under an adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LaunchRequest {
    pub adapter: AdapterKind,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Extra environment for the debuggee. Ordered so the wire form is stable.
    pub env: BTreeMap<String, String>,
    pub stop_on_entry: bool,
    /// The adapter binary, resolved by the client (D050).
    ///
    /// Discovery reads the user's config file and `PATH`, and both belong to
    /// whoever typed the command — not to a daemon that may have been started
    /// days ago from another directory with another environment. So the client
    /// resolves it, exactly as it resolves every other path (D047), and sends
    /// the answer.
    ///
    /// `None` means "resolve it yourself": no current client sends that, and
    /// the daemon's own lookup remains as the fallback rather than as a second
    /// opinion that could disagree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter_command: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Pong {
        version: u32,
        instance: String,
        uptime_ms: u64,
    },
    Status(StatusReport),
    Version {
        lazydap: String,
        protocol: u32,
    },
    Doctor(DoctorReport),
    Launched {
        session_id: SessionId,
        state: SessionState,
        /// Present when the launch ended with the debuggee already paused,
        /// i.e. `stop_on_entry`.
        reason: Option<PauseReason>,
        /// What the adapter actually called that reason, when we normalised
        /// it. See [`StableState::raw_reason`].
        raw_reason: Option<String>,
        thread_id: Option<i64>,
        capabilities: AdapterCapabilities,
        /// The persisted breakpoints applied during the configuration phase,
        /// with whatever the adapter made of them.
        breakpoints: Vec<BreakpointStatus>,
    },
    Disconnected {
        session_id: SessionId,
        dry_run: bool,
        /// What ending it did (or would do) to the debuggee.
        terminated_debuggee: bool,
    },
    ShuttingDown {
        /// The sessions this shutdown is ending.
        sessions: Vec<SessionSummary>,
    },

    /// A stepping request that did not wait. The program is running now.
    Continued {
        session_id: SessionId,
        /// The thread that was resumed. `None` when nothing was — see
        /// `already_running`, which is the only case that produces it.
        thread_id: Option<i64>,
        /// Nothing was resumed, because nothing was stopped.
        ///
        /// `continue` on a program that is already running sends no request at
        /// all (D055): there is nothing to ask for. Reporting that as an
        /// ordinary success made the two indistinguishable, and the `thread_id`
        /// that came with it was worse than useless — it was whatever the
        /// adapter answered a `threads` call on a running process with, which
        /// against codelldb is `0`, a thread that does not exist. So the field
        /// is `None` here and this one says why (D076).
        already_running: bool,
    },
    /// A stepping request that waited: one blob describing everything that
    /// happened until the program settled.
    ///
    /// Boxed because it is much the largest thing `Response` — and so
    /// `IpcPayload`, and so every frame on the socket — can carry, and every
    /// other message would otherwise be padded out to its size. `Box`
    /// serialises transparently, so the wire shape is unchanged.
    Stepped(Box<StableState>),

    Threads(Vec<ThreadInfo>),
    StackTrace {
        frames: Vec<StackFrame>,
        /// How many frames there are in total, when the adapter says.
        total: Option<u32>,
    },
    Scopes(Vec<Scope>),
    Variables(VariableList),
    Evaluated(EvalResult),
    Output {
        chunks: Vec<OutputChunk>,
        /// Chunks lost because the buffer filled before anybody read it.
        /// Non-zero means this is not the whole story.
        dropped: u64,
    },

    Breakpoints(BreakpointReport),
    Watches(WatchReport),
}

/// One `variables` answer, and whether it is the whole of one.
///
/// A bare `Vec` could not say. A `Vec` of 2000 elements expands to 2001 rows in
/// a single response, and an agent that asked for a container's contents got
/// them all with nothing to indicate it had just spent most of its context on
/// one variable. The cap is a default with a flag to raise it, and this says
/// when it bit — the same honesty [`StableState::output_truncated`] applies to
/// output (D080).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariableList {
    pub variables: Vec<Variable>,
    /// `variables` is a prefix, not the whole list. Ask again with `--start`
    /// past what you have, or raise `--max`.
    pub truncated: bool,
}

impl VariableList {
    /// The whole of a list, however long: nothing was left out.
    pub fn whole(variables: Vec<Variable>) -> Self {
        Self {
            variables,
            truncated: false,
        }
    }
}

/// The top frame's locals, carried by a stop rather than fetched afterwards.
///
/// Reading a local was two commands — `scopes`, then `variables --reference N`
/// — for the single most common thing anybody does after a program stops. The
/// two round trips were the daemon's to make, not the caller's, so it makes
/// them (D078).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameLocals {
    /// The frame these belong to: [`StableState::frame`]'s id. Present so a
    /// reader never has to assume which frame was asked about.
    pub frame_id: i64,
    /// The handle these came from, for paging past `truncated` with
    /// `lazydap variables --reference N --start ...`.
    pub variables_reference: i64,
    pub variables: Vec<Variable>,
    /// `variables` is a prefix. A frame with two thousand locals must not blow
    /// the blob, and a blob that silently dropped the rest would be the lie
    /// this field exists to prevent.
    pub truncated: bool,
}

/// What a `--wait` saw. The one shape agents read.
///
/// Every field is populated on a best-effort basis: a blob that omitted the
/// captured output because the program also crashed would make the crash
/// harder to diagnose, not easier. See `docs/blueprint/10-async-to-sync.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StableState {
    pub state: WaitOutcome,
    /// Why it stopped. Populated when `state` is `paused`.
    pub reason: Option<PauseReason>,
    /// The adapter's own word for `reason`, present only when the two differ.
    ///
    /// codelldb reports an entry stop as an `exception` with description
    /// `signal SIGSTOP` (quirk 6). lazydap normalises that to `entry` because
    /// the JSON schema is the product surface, and keeps the adapter's version
    /// here so nothing is hidden. See D033.
    pub raw_reason: Option<String>,
    pub thread_id: Option<i64>,
    /// Which thread the *adapter* named, present only when that is not
    /// `thread_id`.
    ///
    /// codelldb answers a `next` aimed at one thread with a `stopped` event
    /// naming whichever thread it had selected before. `thread_id` is the
    /// thread lazydap asked to step — which is the one that moved — and this
    /// keeps the adapter's answer visible rather than quietly discarded, the
    /// way `raw_reason` does for D033. See D066.
    pub adapter_thread_id: Option<i64>,
    pub all_threads_stopped: bool,
    /// Threads that also stopped within the coalescing window (D020).
    pub additional_stopped_threads: Vec<i64>,
    /// Which of our breakpoints caused this stop.
    pub hit_breakpoint_ids: Vec<BreakpointId>,
    /// The debuggee's status, when it finished.
    pub exit_code: Option<i32>,
    /// The top frame, fetched for convenience whenever the program paused.
    pub frame: Option<StackFrame>,
    /// The nearest frame below [`Self::frame`] that has a source path, when
    /// `frame` has none.
    ///
    /// **Not a correction of `frame`.** `frame` is where the program stopped
    /// and stays that, whatever it turns out to be; overwriting it would be
    /// lying about where the program is. This is the separate question an agent
    /// actually asks next — *whose code is responsible* — and it has a
    /// different answer whenever a program dies inside a library. A real
    /// segfault stopped in `_platform_strcmp$VARIANT$Base`, which has no path
    /// at all, and naming the user's `lookup_key` at `config.c:40` took a
    /// second command it should not have (D078).
    ///
    /// `None` when `frame` already has a source path — there is nothing to add
    /// — or when no frame in the stack has one. Read it as
    /// `user_frame` first, `frame` second.
    pub user_frame: Option<StackFrame>,
    /// [`Self::frame`]'s locals, so reading one is not a second round trip.
    ///
    /// `None` when the program is not paused, when the adapter would not say,
    /// or when there is no frame to have locals — never a fabricated empty
    /// list, which would claim a frame has no locals when the truth is that
    /// nobody could find out.
    pub locals: Option<FrameLocals>,
    pub captured_output: Vec<OutputChunk>,
    /// You are not seeing all of it.
    ///
    /// True for either of the two ways a blob can be incomplete, because a
    /// reader cannot act on a distinction it was never told about:
    ///
    /// - the wait's own output cap was reached, and everything after it was
    ///   dropped. What is kept is then a *prefix* of what the program printed:
    ///   the wait stops taking output entirely rather than skipping the chunk
    ///   that overran the cap and going on accepting smaller ones behind it,
    ///   which spliced two moments of a program's life together (D070);
    /// - events were lost before the wait could read them — the session buffer
    ///   overran between two CLI invocations, or a live subscription fell
    ///   behind. What is kept is then a *suffix*, and `dropped_events` says how
    ///   much is missing (D072).
    pub output_truncated: bool,
    /// How many events were lost before this blob could carry them. `0` when
    /// nothing was lost that way — including when `output_truncated` is set by
    /// the output cap, which drops bytes rather than events.
    pub dropped_events: u64,
    pub breakpoint_updates: Vec<AdapterBreakpoint>,
    pub thread_updates: Vec<ThreadUpdate>,
    pub elapsed_ms: u64,
}

impl StableState {
    /// An outcome with nothing observed yet. Every wait starts here and fills
    /// fields in as events arrive, so a blob is never half-built.
    pub fn new(state: WaitOutcome) -> Self {
        Self {
            state,
            reason: None,
            raw_reason: None,
            thread_id: None,
            adapter_thread_id: None,
            all_threads_stopped: false,
            additional_stopped_threads: Vec::new(),
            hit_breakpoint_ids: Vec::new(),
            exit_code: None,
            frame: None,
            user_frame: None,
            locals: None,
            captured_output: Vec::new(),
            output_truncated: false,
            dropped_events: 0,
            breakpoint_updates: Vec::new(),
            thread_updates: Vec::new(),
            elapsed_ms: 0,
        }
    }
}

/// The answer to every breakpoint command.
///
/// One shape for list, add, remove and toggle so a client parses breakpoints
/// exactly once, and so `--dry-run` is a flag on a familiar response rather
/// than a different one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakpointReport {
    pub action: BreakpointAction,
    /// Nothing was changed; this is what *would* change.
    pub dry_run: bool,
    /// The breakpoints this command is about: all of them for a list, the
    /// affected ones otherwise.
    pub breakpoints: Vec<BreakpointStatus>,
    /// Ids the selector named but no breakpoint matched. Empty on success;
    /// a caller that piped ids in can tell which ones went stale.
    pub not_found: Vec<BreakpointId>,
    /// Whether a live session was told about the change. `false` means the
    /// breakpoints are recorded and will apply to the next launch.
    pub applied_to_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointAction {
    Listed,
    Added,
    /// A location that already had a breakpoint, set again with different
    /// modifiers. The id is the one it already had (D086).
    Updated,
    /// A location set again with exactly what it already said. Distinct from
    /// `Updated` because a script re-applying a list of breakpoints wants to
    /// know which of them it actually changed.
    Unchanged,
    Removed,
    Toggled,
}

impl BreakpointAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Listed => "listed",
            Self::Added => "added",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
            Self::Removed => "removed",
            Self::Toggled => "toggled",
        }
    }

    /// How to describe it when nothing has happened yet.
    pub fn would(&self) -> &'static str {
        match self {
            Self::Listed => "would list",
            Self::Added => "would add",
            Self::Updated => "would update",
            Self::Unchanged => "would leave unchanged",
            Self::Removed => "would remove",
            Self::Toggled => "would toggle",
        }
    }
}

/// The answer to every watch command.
///
/// The same shape [`BreakpointReport`] has, and for the same reason: one shape
/// for list, add and remove means a client parses watches exactly once, and
/// `--dry-run` is a flag on a familiar response rather than a different one.
///
/// It is missing `applied_to_session`, which is the one real difference between
/// the two. A breakpoint has to be handed to a live adapter to mean anything,
/// so whether that happened is news. There is no DAP request that installs a
/// watch — an expression is evaluated on demand, at a stop, by whoever wants to
/// know — so there is nothing for a session to have been told, and a field
/// saying `false` forever would only invite a caller to wonder what it was
/// waiting for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchReport {
    pub action: WatchAction,
    /// Nothing was changed; this is what *would* change.
    pub dry_run: bool,
    /// The watches this command is about: all of them for a list, the affected
    /// ones otherwise.
    pub watches: Vec<Watch>,
    /// Ids the selector named but no watch matched. Empty on success; a caller
    /// that piped ids in can tell which ones went stale.
    pub not_found: Vec<WatchId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchAction {
    Listed,
    Added,
    Removed,
}

impl WatchAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Listed => "listed",
            Self::Added => "added",
            Self::Removed => "removed",
        }
    }

    /// How to describe it when nothing has happened yet.
    pub fn would(&self) -> &'static str {
        match self {
            Self::Listed => "would list",
            Self::Added => "would add",
            Self::Removed => "would remove",
        }
    }
}

/// `lazydap doctor`: what is set up, and what is not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorReport {
    /// `false` when any check failed, so a script can branch on one field.
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// What the daemon knows about itself. `lazydap status` prints this.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusReport {
    pub instance: String,
    pub daemon_pid: u32,
    pub uptime_ms: u64,
    pub protocol_version: u32,
    pub lazydap_version: String,
    /// `None` when no session has been launched, or the last one was
    /// disconnected.
    pub session: Option<SessionSummary>,
}

/// One session, as much of it as a status call is allowed to be cheap about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: SessionId,
    pub adapter: AdapterKind,
    pub program: PathBuf,
    pub state: SessionState,
    pub exit_code: Option<i32>,
    /// How many events the daemon has buffered for a client that has not asked
    /// for them yet. `--wait` (M6) drains this.
    pub buffered_events: usize,
    /// How many chunks of debuggee output are in that buffer.
    pub captured_output_chunks: usize,
    /// Events discarded because the buffer filled up before anybody read it.
    /// Non-zero means the record is incomplete, and a client should say so
    /// rather than present it as the whole story.
    pub dropped_events: u64,
    pub uptime_ms: u64,
}

/// The adapter capabilities lazydap currently acts on.
///
/// Deliberately not DAP's full `Capabilities`: the daemon translates, so DAP
/// vocabulary stops at the adapter seam (`ARCHITECTURE.md` anti-pattern 4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterCapabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
    /// Whether the adapter honours `variables`' `filter`, `start` and `count`.
    /// When it does not, lazydap applies them itself (D067) — so the flags mean
    /// the same thing to a caller either way, and this says which happened.
    pub supports_variable_paging: bool,
}

/// Something happened that no client asked about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    SessionStarted {
        session_id: SessionId,
        adapter: AdapterKind,
    },
    SessionEnded {
        session_id: SessionId,
        reason: EndReason,
    },
    Stopped {
        session_id: SessionId,
        thread_id: Option<i64>,
        /// The adapter's own answer for which thread stopped, when it names a
        /// different one from the thread lazydap asked to step (D066).
        adapter_thread_id: Option<i64>,
        reason: PauseReason,
        /// The adapter's own word for it, when we normalised the reason.
        raw_reason: Option<String>,
        all_threads_stopped: bool,
        /// Ours, already mapped from whatever the adapter calls them.
        hit_breakpoint_ids: Vec<BreakpointId>,
    },
    Continued {
        session_id: SessionId,
        thread_id: Option<i64>,
        all_threads_continued: bool,
    },
    Output {
        session_id: SessionId,
        chunk: OutputChunk,
    },
    /// A breakpoint changed. `session_id` says what kind of change.
    ///
    /// - **`Some(id)`** — the adapter for that session changed its mind:
    ///   verified one it had not, or moved it to the nearest line with code.
    ///   The [`AdapterBreakpoint`] is that adapter's opinion, and it is only
    ///   true for as long as that session lives. A client that kept it past
    ///   the session's end would be showing a claim nobody is standing behind
    ///   any more.
    /// - **`None`** — the *project's* list changed, because somebody ran
    ///   `lazydap break` (add, remove or toggle). There is no adapter opinion
    ///   in one of these: the verification fields carry no information, and a
    ///   client that applied them would be inventing a claim nobody made. What
    ///   it means is "the list is not what you last read; read it again".
    BreakpointUpdated {
        session_id: Option<SessionId>,
        breakpoint: AdapterBreakpoint,
    },
    ThreadChanged {
        session_id: SessionId,
        update: ThreadUpdate,
    },
    /// The project's watch list changed, because somebody ran `lazydap watch`
    /// (add or remove).
    ///
    /// Always project scope, and so there is no `session_id` to distinguish two
    /// meanings the way [`Self::BreakpointUpdated`] has to. A watch has no
    /// adapter opinion for a session to hold: nothing is ever installed, so
    /// nothing can be verified or moved.
    ///
    /// The payload names *which* watch, and nothing more. What it means to a
    /// client is "the list is not what you last read; read it again" — which is
    /// the only honest thing one of these can say, since an add and a removal
    /// arrive the same way and only the list distinguishes them (D043's lesson,
    /// applied at the start rather than after the bug).
    WatchUpdated { watch_id: WatchId },
}

impl Event {
    pub fn kind(&self) -> EventKind {
        match self {
            Self::SessionStarted { .. } => EventKind::SessionStarted,
            Self::SessionEnded { .. } => EventKind::SessionEnded,
            Self::Stopped { .. } => EventKind::Stopped,
            Self::Continued { .. } => EventKind::Continued,
            Self::Output { .. } => EventKind::Output,
            Self::BreakpointUpdated { .. } => EventKind::BreakpointUpdated,
            Self::ThreadChanged { .. } => EventKind::ThreadChanged,
            Self::WatchUpdated { .. } => EventKind::WatchUpdated,
        }
    }

    /// Which session this is about, when it is about one.
    ///
    /// `None` for the project-scope events — a `lazydap break` or a `lazydap
    /// watch` is a change to the project, and belongs to no session's history.
    pub fn session_id(&self) -> Option<SessionId> {
        match self {
            Self::SessionStarted { session_id, .. }
            | Self::SessionEnded { session_id, .. }
            | Self::Stopped { session_id, .. }
            | Self::Continued { session_id, .. }
            | Self::Output { session_id, .. }
            | Self::ThreadChanged { session_id, .. } => Some(*session_id),
            Self::BreakpointUpdated { session_id, .. } => *session_id,
            // Never a session's: a watch belongs to the project, and there is
            // no adapter opinion in one of these to be scoped to a session.
            Self::WatchUpdated { .. } => None,
        }
    }
}

/// Subscription channels. One per [`Event`] variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SessionStarted,
    SessionEnded,
    Stopped,
    Continued,
    Output,
    BreakpointUpdated,
    ThreadChanged,
    WatchUpdated,
}

/// A failure, always paired with the id of the request that caused it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcError {
    pub code: ErrorCode,
    pub message: String,
    /// Free-form context. `{}` when there is nothing useful to add.
    pub details: serde_json::Value,
}

impl IpcError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: serde_json::Value::Object(serde_json::Map::new()),
        }
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for IpcError {}

/// The closed set of failures the protocol can report. Clients match on this,
/// not on message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCode {
    AdapterNotFound,
    AdapterCrashed,
    AdapterTimeout,
    SessionNotFound,
    SessionAlreadyActive,
    /// The stack, scopes and variables of a running program are undefined.
    /// Asking for them is a caller mistake, not an adapter failure
    /// (`docs/blueprint/10-async-to-sync.md`).
    SessionNotPaused,
    /// A `frame_id` or `variables_reference` from a stop the program has left.
    ///
    /// Distinct from [`Self::BadRequest`], which is a handle that was never
    /// handed out at all. The two need different reactions and so cannot share
    /// a code: a stale handle means "ask again at this stop and retry", a bad
    /// one means "you made that number up". Both used to reach the adapter,
    /// where a stale handle either errored obscurely or — the reason this
    /// exists — collided with one the adapter had since recycled and returned
    /// somebody else's variables under exit 0 (D075).
    StaleHandle,
    InvalidLaunchConfig,
    InvalidProjectRoot,
    DapProtocolError,
    DaemonInternalError,
    Unsupported,
    Timeout,
    Cancelled,
    BadRequest,
    VersionMismatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_envelope_states_the_version_it_was_written_against() {
        let message = IpcMessage::request(1, Request::Ping);
        assert_eq!(message.version, LAZYDAP_PROTOCOL_VERSION);
        assert_eq!(message.id, 1);
    }

    #[test]
    fn events_carry_id_zero_because_nobody_asked_for_them() {
        let event = IpcMessage::event(Event::SessionEnded {
            session_id: SessionId::new(),
            reason: EndReason::Terminated,
        });
        assert_eq!(event.id, 0);
    }

    #[test]
    fn a_launch_request_round_trips_through_json() {
        let request = Request::Launch(LaunchRequest {
            adapter: AdapterKind::Codelldb,
            program: PathBuf::from("/tmp/hello"),
            args: vec!["--verbose".into()],
            cwd: PathBuf::from("/tmp"),
            env: BTreeMap::from([("RUST_LOG".to_string(), "debug".to_string())]),
            stop_on_entry: true,
            adapter_command: Some(PathBuf::from("/usr/local/bin/codelldb")),
        });
        let message = IpcMessage::request(7, request.clone());

        let json = serde_json::to_string(&message).expect("serialise");
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialise");

        assert_eq!(decoded, message, "got: {json}");
        assert!(json.contains(r#""stop_on_entry":true"#), "got: {json}");
    }

    #[test]
    fn every_event_reports_its_own_kind_and_session() {
        let session_id = SessionId::new();
        let event = Event::Stopped {
            session_id,
            thread_id: Some(1),
            adapter_thread_id: None,
            reason: PauseReason::Entry,
            raw_reason: None,
            all_threads_stopped: true,
            hit_breakpoint_ids: Vec::new(),
        };
        assert_eq!(event.kind(), EventKind::Stopped);
        assert_eq!(event.session_id(), Some(session_id));
    }

    #[test]
    fn a_wait_blob_round_trips_with_the_state_spelled_the_way_agents_read_it() {
        let mut blob = StableState::new(WaitOutcome::Paused);
        blob.reason = Some(PauseReason::Breakpoint);
        blob.thread_id = Some(1);
        blob.hit_breakpoint_ids = vec![BreakpointId(3)];
        blob.captured_output = vec![OutputChunk::new(
            lazydap_core::OutputCategory::Stdout,
            "hello\n",
        )];

        let json = serde_json::to_string(&blob).expect("serialise");
        assert!(json.contains(r#""state":"paused""#), "got: {json}");
        assert!(json.contains(r#""reason":"breakpoint""#), "got: {json}");
        assert!(json.contains(r#""hit_breakpoint_ids":[3]"#), "got: {json}");

        let decoded: StableState = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(decoded, blob);
    }

    #[test]
    fn a_wait_request_says_how_long_it_is_prepared_to_wait() {
        let request = Request::Continue {
            session_id: SessionId::new(),
            thread_id: None,
            wait: WaitMode::Wait {
                timeout_ms: Some(5_000),
            },
            all_threads: false,
        };
        let json = serde_json::to_string(&request).expect("serialise");
        let decoded: Request = serde_json::from_str(&json).expect("deserialise");

        assert_eq!(decoded, request, "got: {json}");
        match decoded {
            Request::Continue { wait, .. } => assert!(wait.is_waiting()),
            other => unreachable!("expected a continue, got: {other:?}"),
        }
    }

    #[test]
    fn a_no_wait_stepping_request_is_distinguishable_from_a_defaulted_one() {
        // `NoWait` and `Wait { timeout_ms: None }` mean opposite things — fire
        // and forget versus block for the default 30s — so they must not
        // collapse into the same JSON.
        let no_wait = serde_json::to_string(&WaitMode::NoWait).expect("serialise");
        let defaulted =
            serde_json::to_string(&WaitMode::Wait { timeout_ms: None }).expect("serialise");
        assert_ne!(no_wait, defaulted, "got: {no_wait} and {defaulted}");
        assert!(!WaitMode::NoWait.is_waiting());
    }

    #[test]
    fn a_dry_run_breakpoint_report_says_so_in_the_shape_a_real_one_uses() {
        let report = BreakpointReport {
            action: BreakpointAction::Removed,
            dry_run: true,
            breakpoints: Vec::new(),
            not_found: vec![BreakpointId(9)],
            applied_to_session: false,
        };
        let json = serde_json::to_string(&report).expect("serialise");

        assert!(json.contains(r#""action":"removed""#), "got: {json}");
        assert!(json.contains(r#""dry_run":true"#), "got: {json}");
        assert!(json.contains(r#""not_found":[9]"#), "got: {json}");
    }

    #[test]
    fn a_watch_request_round_trips_through_json() {
        let request = Request::WatchAdd {
            watch: NewWatch {
                expression: "tokens[pos]".to_string(),
                label: Some("current token".to_string()),
            },
            dry_run: false,
        };
        let json = serde_json::to_string(&request).expect("serialise");
        let decoded: Request = serde_json::from_str(&json).expect("deserialise");

        assert_eq!(decoded, request, "got: {json}");
    }

    #[test]
    fn a_request_variant_this_build_does_not_know_is_a_hard_decode_failure() {
        // This is the whole reason M16 bumped the protocol rather than adding
        // variants quietly (D056). `Request` is externally tagged with no
        // fallback, so an unknown variant does not fail *softly* — the whole
        // envelope fails to deserialise, which means the daemon never reaches
        // the `version` field it would have refused on. Two builds both
        // claiming v4 would therefore answer a watch command with a closed
        // connection instead of a `VersionMismatch` a restart clears.
        let frame = r#"{"version":4,"id":7,"payload":{"Request":"WatchList"}}"#
            .replace("WatchList", "SomethingFromTheFuture");
        let error = serde_json::from_str::<IpcMessage>(&frame).expect_err("unknown variant");

        assert!(
            error.to_string().contains("unknown variant"),
            "got: {error}",
        );
    }

    #[test]
    fn a_watch_update_belongs_to_the_project_rather_than_to_any_session() {
        // Which is what keeps it out of a session's event buffer, and therefore
        // out of the blob the next `continue --wait` returns.
        let event = Event::WatchUpdated {
            watch_id: WatchId(3),
        };
        assert_eq!(event.kind(), EventKind::WatchUpdated);
        assert_eq!(event.session_id(), None);
    }

    #[test]
    fn a_dry_run_watch_report_says_so_in_the_shape_a_real_one_uses() {
        let report = WatchReport {
            action: WatchAction::Removed,
            dry_run: true,
            watches: Vec::new(),
            not_found: vec![WatchId(9)],
        };
        let json = serde_json::to_string(&report).expect("serialise");

        assert!(json.contains(r#""action":"removed""#), "got: {json}");
        assert!(json.contains(r#""dry_run":true"#), "got: {json}");
        assert!(json.contains(r#""not_found":[9]"#), "got: {json}");
    }

    #[test]
    fn an_error_carries_a_code_to_branch_on_and_details_to_read() {
        let error = IpcError::new(ErrorCode::AdapterNotFound, "no codelldb")
            .with_details(serde_json::json!({ "searched": ["/usr/bin"] }));

        let json = serde_json::to_string(&IpcMessage::error(4, error)).expect("serialise");
        let decoded: IpcMessage = serde_json::from_str(&json).expect("deserialise");

        match decoded.payload {
            IpcPayload::Error(error) => {
                assert_eq!(error.code, ErrorCode::AdapterNotFound);
                assert_eq!(error.details["searched"][0], "/usr/bin");
            }
            other => unreachable!("expected an error, got: {other:?}"),
        }
    }
}
