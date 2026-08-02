//! The lazydap IPC protocol: what clients and the daemon say to each other.
//!
//! Length-delimited JSON over a Unix socket (D004). One envelope type
//! ([`IpcMessage`]) carries requests, responses, events and errors, and every
//! envelope states the protocol version it was written against.
//!
//! This crate depends on `lazydap-core` and nothing else internal — that is
//! what lets a TUI, a web client or an agent skill speak the protocol without
//! being able to reach into the daemon (`ARCHITECTURE.md`, D005).
//!
//! The full schema, including the parts not yet implemented, is
//! `docs/blueprint/04-protocol.md`.

mod codec;
mod connection;
mod types;

pub use codec::IpcCodec;
pub use connection::IpcConnection;
pub use types::{
    AdapterCapabilities, BreakpointAction, BreakpointReport, DoctorCheck, DoctorReport, ErrorCode,
    Event, EventKind, FrameLocals, IpcError, IpcMessage, IpcPayload, LAZYDAP_PROTOCOL_VERSION,
    LaunchRequest, Request, Response, SessionSummary, StableState, StatusReport, VariableList,
    WaitMode, WatchAction, WatchReport,
};
