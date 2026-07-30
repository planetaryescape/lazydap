use lazydap_core::{AdapterKind, EndReason, OutputChunk, PauseReason, SessionId, SessionState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The version every message in this build is written against.
///
/// Bumping it is a breaking change to a product surface and needs an entry in
/// `docs/blueprint/15-decision-log.md`. Clients and daemon compare versions on
/// the first exchange; see [`Request::Ping`].
pub const LAZYDAP_PROTOCOL_VERSION: u32 = 1;

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
/// Bucketed per `ARCHITECTURE.md`: diagnostics first, then session control.
/// M5 implements the handful below; the rest of the schema in
/// `docs/blueprint/04-protocol.md` lands with M6 and M11.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Request {
    // --- Diagnostics ---
    /// Version handshake and liveness probe. The first thing every client
    /// sends: the daemon answers [`Response::Pong`] with its own version, and
    /// a mismatch means the client is talking to a daemon from another build.
    Ping,
    Status,
    /// Ask the daemon to end every session and exit.
    Shutdown,

    // --- Session ---
    Launch(LaunchRequest),
    Disconnect {
        session_id: SessionId,
        /// Kill the debuggee too, rather than leaving it running detached.
        terminate: bool,
    },

    // --- Not implemented yet ---
    /// Event streaming for long-lived clients. The variant exists so the
    /// wire format is settled; the daemon answers [`ErrorCode::Unsupported`]
    /// until the TUI needs it at M11.
    Subscribe {
        channels: Vec<EventKind>,
    },
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Response {
    Pong {
        version: u32,
        instance: String,
        uptime_ms: u64,
    },
    Status(StatusReport),
    Launched {
        session_id: SessionId,
        state: SessionState,
        /// Present when the launch ended with the debuggee already paused,
        /// i.e. `stop_on_entry`.
        reason: Option<PauseReason>,
        thread_id: Option<i64>,
        capabilities: AdapterCapabilities,
    },
    Disconnected {
        session_id: SessionId,
    },
    ShuttingDown,
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
        reason: PauseReason,
        all_threads_stopped: bool,
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
}

impl Event {
    pub fn kind(&self) -> EventKind {
        match self {
            Self::SessionStarted { .. } => EventKind::SessionStarted,
            Self::SessionEnded { .. } => EventKind::SessionEnded,
            Self::Stopped { .. } => EventKind::Stopped,
            Self::Continued { .. } => EventKind::Continued,
            Self::Output { .. } => EventKind::Output,
        }
    }

    pub fn session_id(&self) -> SessionId {
        match self {
            Self::SessionStarted { session_id, .. }
            | Self::SessionEnded { session_id, .. }
            | Self::Stopped { session_id, .. }
            | Self::Continued { session_id, .. }
            | Self::Output { session_id, .. } => *session_id,
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
            reason: PauseReason::Entry,
            all_threads_stopped: true,
        };
        assert_eq!(event.kind(), EventKind::Stopped);
        assert_eq!(event.session_id(), session_id);
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
