//! Domain types every other lazydap crate agrees on.
//!
//! Zero I/O by construction, and no internal dependencies: `core` is the floor
//! of the dependency graph (see `ARCHITECTURE.md`), so nothing here may reach
//! for a socket, a file, or a child process. Types that describe *how* clients
//! and the daemon talk live in `lazydap-protocol`; types that describe *what*
//! they are talking about live here.

mod breakpoint;
mod inspect;
mod launch;
mod selector;
mod session;
mod watch;

pub use breakpoint::{
    AdapterBreakpoint, BadLocation, Breakpoint, BreakpointId, BreakpointStatus, Location,
    NewBreakpoint,
};
pub use inspect::{
    BadValue, EvalContext, EvalResult, Scope, SourceRef, StackFrame, StepKind, ThreadInfo,
    ThreadUpdate, ThreadUpdateKind, Variable, VariableFilter, WaitOutcome,
};
pub use launch::{LaunchConfig, LaunchConfigSource, LaunchKind, NotRunnable};
pub use selector::BreakpointSelector;
pub use session::{
    AdapterKind, EndReason, OutputCategory, OutputChunk, PauseReason, SessionId, SessionState,
    UnknownAdapter, now_ms,
};
pub use watch::{NewWatch, Watch, WatchId, WatchSelector, WatchValue};
