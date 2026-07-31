//! What can happen, and what the reducer can ask for in return.
//!
//! The two halves of D012's `(State, Msg) -> (State, Cmd)`: [`Msg`] is every
//! way the world reaches the TUI, [`Cmd`] every way the TUI reaches back. The
//! reducer is a pure function between them, which is what makes "add a key"
//! mean "add a match arm" and nothing else.

use lazydap_protocol::{Event, IpcError, Request, Response};
use ratatui::crossterm::event::KeyEvent;
use std::path::PathBuf;

/// Something happened.
#[derive(Debug, Clone)]
pub enum Msg {
    Key(KeyEvent),
    /// A connection to the daemon is up, and this is the first thing the
    /// reducer hears about it.
    ///
    /// Init is a reducer decision rather than something the loop hard-codes,
    /// which is what lets a *re*-connection (M19) re-run exactly the same
    /// opening moves as the first one instead of a second copy of them.
    Connected,
    /// Text pasted into the terminal, delivered whole rather than as the
    /// keystrokes it resembles.
    ///
    /// The distinction is a safety property, not a convenience. Without
    /// bracketed paste, pasting `counter\nc` into the add-watch prompt is
    /// indistinguishable from typing it: the newline submits, and the `c` that
    /// follows reaches the global bindings and resumes the debuggee. As one
    /// message it can be routed to whatever is being typed into, or dropped
    /// when nothing is.
    Paste(String),
    /// The terminal changed size. Carries no size: the next draw asks the
    /// frame for its own area, so a stored copy could only ever disagree with
    /// it. The message exists to make that draw happen now.
    Resize,
    /// The redraw heartbeat. Nothing depends on it yet; it is what lets a
    /// pane that ages (an elapsed timer, a spinner) redraw without an input.
    Tick,
    /// The terminal stopped producing input, so no key can ever arrive again
    /// — including the one that quits.
    ///
    /// Fatal on purpose. The alternative, found in review, is a TUI spinning
    /// in raw mode with no working quit key, which leaves the user killing it
    /// from another terminal.
    InputClosed,
    /// A [`Cmd::LoadSource`] finished. Carries the error rather than the file
    /// contents when it did not, because "the file is missing" is something
    /// the user needs to see and the reducer is where seeing it is decided.
    ///
    /// `id` is the one the [`Cmd::LoadSource`] carried. Reads finish in
    /// whatever order the filesystem manages, not the order they were asked
    /// for, so the reducer needs it to tell the answer it is waiting for from
    /// one it has been overtaken by.
    SourceLoaded {
        id: u64,
        path: PathBuf,
        contents: std::result::Result<String, String>,
    },

    /// The daemon says something changed. Unsolicited, and the reason the TUI
    /// never polls.
    DaemonEvent(Event),
    /// An answer to something the TUI asked.
    ///
    /// Boxed because `Response` is by far the biggest thing that can happen —
    /// a stack trace or a page of variables — and every `Msg` on the channel
    /// would otherwise be sized for it, `Tick` included.
    DaemonResponse {
        id: u64,
        response: Box<Response>,
    },
    /// A request the daemon refused. Shown, not swallowed: "step" doing
    /// nothing with no explanation is the worst kind of debugger bug.
    DaemonFailed {
        id: u64,
        error: IpcError,
    },
    /// The connection ended.
    DaemonGone,
    /// A [`Cmd::Reconnect`] finished. `Err` carries what to tell the user about
    /// the attempt that failed (M19).
    ///
    /// `attempt` is the one this answers. Without it a reply from an attempt
    /// that has already been superseded would be taken for the current one, and
    /// two ladders would climb at once.
    Reconnected {
        attempt: u32,
        outcome: std::result::Result<(), String>,
    },
}

/// Something the reducer wants done. Executed by the loop, never by the
/// reducer itself — that is what keeps the reducer pure and testable.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Nothing to do. The overwhelmingly common answer.
    None,
    Quit,
    /// Read a file. `id` comes back on the [`Msg::SourceLoaded`] so a stale
    /// answer can be told from the current one.
    LoadSource {
        id: u64,
        path: PathBuf,
    },
    /// Ask the daemon something. The answer arrives as a [`Msg`], never as a
    /// return value: the reducer does no I/O.
    ///
    /// The `id` is chosen by the reducer rather than by the write pump, which
    /// is the only way a reply can be matched to the thing that asked for it
    /// (D040). [`lazydap_protocol::Response::Variables`] is a bare list of
    /// variables — nothing in it says which node was being expanded — and a
    /// stack trace for a stop the program has already left looks exactly like
    /// one for the stop it is on.
    SendIpc {
        id: u64,
        request: Request,
    },
    /// Several commands, in order.
    ///
    /// Sequential and dumb: the loop runs them one after another. It exists
    /// because one message can genuinely need two things — a stop needs both
    /// the stack and the scopes, and jumping to a frame needs both its file
    /// and its variables — and nesting those into one `Cmd` variant each would
    /// be a variant per pair.
    Batch(Vec<Cmd>),
    /// Try to get a connection to the daemon back, starting one if there is
    /// none (M19). Answered with [`Msg::Reconnected`].
    Reconnect {
        /// Which attempt this is, carried back on the answer so a reply that
        /// has been superseded can be told from the current one.
        attempt: u32,
        /// How long to wait first. The reducer owns the backoff, so the shape
        /// of the retry curve is testable without waiting for it.
        delay_ms: u64,
    },
}
