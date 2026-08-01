use crate::adapter::AdapterHandle;
use crate::debuggee::Debuggee;
use lazydap_core::{
    AdapterBreakpoint, AdapterKind, BreakpointId, BreakpointStatus, EndReason, OutputChunk,
    SessionId, SessionState,
};
use lazydap_protocol::{
    ErrorCode, Event, IpcError, LAZYDAP_PROTOCOL_VERSION, SessionSummary, StatusReport,
};
use lazydap_store::ProjectStore;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use tokio::sync::{broadcast, watch};

/// How many events a session holds for a client that has not asked for them
/// yet. Between two CLI invocations a chatty debuggee can produce a lot of
/// output; keeping the newest thousand bounds memory without losing the part
/// anybody reads.
const EVENT_BUFFER_CAPACITY: usize = 1000;

/// Slack for live subscribers. Nobody subscribes until M11; a lagging client
/// loses old events rather than blocking the session.
const EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Everything one daemon owns.
pub struct DaemonState {
    pub instance: String,
    /// Per-project breakpoints and (later) watches. Outlives every session:
    /// breakpoints are something a project has, not something a session has.
    pub store: Arc<ProjectStore>,
    started_at: Instant,
    /// Keyed by id even though v0.1 allows one at a time (D007): the map is
    /// what makes lifting that limit a daemon-only change.
    sessions: RwLock<HashMap<SessionId, Slot>>,
    event_tx: broadcast::Sender<SeqEvent>,
    shutdown_tx: watch::Sender<bool>,
}

/// An event with the position it holds in its session's history.
///
/// The sequence number is what makes `--wait` exact. A wait subscribes, then
/// drains whatever was buffered before it started; without a number to compare
/// against, an event landing between those two steps would be reported twice,
/// and one landing before the subscription would be lost. The number settles
/// both: drain up to a watermark, ignore anything live at or below it.
#[derive(Debug, Clone)]
pub struct SeqEvent {
    pub seq: u64,
    pub event: Event,
}

/// A session, or the intent to have one.
///
/// Launching takes seconds — spawning an adapter, waiting on a handshake — and
/// the slot has to be occupied for all of it, or two concurrent launches both
/// pass the "is anything running?" check and both start an adapter.
enum Slot {
    Reserved,
    Live(Arc<Session>),
}

impl DaemonState {
    pub fn new(instance: String, store: Arc<ProjectStore>) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (shutdown_tx, _) = watch::channel(false);
        Arc::new(Self {
            instance,
            store,
            started_at: Instant::now(),
            sessions: RwLock::new(HashMap::new()),
            event_tx,
            shutdown_tx,
        })
    }

    pub fn uptime_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    pub fn events(&self) -> broadcast::Sender<SeqEvent> {
        self.event_tx.clone()
    }

    /// Announce something about the *project* rather than about a session.
    ///
    /// Broadcast only, never buffered: the buffer is a session's history, read
    /// by `lazydap status` and by the start of a `--wait`, and a breakpoint
    /// somebody set between two sessions belongs to neither. The sequence
    /// number is `0` for the same reason — it orders a session's events, and
    /// there is no session here to order this against. Nothing reads it: a
    /// `--wait` skips any event whose session is not its own, which a
    /// project-scope one never is.
    pub fn emit_project(&self, event: Event) {
        tracing::debug!(target: "daemon.events", ?event, "announcing a project change");
        let _ = self.event_tx.send(SeqEvent { seq: 0, event });
    }

    /// Every live session, for the shutdown preview. Does not give up slots.
    pub fn summaries(&self) -> Vec<SessionSummary> {
        read(&self.sessions)
            .values()
            .filter_map(|slot| match slot {
                Slot::Live(session) => Some(session.summary()),
                Slot::Reserved => None,
            })
            .collect()
    }

    /// Claim the single session slot, or explain why it is taken (D007).
    pub fn reserve(self: &Arc<Self>, id: SessionId) -> Result<SessionReservation, IpcError> {
        let mut sessions = write(&self.sessions);
        if let Some(existing) = describe_occupant(&sessions) {
            return Err(IpcError::new(
                ErrorCode::SessionAlreadyActive,
                "a debug session is already active; run `lazydap disconnect` first",
            )
            .with_details(existing));
        }
        sessions.insert(id, Slot::Reserved);
        Ok(SessionReservation {
            state: Arc::clone(self),
            id,
            promoted: false,
        })
    }

    /// Drop sessions whose program has finished, so the slot is free again.
    ///
    /// M5 left a finished session holding the slot, which meant a second
    /// `lazydap launch` was refused until somebody ran `lazydap disconnect` on
    /// a session that had no adapter left to disconnect from. The slot exists
    /// to stop two adapters running at once (D007); a session with no adapter
    /// is not what it is protecting against.
    ///
    /// Returns how many were reaped, which is only interesting to a log line.
    pub fn reap_finished(&self) -> usize {
        let mut sessions = write(&self.sessions);
        let finished: Vec<SessionId> = sessions
            .iter()
            .filter_map(|(id, slot)| match slot {
                Slot::Live(session) if !session.state().is_live() => Some(*id),
                _ => None,
            })
            .collect();

        for id in &finished {
            tracing::debug!(
                target: "daemon.session",
                session_id = %id,
                "reaping a session whose program has finished",
            );
            // The finished session never disconnected, so delve's compiled
            // binary is still on disk (finding 4); the adapter itself goes when
            // the session's `Drop` fires `kill_on_drop`, but that does not touch
            // the file.
            if let Some(Slot::Live(session)) = sessions.get(id) {
                session.clean_compiled_artifact();
            }
            sessions.remove(id);
        }
        finished.len()
    }

    /// The live session, if there is one.
    pub fn active_session(&self) -> Option<Arc<Session>> {
        read(&self.sessions).values().find_map(|slot| match slot {
            Slot::Live(session) => Some(Arc::clone(session)),
            Slot::Reserved => None,
        })
    }

    /// Look up a live session without giving up its slot.
    ///
    /// Teardown uses this rather than [`remove_session`](Self::remove_session):
    /// the slot has to stay occupied for as long as the adapter is being shut
    /// down, or a concurrent launch walks straight through the single-session
    /// check while the previous adapter is still alive.
    pub fn session(&self, id: SessionId) -> Option<Arc<Session>> {
        match read(&self.sessions).get(&id) {
            Some(Slot::Live(session)) => Some(Arc::clone(session)),
            _ => None,
        }
    }

    /// Claim a session for teardown, freeing its slot when the guard drops.
    ///
    /// The slot has to stay occupied while the adapter is being shut down, but
    /// it must not stay occupied *forever* if that goes wrong: without a
    /// guard, a panic anywhere in teardown leaves the slot held and every
    /// later `launch` rejected until the daemon is restarted. Tying the
    /// removal to a drop covers the normal path and the unwinding one with
    /// the same line.
    pub fn begin_teardown(self: &Arc<Self>, id: SessionId) -> Option<SessionTeardown> {
        let session = self.session(id)?;
        Some(SessionTeardown {
            state: Arc::clone(self),
            session,
        })
    }

    /// Forget a session, handing it back so the caller can shut its adapter
    /// down.
    pub fn remove_session(&self, id: SessionId) -> Option<Arc<Session>> {
        match write(&self.sessions).remove(&id) {
            Some(Slot::Live(session)) => Some(session),
            _ => None,
        }
    }

    pub fn drain_sessions(&self) -> Vec<Arc<Session>> {
        write(&self.sessions)
            .drain()
            .filter_map(|(_, slot)| match slot {
                Slot::Live(session) => Some(session),
                Slot::Reserved => None,
            })
            .collect()
    }

    pub fn status(&self) -> StatusReport {
        StatusReport {
            instance: self.instance.clone(),
            daemon_pid: std::process::id(),
            uptime_ms: self.uptime_ms(),
            protocol_version: LAZYDAP_PROTOCOL_VERSION,
            lazydap_version: env!("CARGO_PKG_VERSION").to_string(),
            session: self.active_session().map(|session| session.summary()),
        }
    }

    pub fn request_shutdown(&self) {
        self.shutdown_tx.send_replace(true);
    }

    pub fn shutdown_requested(&self) -> bool {
        *self.shutdown_tx.borrow()
    }

    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown_tx.subscribe()
    }
}

