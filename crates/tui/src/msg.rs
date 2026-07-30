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
    /// The connection ended. Terminal for this run of the TUI — reconnection
    /// is a v0.1 job, and until then saying so beats a screen that has quietly
    /// stopped being true.
    DaemonGone,
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
    SendIpc(Request),
}
