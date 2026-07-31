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
/// Here rather than in one of the panes because all of them draw it, and a
/// shared rule living inside one of its users is how two of them end up
/// disagreeing about what "focused" looks like.
pub(crate) fn border_style(focused: bool) -> Style {
    match focused {
        true => Style::default().fg(Color::Cyan),
        false => Style::default().fg(Color::DarkGray),
    }
}

/// The first line of an adapter's complaint, which is all a pane row has space
/// for.
///
/// Here for [`border_style`]'s reason: the watches pane and the REPL both show
/// errors the adapter wrote, both get multi-line diagnostics — codelldb follows
/// an error with `note:` lines — and a row that grew to three would push the
/// rest of the pane off the screen.
pub(crate) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_string()
}