/// Occupancy of the session slot, for the error a rejected launch gets back.
fn describe_occupant(sessions: &HashMap<SessionId, Slot>) -> Option<serde_json::Value> {
    sessions.iter().next().map(|(id, slot)| match slot {
        Slot::Live(session) => serde_json::json!({
            "session_id": id.to_string(),
            "state": session.state(),
        }),
        Slot::Reserved => serde_json::json!({
            "session_id": id.to_string(),
            "state": "launching",
        }),
    })
}

/// A held session slot. Dropping it without promoting frees the slot, so a
/// launch that fails half way does not lock the daemon out of ever launching
/// again.
pub struct SessionReservation {
    state: Arc<DaemonState>,
    id: SessionId,
    promoted: bool,
}

impl SessionReservation {
    /// Turn the reservation into a live session and announce it.
    ///
    /// The two happen here, in this order, and nowhere else — that ordering is
    /// a contract with every subscriber and it used to be the caller's to get
    /// right, which it did not. `SessionStarted` was emitted before the
    /// session reached the map, so a client subscribing in that window missed
    /// the event (it subscribed too late) *and* got a session-less snapshot
    /// (the session was not there yet), leaving a TUI showing "no session"
    /// until the program next moved.
    ///
    /// Announcing from inside the promotion makes the window impossible rather
    /// than merely absent: a subscriber now either sees the session in its
    /// snapshot or receives the event, and never neither.
    pub fn promote(mut self, session: Arc<Session>) {
        write(&self.state.sessions).insert(self.id, Slot::Live(Arc::clone(&session)));
        self.promoted = true;

        session.emit(Event::SessionStarted {
            session_id: self.id,
            adapter: session.adapter_kind,
        });
    }
}

impl Drop for SessionReservation {
    fn drop(&mut self) {
        if !self.promoted {
            write(&self.state.sessions).remove(&self.id);
        }
    }
}

/// A session being torn down. Its slot is freed when this drops, whether
/// teardown finished or panicked.
pub struct SessionTeardown {
    state: Arc<DaemonState>,
    session: Arc<Session>,
}

impl SessionTeardown {
    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }
}

impl Drop for SessionTeardown {
    fn drop(&mut self) {
        self.state.remove_session(self.session.id);
    }
}

/// What [`Session::claim_run`] decided, and the only way to find out: the
/// state it was decided from is not readable afterwards without racing the
/// pump all over again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunClaim {
    /// Ask the adapter. The session is now `Running`, and `previous` is what
    /// it was — what [`Session::restore_state`] puts back if the request
    /// fails.
    Ask { previous: SessionState },
    /// Do not ask, and do not touch the state: the program is already doing
    /// what was requested.
    AlreadyRunning,
}

/// Identifies one request the program was asked to perform.
///
/// The point of the number is *withdrawal*. A request that the adapter rejects
/// has to take its marker back down, and between the send and the rejection
/// another request can have replaced it — a `pause` arriving while a step is
/// being refused. Clearing "the marker" would then erase the newer one and
/// resurrect exactly the bug the marker exists to prevent, so a withdrawal
/// names the marker it installed and clears nothing else (D071).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkerId(u64);

/// A step the program has not answered, and the thread it was aimed at.
///
/// codelldb answers a `next` with a `stopped` event naming whichever thread it
/// had selected before (D066).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutstandingStep {
    pub id: MarkerId,
    pub thread_id: i64,
}

/// The requests in flight, in the two slots they can occupy at the same time.
///
/// Two slots rather than one, because a `pause` deliberately does **not** take
/// the execution permit (D021) — it exists to interrupt a run already under
/// way, and queueing it behind that run would mean the only way to stop a
/// runaway program is to wait for it to stop. So a pause can be outstanding
/// beside the step it is interrupting, and a single slot lost one of them:
/// `step --thread A --wait` racing `pause --wait` overwrote `Step(A)` with
/// `Pause`, so the step's stop lost its thread correction *and* consumed the
/// marker, leaving the pause's own `SIGSTOP` to be reported as a genuine
/// exception. Both bugs, from one overwrite (D071).
///
/// One slot each is enough: the permit admits one step at a time, and
/// [`AdapterHandle::interrupt`](crate::adapter::AdapterHandle::interrupt) is
/// the only other thing that moves the program.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Outstanding {
    pub step: Option<OutstandingStep>,
    /// A `pause`. codelldb implements it with a `SIGSTOP` and so reports it as
    /// an exception, exactly as it reports an entry stop (D064).
    pub pause: Option<MarkerId>,
}

