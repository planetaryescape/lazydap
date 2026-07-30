//! Everything the TUI knows.
//!
//! One struct, owned by the loop and handed to the reducer by value. Fields
//! are crate-visible rather than public: the reducer is the only thing that
//! writes them (D012), and the crate boundary is what enforces that.

use crate::panes::source::SourceView;

#[derive(Default)]
pub struct AppState {
    /// The file on screen, once one has loaded.
    pub(crate) source: Option<SourceView>,
    /// A message that replaces the usual position readout in the status row.
    ///
    /// For things the user has to be told and cannot infer from the screen —
    /// a file that would not open, and from M11 a daemon that went away.
    pub(crate) notice: Option<String>,
    /// The first half of a two-key sequence, i.e. the `g` of `gg`.
    ///
    /// In the state rather than in the loop because it *is* state: whether the
    /// next `g` means "go to the top" depends on what came before it.
    pub(crate) awaiting_g: bool,
}
