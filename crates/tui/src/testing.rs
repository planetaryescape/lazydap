//! Rendering a view without a terminal.
//!
//! ratatui's `TestBackend` gives a real `Buffer`, so a snapshot is an
//! assertion about what the user sees rather than about which widgets were
//! constructed. Only the symbols are compared: colour and emphasis are real
//! decisions but not ones a string comparison judges usefully, and a test that
//! breaks every time a border changes shade gets deleted rather than fixed.

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Draw `view` into a `width`×`height` terminal and read back the text.
pub fn render(width: u16, height: u16, view: impl FnOnce(&mut Frame)) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("a test terminal");
    terminal.draw(view).expect("draw");

    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}
