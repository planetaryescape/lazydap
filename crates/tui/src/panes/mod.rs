//! The panes the TUI is made of.
//!
//! One module per pane, each owning its own scroll and selection state and
//! knowing how to draw itself into a `Rect`. The reducer (M10) moves them; it
//! does not reach into their fields.

pub mod input;
pub mod repl;
pub mod scopes;
pub mod source;
pub mod stack;
pub mod watches;

use ratatui::style::{Color, Style};

/// How a pane's border says whether the keys are going to it.
///
/// Here rather than in one of the panes because all three draw it, and a
/// shared rule living inside one of its users is how two of them end up
/// disagreeing about what "focused" looks like.
pub(crate) fn border_style(focused: bool) -> Style {
    match focused {
        true => Style::default().fg(Color::Cyan),
        false => Style::default().fg(Color::DarkGray),
    }
}