/// Both slots, plus the counter that names what goes in them.
#[derive(Debug, Default)]
struct Markers {
    slots: Outstanding,
    next_id: u64,
}

impl Markers {
    fn issue(&mut self) -> MarkerId {
        self.next_id += 1;
        MarkerId(self.next_id)
    }

    /// Clear whichever slot holds `id`, and nothing else.
    fn withdraw(&mut self, id: MarkerId) {
        if self.slots.step.is_some_and(|step| step.id == id) {
            self.slots.step = None;
        }
        if self.slots.pause == Some(id) {
            self.slots.pause = None;
        }
    }
}

/// One live debug session.
pub struct Session {
    pub id: SessionId,
    pub adapter_kind: AdapterKind,
    pub program: PathBuf,
    started_at: Instant,
    state: RwLock<SessionState>,
    /// Bumped every time [`Self::state`] is written.
    ///
    /// A fence for the handlers that read a paused program in more than one
    /// step. Checking "is it paused" and then awaiting an adapter round trip
    /// leaves a window: another client can `continue` in between, and the
    /// second request then reaches a *running* program — which answers with
    /// stale data or, more often, sits there until the adapter times out
    /// instead of saying `SessionNotPaused`.
    ///
    /// Capturing this at the check and comparing it before the next request
    /// closes that window. It counts writes rather than tracking the state
    /// itself on purpose: a program that resumed and stopped again is at a
    /// *different* stop, so its frame ids are new and the answer is still not
    /// the one that was asked for. This is D040's discipline, daemon-side.
    stop_generation: AtomicU64,
    exit_code: RwLock<Option<i32>>,
    ended: Mutex<bool>,
    events: Mutex<EventBuffer>,
    event_tx: broadcast::Sender<SeqEvent>,
    adapter: AdapterHandle,
    /// The thread that stopped last, so a caller can say `lazydap continue`
    /// without first asking which thread it means.
    last_thread_id: RwLock<Option<i64>>,
    /// The execution request the program has not answered yet.
    ///
    /// Two stops cannot be read from the stop alone, and both are answered by
    /// knowing what was asked for: codelldb reports a `pause` exactly as it
    /// reports an entry stop (D064), and answers a `next` aimed at one thread
    /// by naming another (D066). Written before the request goes out — an
    /// adapter can emit the `stopped` event before it acknowledges the request
    /// that caused it — and taken by the stop that answers it.
    outstanding: RwLock<Markers>,
    /// What the adapter currently thinks of the breakpoints we gave it, keyed
    /// by our id, plus the adapter's own id for each — the only way to read a
    /// `breakpoint` event or a `hitBreakpointIds` list, both of which speak
    /// adapter ids exclusively.
    breakpoints: Mutex<BreakpointMap>,
    /// The program the adapter started for us, once it has said which pid.
    ///
    /// Set only for a program *we launched*. When `attach` lands it must stay
    /// `None` for those sessions: the whole point of attaching is that the
    /// process was somebody else's first, and killing it because our adapter
    /// crashed would be destroying something we were only ever looking at.
    debuggee: Mutex<Option<Debuggee>>,
}

impl Session {
    pub fn new(
        id: SessionId,
        adapter_kind: AdapterKind,
        program: PathBuf,
        state: SessionState,
        adapter: AdapterHandle,
        event_tx: broadcast::Sender<SeqEvent>,
    ) -> Self {
        Self {
            id,
            adapter_kind,
            program,
            started_at: Instant::now(),
            state: RwLock::new(state),
            stop_generation: AtomicU64::new(0),
            exit_code: RwLock::new(None),
            ended: Mutex::new(false),
            events: Mutex::new(EventBuffer::new(EVENT_BUFFER_CAPACITY)),
            event_tx,
            adapter,
            last_thread_id: RwLock::new(None),
            outstanding: RwLock::new(Markers::default()),
            breakpoints: Mutex::new(BreakpointMap::default()),
            debuggee: Mutex::new(None),
        }
    }

    pub fn adapter(&self) -> &AdapterHandle {
        &self.adapter
    }

    /// Live events, from now on. Subscribe *before* sending the request whose
    /// consequences you are waiting for, or the fastest ones arrive first.
    pub fn subscribe(&self) -> broadcast::Receiver<SeqEvent> {
        self.event_tx.subscribe()
    }

    pub fn last_thread_id(&self) -> Option<i64> {
        *read(&self.last_thread_id)
    }

    pub fn set_last_thread_id(&self, thread_id: Option<i64>) {
        if let Some(thread_id) = thread_id {
            *write(&self.last_thread_id) = Some(thread_id);
        }
    }

    /// Record that a step is in flight, and say which marker it installed.
    pub fn expect_step(&self, thread_id: i64) -> MarkerId {
        let mut markers = write(&self.outstanding);
        let id = markers.issue();
        markers.slots.step = Some(OutstandingStep { id, thread_id });
        id
    }

    /// Record that a pause is in flight. Leaves any step slot alone: the two
    /// are separate requests and both are outstanding until each is answered.
    pub fn expect_pause(&self) -> MarkerId {
        let mut markers = write(&self.outstanding);
        let id = markers.issue();
        markers.slots.pause = Some(id);
        id
    }

    /// Forget both. A `continue` resumes the program, which makes any step
    /// still recorded finished and any pause that never landed stale.
    pub fn expect_nothing(&self) {
        write(&self.outstanding).slots = Outstanding::default();
    }

    /// Take one marker back down, by name.
    ///
    /// Used both by the stop that answers a request and by the error path when
    /// the adapter rejects one. Naming the marker is what makes the second safe:
    /// a rejected `pause` must not clear a step that replaced it in the
    /// meantime (D071).
    pub fn withdraw(&self, id: MarkerId) {
        write(&self.outstanding).withdraw(id);
    }

    /// What the program has been asked to do and has not answered.
    ///
    /// A snapshot, not a take: which of these a given stop answers depends on
    /// what the stop turns out to be, and that is the pump's judgement to make.
    pub fn outstanding(&self) -> Outstanding {
        read(&self.outstanding).slots
    }

    /// Record what the adapter made of the breakpoints in one source file.
    pub fn record_breakpoints(&self, applied: &[AdapterBreakpoint]) {
        lock(&self.breakpoints).record(applied);
    }

    /// Our id for an adapter's id, when we gave it one.
    pub fn breakpoint_id_for(&self, adapter_id: i64) -> Option<BreakpointId> {
        lock(&self.breakpoints).ours(adapter_id)
    }

