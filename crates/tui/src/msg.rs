//! What can happen, and what the reducer can ask for in return.
//!
//! The two halves of D012's `(State, Msg) -> (State, Cmd)`: [`Msg`] is every
//! way the world reaches the TUI, [`Cmd`] every way the TUI reaches back. The
//! reducer is a pure function between them, which is what makes "add a key"
//! mean "add a match arm" and nothing else.

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
    /// A [`Cmd::LoadSource`] finished. Carries the error rather than the file
    /// contents when it did not, because "the file is missing" is something
    /// the user needs to see and the reducer is where seeing it is decided.
    SourceLoaded {
        path: PathBuf,
        contents: std::result::Result<String, String>,
    },
}

/// Something the reducer wants done. Executed by the loop, never by the
/// reducer itself — that is what keeps the reducer pure and testable.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// Nothing to do. The overwhelmingly common answer.
    None,
    Quit,
    LoadSource(PathBuf),
}
