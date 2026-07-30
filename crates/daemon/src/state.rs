use crate::adapter::AdapterHandle;
use lazydap_core::{AdapterKind, EndReason, SessionId, SessionState};
use lazydap_protocol::{
    ErrorCode, Event, IpcError, LAZYDAP_PROTOCOL_VERSION, SessionSummary, StatusReport,
};
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
    started_at: Instant,
    /// Keyed by id even though v0.1 allows one at a time (D007): the map is
    /// what makes lifting that limit a daemon-only change.
    sessions: RwLock<HashMap<SessionId, Slot>>,
    event_tx: broadcast::Sender<Event>,
    shutdown_tx: watch::Sender<bool>,
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
    pub fn new(instance: String) -> Arc<Self> {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let (shutdown_tx, _) = watch::channel(false);
        Arc::new(Self {
            instance,
            started_at: Instant::now(),
            sessions: RwLock::new(HashMap::new()),
            event_tx,
            shutdown_tx,
        })
    }

    pub fn uptime_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    pub fn events(&self) -> broadcast::Sender<Event> {
        self.event_tx.clone()
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
    pub fn promote(mut self, session: Arc<Session>) {
        write(&self.state.sessions).insert(self.id, Slot::Live(session));
        self.promoted = true;
    }
}

impl Drop for SessionReservation {
    fn drop(&mut self) {
        if !self.promoted {
            write(&self.state.sessions).remove(&self.id);
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
    exit_code: RwLock<Option<i32>>,
    ended: Mutex<bool>,
    events: Mutex<EventBuffer>,
    event_tx: broadcast::Sender<Event>,
    adapter: AdapterHandle,
}

impl Session {
    pub fn new(
        id: SessionId,
        adapter_kind: AdapterKind,
        program: PathBuf,
        state: SessionState,
        adapter: AdapterHandle,
        event_tx: broadcast::Sender<Event>,
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
        }
    }

    pub fn adapter(&self) -> &AdapterHandle {
        &self.adapter
    }

    pub fn state(&self) -> SessionState {
        *read(&self.state)
    }

    pub fn set_state(&self, state: SessionState) {
        *write(&self.state) = state;
    }

    pub fn set_exit_code(&self, code: Option<i32>) {
        *write(&self.exit_code) = code;
    }

    pub fn exit_code(&self) -> Option<i32> {
        *read(&self.exit_code)
    }

    /// Record an event and offer it to any subscriber.
    ///
    /// The buffer is what a later `lazydap status` (and M6's `--wait`) reads;
    /// the broadcast is for clients watching live. Both, always — a client
    /// that connects between two CLI calls should not change what the next one
    /// sees.
    pub fn emit(&self, event: Event) {
        lock(&self.events).push(event.clone());
        // Errors here only mean nobody is listening, which is the normal case.
        let _ = self.event_tx.send(event);
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

/// A bounded, drop-oldest ring of events.
struct EventBuffer {
    events: VecDeque<Event>,
    capacity: usize,
    dropped: u64,
}

impl EventBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity.min(64)),
            capacity,
            dropped: 0,
        }
    }

    fn push(&mut self, event: Event) {
        if self.events.len() == self.capacity {
            self.events.pop_front();
            self.dropped += 1;
        }
        self.events.push_back(event);
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    fn output_chunks(&self) -> usize {
        self.events
            .iter()
            .filter(|event| matches!(event, Event::Output { .. }))
            .count()
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
    use lazydap_core::{OutputCategory, OutputChunk};

    fn state() -> Arc<DaemonState> {
        DaemonState::new("lazydap-test".to_string())
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
        let output = |text: &str| Event::Output {
            session_id,
            chunk: OutputChunk::new(OutputCategory::Stdout, text),
        };

        buffer.push(Event::SessionStarted {
            session_id,
            adapter: AdapterKind::Codelldb,
        });
        buffer.push(output("first"));
        buffer.push(output("second"));

        assert_eq!(buffer.len(), 2, "the buffer is capped");
        assert_eq!(buffer.dropped, 1);
        assert_eq!(
            buffer.output_chunks(),
            2,
            "the oldest event was the one dropped"
        );
    }
}