    /// The adapter's current opinion of one of our breakpoints.
    pub fn breakpoint_status(&self, id: BreakpointId) -> Option<AdapterBreakpoint> {
        lock(&self.breakpoints).status(id)
    }

    /// Remember the process the adapter started for us, the first time it says.
    ///
    /// Only the first: codelldb prints its launch line once, and a later one
    /// would mean something we have no model for.
    ///
    /// The adapter's own word for *what* it started wins over what was
    /// launched, when it gives one. Usually they are the same file; under
    /// delve's `mode: "debug"` they are not, because the `.go` source was
    /// compiled to a binary somewhere else, and the reaper matching on the
    /// source path would decline to kill its own debuggee (D061).
    pub fn set_debuggee(&self, started: crate::adapter::StartedProcess) {
        let mut held = lock(&self.debuggee);
        if held.is_some() {
            return;
        }
        let program = started.program.unwrap_or_else(|| self.program.clone());
        tracing::debug!(
            target: "daemon.session",
            session_id = %self.id,
            pid = started.pid,
            program = %program.display(),
            "the adapter told us the debuggee's pid",
        );
        *held = Some(Debuggee {
            pid: started.pid,
            program,
        });
    }

    /// Kill the debuggee if the adapter died without stopping it (D045).
    ///
    /// Answers what happened, so the synthesised ending can say so.
    pub async fn reap_debuggee(&self) -> Option<String> {
        let debuggee = lock(&self.debuggee).clone()?;
        debuggee.reap().await
    }

    /// Remove a binary lazydap had delve compile, on teardown (best-effort).
    ///
    /// delve deletes it itself on a clean `disconnect`; this covers the case it
    /// did not get to — an adapter that died mid-session (delve quirk 5, D045's
    /// sibling for files rather than processes). The debuggee's program is what
    /// delve said it *ran*, which under `mode: "debug"` is the compiled binary
    /// (D061); only a path the delve adapter recognises as one of ours is
    /// touched, so an `exec`-mode debuggee the user built is never deleted. A
    /// file already gone — the ordinary `disconnect` case — is success.
    pub fn clean_compiled_artifact(&self) {
        let Some(debuggee) = lock(&self.debuggee).clone() else {
            return;
        };
        if !crate::adapter::is_compiled_artifact(&debuggee.program) {
            return;
        }
        match std::fs::remove_file(&debuggee.program) {
            Ok(()) => tracing::debug!(
                target: "daemon.session",
                session_id = %self.id,
                path = %debuggee.program.display(),
                "removed delve's compiled binary on teardown",
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                target: "daemon.session",
                session_id = %self.id,
                path = %debuggee.program.display(),
                %error,
                "could not remove delve's compiled binary",
            ),
        }
    }

    /// Fold in a `breakpoint` event so a later `break --list` reflects it.
    pub fn update_breakpoint(&self, update: &AdapterBreakpoint) {
        lock(&self.breakpoints).update(update);
    }

    /// Dress our persisted breakpoints in whatever the session knows about
    /// them.
    pub fn decorate(&self, breakpoints: Vec<lazydap_core::Breakpoint>) -> Vec<BreakpointStatus> {
        let map = lock(&self.breakpoints);
        breakpoints
            .into_iter()
            .map(|breakpoint| {
                let mut status = BreakpointStatus::unverified(breakpoint);
                if let Some(update) = map.status(status.breakpoint.id) {
                    status.apply(&update);
                }
                status
            })
            .collect()
    }

    pub fn state(&self) -> SessionState {
        *read(&self.state)
    }

    pub fn set_state(&self, state: SessionState) {
        *write(&self.state) = state;
        self.bump_stop_generation();
    }

    /// Where the session is in its stop/resume history. See
    /// [`Self::stop_generation`].
    pub fn stop_generation(&self) -> u64 {
        self.stop_generation.load(Ordering::SeqCst)
    }

