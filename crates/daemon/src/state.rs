use crate::adapter::AdapterHandle;
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

/// One live debug session.
pub struct Session {
    pub id: SessionId,
    pub adapter_kind: AdapterKind,
    pub program: PathBuf,
    started_at: Instant,
    state: RwLock<SessionState>,
    exit_code: RwLock<Option<i32>>,
    ended: Mutex<bool>,
    events: Mutex<EventBuffer>,
    event_tx: broadcast::Sender<SeqEvent>,
    adapter: AdapterHandle,
    /// The thread that stopped last, so a caller can say `lazydap continue`
    /// without first asking which thread it means.
    last_thread_id: RwLock<Option<i64>>,
    /// What the adapter currently thinks of the breakpoints we gave it, keyed
    /// by our id, plus the adapter's own id for each — the only way to read a
    /// `breakpoint` event or a `hitBreakpointIds` list, both of which speak
    /// adapter ids exclusively.
    breakpoints: Mutex<BreakpointMap>,
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
            exit_code: RwLock::new(None),
            ended: Mutex::new(false),
            events: Mutex::new(EventBuffer::new(EVENT_BUFFER_CAPACITY)),
            event_tx,
            adapter,
            last_thread_id: RwLock::new(None),
            breakpoints: Mutex::new(BreakpointMap::default()),
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
    pub fn undelivered(&self) -> (Vec<Event>, u64) {
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
                self.delivered = self.delivered.max(lost.seq);
            }
            self.dropped += 1;
        }
        self.events.push_back(sequenced.clone());
        sequenced
    }

    fn undelivered(&self) -> (Vec<Event>, u64) {
        let undelivered: Vec<Event> = self
            .events
            .iter()
            .filter(|sequenced| sequenced.seq > self.delivered)
            .map(|sequenced| sequenced.event.clone())
            .collect();

        // The watermark is the newest event that exists, not merely the newest
        // one still in the buffer: anything dropped is gone, and re-reporting
        // it later is impossible either way.
        (undelivered, self.next_seq - 1)
    }

    fn mark_delivered(&mut self, seq: u64) {
        self.delivered = self.delivered.max(seq);
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

        let (undelivered, watermark) = session.undelivered();
        assert_eq!(undelivered.len(), 1);
        assert_eq!(watermark, 1);
    }

    #[test]
    fn the_same_event_is_never_reported_to_two_waits_that_both_finished() {
        let session = ended_session();
        session.emit(output_event(session.id, "first"));

        let (first, watermark) = session.undelivered();
        session.mark_delivered(watermark);
        let (second, _) = session.undelivered();

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

        let (peeked, _) = session.undelivered();
        assert_eq!(peeked.len(), 1, "the failed wait read it");

        let (still_there, _) = session.undelivered();
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

        let (undelivered, watermark) = session.undelivered();
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
