//! Domain types every other lazydap crate agrees on.
//!
//! Zero I/O by construction, and no internal dependencies: `core` is the floor
//! of the dependency graph (see `ARCHITECTURE.md`), so nothing here may reach
//! for a socket, a file, or a child process. Types that describe *how* clients
//! and the daemon talk live in `lazydap-protocol`; types that describe *what*
//! they are talking about live here.

mod session;

pub use session::{
    AdapterKind, EndReason, OutputCategory, OutputChunk, PauseReason, SessionId, SessionState,
    UnknownAdapter, now_ms,
};