    fn bump_stop_generation(&self) {
        self.stop_generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Whether the session is still sitting at the stop `fence` was taken at.
    ///
    /// The half of the fence that matters: a handler captures the generation
    /// beside its pause check and calls this immediately before the request it
    /// actually wanted to make.
    pub fn still_at(&self, fence: u64) -> bool {
        self.state() == SessionState::Paused && self.stop_generation() == fence
    }

    /// Take the session to `Running`, and say whether the adapter has to be
    /// asked to make that true.
    ///
    /// The decision and the transition are one operation under one lock, and
    /// they have to be. Sampling the state, deciding, and then writing it —
    /// which is what this replaces — leaves two windows for the pump to record
    /// a stop in between, and each one corrupts a different thing:
    ///
    /// - A stop landing **before the sample** made an already-running program
    ///   look paused, so a `continue` was sent that resumed the program past
    ///   the very stop the caller was about to be told about.
    /// - A stop landing **after the sample** was overwritten by the
    ///   unconditional `Running` that followed, leaving `--wait` returning a
    ///   paused blob while the session claimed to be running — and every later
    ///   `stack`, `scopes` and `eval` refused, because those need a stable
    ///   state.
    ///
    /// `resume_only` marks a movement a running program is *already* carrying
    /// out, which is `continue` and nothing else. For those, finding the
    /// session already `Running` means there is nothing to ask for: no request
    /// is sent, and — importantly — no state is written, so a stop the pump
    /// records a moment later stands (D055). `step` passes `false`: "step" has
    /// no reading that means "wait for whatever happens next".
    pub fn claim_run(&self, resume_only: bool) -> RunClaim {
        let mut state = write(&self.state);

        if resume_only && *state == SessionState::Running {
            tracing::debug!(
                target: "daemon.session",
                session_id = %self.id,
                "the program is already running; nothing to ask the adapter for",
            );
            return RunClaim::AlreadyRunning;
        }

        let previous = *state;
        *state = SessionState::Running;
        // The resume every fence exists to notice.
        self.bump_stop_generation();
        RunClaim::Ask { previous }
    }

    /// Undo a state this code wrote, and only if nothing has moved on since.
    ///
    /// A failed execution request wants to put the session back the way it
    /// found it. It must not do that blindly: the adapter can execute a
    /// `continue`, emit `terminated`, and die before acknowledging, so by the
    /// time the failure is handled the pump may have recorded a real ending.
    /// Stamping `paused` over that leaves a dead session looking live, which
    /// then refuses every later `launch` for a program that is not there.
    ///
    /// Compare-and-set under one lock: replaces `expected` with `previous`, or
    /// does nothing. Returns whether it did.
    pub fn restore_state(&self, expected: SessionState, previous: SessionState) -> bool {
        let mut state = write(&self.state);
        if *state != expected {
            tracing::debug!(
                target: "daemon.session",
                session_id = %self.id,
                current = state.as_str(),
                "not restoring session state; something else moved it on",
            );
            return false;
        }
        *state = previous;
        // Also a write, and a fence must not survive one: the session going
        // Paused → Running → Paused is a different stop from the one anybody
        // sampled, whichever way it got back.
        self.bump_stop_generation();
        true
    }

    pub fn set_exit_code(&self, code: Option<i32>) {
        *write(&self.exit_code) = code;
    }

    pub fn exit_code(&self) -> Option<i32> {
        *read(&self.exit_code)
    }

    /// Record an event and offer it to any subscriber.
    ///
    /// The buffer is what a later `lazydap status`, `lazydap output` and the
    /// start of a `--wait` read; the broadcast is for whoever is watching live.
    /// Both, always — a client that connects between two CLI calls should not
    /// change what the next one sees.
    ///
    /// The sequence number is assigned under the buffer lock, so the buffered
    /// copy and the broadcast copy of an event always carry the same one.
    pub fn emit(&self, event: Event) {
        let sequenced = lock(&self.events).push(event);
        // An error here only means nobody is listening, which is the normal
        // case between CLI invocations.
        let _ = self.event_tx.send(sequenced);
    }

    /// Everything buffered that no `--wait` has reported yet, and the sequence
    /// number to ignore live events at or below.
    ///
    /// Output a program produced between two CLI invocations belongs in the
    /// next blob — nobody has seen it, and a `continue --wait` that dropped it
    /// would lose the reason the program is where it is.
    ///
    /// **Reads without consuming.** Delivery is committed by
    /// [`mark_delivered`](Self::mark_delivered) once a blob is actually
    /// returned: a wait whose request is rejected never reports anything, and
    /// marking its backlog delivered would lose those events for good.
    pub fn undelivered(&self) -> (Vec<Event>, u64, u64) {
        lock(&self.events).undelivered()
    }

    /// Record that everything up to `seq` has been reported to a caller.
    ///
    /// [`take_undelivered`](Self::take_undelivered) marks what it drains, but a
    /// wait goes on to consume events *live* for as long as it runs, and those
    /// are just as delivered. Without this the next wait re-reports them, and
    /// the second `continue --wait` of a session carries the first one's
    /// output — which looks exactly like the program printing twice.
    pub fn mark_delivered(&self, seq: u64) {
        lock(&self.events).mark_delivered(seq);
    }

    /// Buffered debuggee output, optionally from a moment onwards. A read, not
    /// a drain: `lazydap output` twice shows the same thing twice.
    pub fn buffered_output(&self, since_ms: Option<u64>) -> (Vec<OutputChunk>, u64) {
        lock(&self.events).output(since_ms)
    }

    /// End the session exactly once.
    ///
    /// Adapters usually send `terminated` and *then* close the socket, so the
    /// pump sees two endings for one death. The first wins; the second is
    /// dropped. Returns whether this call was the one that ended it.
    pub fn end_once(&self, reason: EndReason) -> bool {
        let mut ended = lock(&self.ended);
        if *ended {
            return false;
        }
        *ended = true;

        self.set_state(match &reason {
            EndReason::Exited { .. } => SessionState::Exited,
            EndReason::AdapterDied { .. } => SessionState::AdapterDied,
            EndReason::Disconnected | EndReason::Terminated => SessionState::Terminated,
        });
        drop(ended);

        self.emit(Event::SessionEnded {
            session_id: self.id,
            reason,
        });
        true
    }

    pub fn summary(&self) -> SessionSummary {
        let events = lock(&self.events);
        SessionSummary {
            session_id: self.id,
            adapter: self.adapter_kind,
            program: self.program.clone(),
            state: self.state(),
            exit_code: self.exit_code(),
            buffered_events: events.len(),
            captured_output_chunks: events.output_chunks(),
            dropped_events: events.dropped,
            uptime_ms: self.started_at.elapsed().as_millis() as u64,
        }
    }

    /// Seed the buffer with output captured before the pump took over.
    pub fn seed_events(&self, events: Vec<Event>) {
        let mut buffer = lock(&self.events);
        for event in events {
            buffer.push(event);
        }
    }
}

/// A bounded, drop-oldest ring of events, each with its position in the
/// session's history.
struct EventBuffer {
    events: VecDeque<SeqEvent>,
    capacity: usize,
    dropped: u64,
    /// Events that fell out of the buffer before any wait carried them.
    /// Distinct from `dropped`, which is the session-lifetime total the
    /// `output` command reports: this one is reset every time a wait reports
    /// it, because it answers "is the blob I am about to hand back whole".
    undelivered_loss: u64,
    next_seq: u64,
    /// The highest sequence number handed to a `--wait`.
    delivered: u64,
}

impl EventBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.min(64)),
            capacity,
            dropped: 0,
            undelivered_loss: 0,
            next_seq: 1,
            delivered: 0,
        }
    }

    fn push(&mut self, event: Event) -> SeqEvent {
        let sequenced = SeqEvent {
            seq: self.next_seq,
            event,
        };
        self.next_seq += 1;

        if self.events.len() == self.capacity {
            // Whatever fell off was never delivered; a wait that reports fewer
            // events than happened must not also claim to have seen them.
            if let Some(lost) = self.events.pop_front() {
                // Advancing `delivered` past it is what stops the next wait
                // trying to re-report an event that no longer exists — and it
                // is also what made the loss invisible, because the gap it
                // leaves is exactly the thing a wait needs to know about.
                // Counted here, before it is papered over (D072).
                if lost.seq > self.delivered {
                    self.undelivered_loss += 1;
                }
                self.delivered = self.delivered.max(lost.seq);
            }
            self.dropped += 1;
        }
        self.events.push_back(sequenced.clone());
        sequenced
    }

    /// The backlog, the watermark, and how much of the backlog is missing.
    ///
    /// The third number is the one a `--wait` cannot do without. A debuggee
    /// that prints more than [`EVENT_BUFFER_CAPACITY`] events before anybody
    /// calls `continue --wait` pushes the beginning of its own output out of
    /// the buffer, and the wait then hands back a *suffix* while reporting
    /// nothing wrong — the same lie as a spliced middle, from the other end
    /// (D072).
    fn undelivered(&self) -> (Vec<Event>, u64, u64) {
        let undelivered: Vec<Event> = self
            .events
            .iter()
            .filter(|sequenced| sequenced.seq > self.delivered)
            .map(|sequenced| sequenced.event.clone())
            .collect();

        // The watermark is the newest event that exists, not merely the newest
        // one still in the buffer: anything dropped is gone, and re-reporting
        // it later is impossible either way.
        (undelivered, self.next_seq - 1, self.undelivered_loss)
    }

    /// Everything up to `seq` has reached a caller — including the loss, which
    /// has now been reported and must not be counted again by the next wait.
    fn mark_delivered(&mut self, seq: u64) {
        self.delivered = self.delivered.max(seq);
        self.undelivered_loss = 0;
    }

    fn output(&self, since_ms: Option<u64>) -> (Vec<OutputChunk>, u64) {
        let chunks = self
            .events
            .iter()
            .filter_map(|sequenced| match &sequenced.event {
                Event::Output { chunk, .. } => Some(chunk),
                _ => None,
            })
            .filter(|chunk| since_ms.is_none_or(|since| chunk.timestamp_ms >= since))
            .cloned()
            .collect();
        (chunks, self.dropped)
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    fn output_chunks(&self) -> usize {
        self.events
            .iter()
            .filter(|sequenced| matches!(sequenced.event, Event::Output { .. }))
            .count()
    }
}

