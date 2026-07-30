//! The panes the TUI is made of.
//!
//! One module per pane, each owning its own scroll and selection state and
//! knowing how to draw itself into a `Rect`. The reducer (M10) moves them; it
//! does not reach into their fields.

pub mod source;