/// Which of our breakpoints the adapter is calling what, and what it thinks of
/// them.
///
/// Two directions are needed and neither is derivable from the other: an
/// adapter id has to become ours (a `stopped` event lists the adapter's), and
/// one of ours has to yield its current state (`break --list` wants to say
/// whether it verified).
#[derive(Default)]
struct BreakpointMap {
    by_ours: HashMap<BreakpointId, AdapterBreakpoint>,
}

impl BreakpointMap {
    fn record(&mut self, applied: &[AdapterBreakpoint]) {
        for breakpoint in applied {
            if let Some(id) = breakpoint.id {
                self.by_ours.insert(id, breakpoint.clone());
            }
        }
    }

    fn ours(&self, adapter_id: i64) -> Option<BreakpointId> {
        self.by_ours
            .iter()
            .find(|(_, breakpoint)| breakpoint.adapter_id == Some(adapter_id))
            .map(|(id, _)| *id)
    }

    fn status(&self, id: BreakpointId) -> Option<AdapterBreakpoint> {
        self.by_ours.get(&id).cloned()
    }

    /// Fold in a `breakpoint` event, which arrives keyed by the adapter's id.
    ///
    /// An update for a breakpoint we never set — the adapter's own, or one
    /// from a previous session — is dropped rather than invented into the map.
    fn update(&mut self, update: &AdapterBreakpoint) {
        let Some(adapter_id) = update.adapter_id else {
            return;
        };
        let Some(ours) = self.ours(adapter_id) else {
            return;
        };
        if let Some(existing) = self.by_ours.get_mut(&ours) {
            existing.verified = update.verified;
            if update.line.is_some() {
                existing.line = update.line;
            }
            if update.message.is_some() {
                existing.message = update.message.clone();
            }
        }
    }
}

/// Lock helpers that treat a poisoned lock as usable.
///
/// A panic while holding one of these leaves a `SessionState` or a counter
/// mid-update, not a corrupt invariant, and refusing to serve every later
/// request over it would be a worse failure than carrying on.
fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::OutputCategory;

    fn state() -> Arc<DaemonState> {
        let root = std::env::temp_dir().join(format!(
            "lazydap-state-{}-{:?}",
            std::process::id(),
            std::thread::current().id(),
        ));
        std::fs::create_dir_all(&root).expect("create the project root");
        DaemonState::new(
            "lazydap-test".to_string(),
            ProjectStore::load(&root).expect("load the store"),
        )
    }

    /// A session whose adapter has already gone — the state every session
    /// reaches, and the one the ending rules are about.
    fn ended_session() -> Session {
        let (event_tx, _keep_open) = tokio::sync::broadcast::channel(16);
        Session::new(
            SessionId::new(),
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            crate::adapter::AdapterHandle::detached(),
            event_tx,
        )
    }

    #[test]
    fn a_second_launch_is_rejected_while_one_is_already_reserved() {
        let state = state();
        let first = SessionId::new();
        let _reservation = state.reserve(first).expect("the first launch wins");

        let error = match state.reserve(SessionId::new()) {
            Err(error) => error,
            Ok(_) => unreachable!("a second launch must be refused"),
        };
        assert_eq!(error.code, ErrorCode::SessionAlreadyActive, "got: {error}");
        assert_eq!(
            error.details["session_id"],
            first.to_string(),
            "the error should name the session in the way",
        );
        assert_eq!(error.details["state"], "launching");
    }

    #[test]
    fn a_session_is_announced_only_once_it_can_be_found() {
        // The bug this pins: `SessionStarted` used to be emitted before the
        // reservation was promoted, so a client that subscribed in between got
        // a snapshot with no session *and* had already missed the event.
        let state = state();
        let session_id = SessionId::new();
        let reservation = state.reserve(session_id).expect("reserve");
        let session = Arc::new(Session::new(
            session_id,
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            crate::adapter::AdapterHandle::detached(),
            state.events(),
        ));

        let mut events = state.events().subscribe();
        assert!(
            events.try_recv().is_err(),
            "a reservation is not an announcement",
        );
        assert!(state.active_session().is_none());

        reservation.promote(session);

        match events.try_recv().expect("the session should be announced") {
            SeqEvent {
                event: Event::SessionStarted { session_id: id, .. },
                ..
            } => assert_eq!(id, session_id),
            other => unreachable!("expected a session-started event, got: {other:?}"),
        }
        assert!(
            state.active_session().is_some(),
            "and be findable by anyone the announcement reaches",
        );
    }

    #[test]
    fn a_launch_that_fails_frees_the_slot() {
        let state = state();

        let reservation = state.reserve(SessionId::new()).expect("reserve");
        drop(reservation); // as if the adapter failed to start

        state
            .reserve(SessionId::new())
            .expect("the next launch should be allowed to try");
    }

    #[test]
    fn a_reservation_is_not_yet_a_session() {
        let state = state();
        let _reservation = state.reserve(SessionId::new()).expect("reserve");

        assert!(
            state.active_session().is_none(),
            "a half-launched session must not show up as active",
        );
        assert!(state.status().session.is_none());
    }

    #[test]
    fn a_late_exit_code_still_reaches_status_after_the_session_ended() {
        // DAP does not guarantee `exited` arrives before `terminated`, so a
        // code recorded after the ending must not be dropped.
        let session = ended_session();
        assert!(session.end_once(EndReason::Terminated));

        session.set_exit_code(Some(3));

        assert_eq!(session.exit_code(), Some(3));
        assert!(
            !session.end_once(EndReason::Exited { exit_code: Some(3) }),
            "the session ends once, however many endings arrive",
        );
    }

    #[test]
    fn disconnecting_an_already_finished_session_keeps_how_it_finished() {
        // `lazydap disconnect` after the debuggee ran to completion must not
        // relabel a clean exit as a termination.
        let session = ended_session();
        session.set_exit_code(Some(0));
        assert!(session.end_once(EndReason::Exited { exit_code: Some(0) }));

        assert!(!session.end_once(EndReason::Disconnected));
        assert_eq!(session.state(), SessionState::Exited);
        assert_eq!(session.summary().exit_code, Some(0));
    }

    #[test]
    fn the_first_ending_wins_so_a_dead_adapter_cannot_rewrite_it() {
        let session = ended_session();
        assert!(session.end_once(EndReason::Exited { exit_code: Some(0) }));
        assert_eq!(session.state(), SessionState::Exited);

        // What the pump does when the socket closes behind an adapter that
        // has already reported the debuggee finishing.
        assert!(!session.end_once(EndReason::AdapterDied {
            detail: "eof".to_string(),
        }));
        assert_eq!(
            session.state(),
            SessionState::Exited,
            "a clean exit must not be relabelled as a crash",
        );
    }

    #[test]
    fn the_event_buffer_keeps_the_newest_events_and_counts_output() {
        let mut buffer = EventBuffer::new(2);
        let session_id = SessionId::new();

        buffer.push(Event::SessionStarted {
            session_id,
            adapter: AdapterKind::Codelldb,
        });
        buffer.push(output_event(session_id, "first"));
        buffer.push(output_event(session_id, "second"));

        assert_eq!(buffer.len(), 2, "the buffer is capped");
        assert_eq!(buffer.dropped, 1);
        assert_eq!(
            buffer.output_chunks(),
            2,
            "the oldest event was the one dropped"
        );
    }

    #[test]
    fn a_wait_is_told_about_output_that_arrived_before_it_started() {
        // Output produced between two CLI invocations has no other way to
        // reach the caller: the next `--wait` blob is the only thing it reads.
        let session = ended_session();
        session.emit(output_event(session.id, "printed while nobody was asking"));

        let (undelivered, watermark, _) = session.undelivered();
        assert_eq!(undelivered.len(), 1);
        assert_eq!(watermark, 1);
    }

    #[test]
    fn the_same_event_is_never_reported_to_two_waits_that_both_finished() {
        let session = ended_session();
        session.emit(output_event(session.id, "first"));

        let (first, watermark, _) = session.undelivered();
        session.mark_delivered(watermark);
        let (second, ..) = session.undelivered();

        assert_eq!(first.len(), 1);
        assert!(
            second.is_empty(),
            "a second wait must not re-report what the first already carried",
        );
    }

    #[test]
    fn a_wait_that_never_returned_a_blob_leaves_its_backlog_for_the_next_one() {
        // Reading the backlog is not the same as reporting it. A `continue`
        // the adapter rejects produces no blob, and marking those events
        // delivered would lose them with nobody having seen them.
        let session = ended_session();
        session.emit(output_event(session.id, "nobody has seen this"));

        let (peeked, ..) = session.undelivered();
        assert_eq!(peeked.len(), 1, "the failed wait read it");

        let (still_there, ..) = session.undelivered();
        assert_eq!(
            still_there.len(),
            1,
            "and left it, because it never reported anything",
        );
    }

    #[test]
    fn the_watermark_covers_events_the_buffer_dropped_on_the_floor() {
        // Otherwise a wait would drain the two survivors, set the watermark to
        // their sequence, and then treat the *next* live event as old.
        let session = Session::new(
            SessionId::new(),
            AdapterKind::Codelldb,
            PathBuf::from("/tmp/hello"),
            SessionState::Running,
            crate::adapter::AdapterHandle::detached(),
            tokio::sync::broadcast::channel(16).0,
        );
        for index in 0..(EVENT_BUFFER_CAPACITY + 5) {
            session.emit(output_event(session.id, &format!("line {index}")));
        }

        let (undelivered, watermark, _) = session.undelivered();
        assert_eq!(undelivered.len(), EVENT_BUFFER_CAPACITY);
        assert_eq!(
            watermark,
            (EVENT_BUFFER_CAPACITY + 5) as u64,
            "the watermark tracks what happened, not what survived",
        );
    }

    #[test]
    fn output_can_be_read_from_a_moment_onwards_without_consuming_it() {
        let session = ended_session();
        session.emit(output_event(session.id, "old"));
        let cutoff = lazydap_core::now_ms() + 1;
        std::thread::sleep(std::time::Duration::from_millis(2));
        session.emit(output_event(session.id, "new"));

        let (recent, _) = session.buffered_output(Some(cutoff));
        assert_eq!(recent.len(), 1, "got: {recent:?}");
        assert_eq!(recent[0].output, "new");

        let (all, _) = session.buffered_output(None);
        assert_eq!(all.len(), 2, "reading must not consume");
    }

    /// The first of the two interleavings `claim_run` exists for: a `continue`
    /// arriving while the program is running.
    ///
    /// Sampling the state and then writing it left a window where the pump
    /// recorded a stop in between; the handler saw `Paused`, sent `continue`,
    /// and resumed the program past the very stop the caller was about to be
    /// told about. Deciding under the lock is what closes it — a stop can only
    /// land before the claim (in which case this is an ordinary resume) or
    /// after it (in which case nothing was sent and the stop stands).
    #[test]
    fn a_continue_on_a_running_program_asks_for_nothing() {
        let session = ended_session();
        session.set_state(SessionState::Running);

        assert_eq!(session.claim_run(true), RunClaim::AlreadyRunning);
        assert_eq!(
            session.state(),
            SessionState::Running,
            "and the state is the pump's to move, not ours",
        );
    }

    /// The second interleaving: the stop lands *after* the decision.
    ///
    /// The unconditional `set_state(Running)` that used to follow the sample
    /// overwrote it, so `--wait` returned a paused blob while the session
    /// claimed to be running — and every later `stack`, `scopes` and `eval`
    /// was refused, because those need a stable state.
    #[test]
    fn a_suppressed_continue_never_overwrites_a_stop_that_lands_after_it() {
        let session = ended_session();
        session.set_state(SessionState::Running);

        assert_eq!(session.claim_run(true), RunClaim::AlreadyRunning);
        // The pump, a moment later.
        session.set_state(SessionState::Paused);

        assert_eq!(
            session.state(),
            SessionState::Paused,
            "the suppression path writes no state, so the stop stands",
        );
    }

    #[test]
    fn a_fence_survives_nothing_happening_and_nothing_else() {
        // The window it closes: a handler checks "is it paused", awaits the
        // adapter to resolve a frame, and by the time it sends the request it
        // actually wanted, another client has resumed the program. The second
        // request then reaches a running program — stale values, or a ten
        // second adapter timeout instead of `SessionNotPaused`.
        let session = ended_session();
        session.set_state(SessionState::Paused);
        let fence = session.stop_generation();

        assert!(session.still_at(fence), "nothing has moved");

        // Another client's `continue`.
        session.claim_run(true);
        assert!(!session.still_at(fence), "the program is running now");
    }

    #[test]
    fn a_fence_does_not_survive_a_resume_and_a_second_stop() {
        // The subtle half: the session is paused again, so re-reading the
        // state alone would say yes. It is a *different* stop — every frame id
        // resolved before it addresses nothing — so the answer is still no.
        let session = ended_session();
        session.set_state(SessionState::Paused);
        let fence = session.stop_generation();

        session.claim_run(true);
        session.set_state(SessionState::Paused);

        assert_eq!(session.state(), SessionState::Paused);
        assert!(
            !session.still_at(fence),
            "paused again is not the stop that was asked about",
        );
    }

    #[test]
    fn restoring_a_state_moves_the_fence_too() {
        // Otherwise a failed execution request putting the session back would
        // let a fence taken before it survive, which is the same lie.
        let session = ended_session();
        session.set_state(SessionState::Paused);
        let fence = session.stop_generation();

        session.claim_run(true);
        assert!(session.restore_state(SessionState::Running, SessionState::Paused));

        assert!(!session.still_at(fence));
    }

    #[test]
    fn a_continue_on_a_paused_program_asks_the_adapter_and_takes_it_running() {
        let session = ended_session();
        session.set_state(SessionState::Paused);

        assert_eq!(
            session.claim_run(true),
            RunClaim::Ask {
                previous: SessionState::Paused,
            },
        );
        assert_eq!(session.state(), SessionState::Running);
    }

    #[test]
    fn a_step_is_always_asked_for_even_on_a_running_program() {
        // "step" has no reading that means "wait for whatever happens next",
        // so there is nothing to suppress. It reaches the adapter and, on a
        // running program, may well reach a timeout — the same on both
        // adapters, and a known follow-up rather than something D055 fixed.
        let session = ended_session();
        session.set_state(SessionState::Running);

        assert_eq!(
            session.claim_run(false),
            RunClaim::Ask {
                previous: SessionState::Running,
            },
        );
    }

    #[test]
    fn a_failed_request_puts_back_only_the_state_it_wrote() {
        let session = ended_session();
        session.set_state(SessionState::Running);

        assert!(
            session.restore_state(SessionState::Running, SessionState::Paused),
            "nothing else had touched it",
        );
        assert_eq!(session.state(), SessionState::Paused);
    }

    #[test]
    fn a_failed_request_does_not_resurrect_a_session_that_has_since_ended() {
        // The adapter can execute a `continue`, emit `terminated` and die
        // before acknowledging. By the time the failure is handled the pump
        // has recorded the ending, and stamping `paused` over it would leave a
        // dead session looking live — refusing every later launch for a
        // program that is not there.
        let session = ended_session();
        session.set_state(SessionState::Running);
        session.end_once(EndReason::Exited { exit_code: Some(0) });

        assert!(
            !session.restore_state(SessionState::Running, SessionState::Paused),
            "the pump moved it on; the restore must decline",
        );
        assert_eq!(session.state(), SessionState::Exited);
    }

    #[test]
    fn an_adapter_breakpoint_id_maps_back_to_ours() {
        let session = ended_session();
        session.record_breakpoints(&[AdapterBreakpoint {
            id: Some(BreakpointId(7)),
            adapter_id: Some(3),
            verified: false,
            line: None,
            message: None,
        }]);

        assert_eq!(session.breakpoint_id_for(3), Some(BreakpointId(7)));
        assert_eq!(
            session.breakpoint_id_for(99),
            None,
            "an id we never set must not resolve to one we did",
        );
    }

    #[test]
    fn a_breakpoint_event_updates_what_a_later_list_reports() {
        // codelldb verifies lazily: the response says false, an event says
        // true and moves the line. A `break --list` afterwards has to agree
        // with the debugger, not with the request we sent.
        let session = ended_session();
        session.record_breakpoints(&[AdapterBreakpoint {
            id: Some(BreakpointId(1)),
            adapter_id: Some(5),
            verified: false,
            line: None,
            message: None,
        }]);

        session.update_breakpoint(&AdapterBreakpoint {
            id: None,
            adapter_id: Some(5),
            verified: true,
            line: Some(21),
            message: None,
        });

        let decorated = session.decorate(vec![lazydap_core::Breakpoint {
            id: BreakpointId(1),
            source: PathBuf::from("/tmp/main.c"),
            line: 19,
            column: None,
            condition: None,
            hit_condition: None,
            log_message: None,
            enabled: true,
        }]);

        assert!(decorated[0].verified);
        assert_eq!(decorated[0].effective_line(), 21);
    }

    #[test]
    fn an_update_for_a_breakpoint_we_never_set_is_ignored() {
        let session = ended_session();
        session.update_breakpoint(&AdapterBreakpoint {
            id: None,
            adapter_id: Some(404),
            verified: true,
            line: Some(1),
            message: None,
        });
        assert_eq!(session.breakpoint_id_for(404), None);
    }

    fn output_event(session_id: SessionId, text: &str) -> Event {
        Event::Output {
            session_id,
            chunk: OutputChunk::new(OutputCategory::Stdout, text),
        }
    }
}
