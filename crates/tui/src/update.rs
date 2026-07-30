//! The reducer: `(State, Msg) -> (State, Cmd)`.
//!
//! Pure. No I/O, no clock, no terminal — anything that touches the world is
//! returned as a [`Cmd`] for the loop to run. That is the whole point of D012:
//! every state transition in the TUI is reachable from a test, and adding a
//! key is adding one arm to one match.

use crate::msg::{Cmd, Msg};
use crate::panes::source::SourceView;
use crate::state::AppState;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

pub fn update(state: AppState, msg: Msg) -> (AppState, Cmd) {
    match msg {
        // Nothing to store: every draw asks the frame for its own area. The
        // message exists so the loop wakes and repaints now rather than at the
        // next tick, which is the difference between a resize that looks
        // instant and one that looks stuck.
        Msg::Resize | Msg::Tick => (state, Cmd::None),
        Msg::SourceLoaded { path, contents } => source_loaded(state, path, contents),
        // Terminals with the kitty protocol on report releases and repeats as
        // well as presses. Acting on all three turns one keystroke into three.
        Msg::Key(key) if key.kind != KeyEventKind::Press => (state, Cmd::None),
        Msg::Key(key) => key_press(state, key),
    }
}

fn key_press(mut state: AppState, key: KeyEvent) -> (AppState, Cmd) {
    // Every key clears the pending prefix, including the one that consumes it.
    // `gj` is not `gg`, and must not leave the `g` armed for the next key.
    let awaiting_g = std::mem::take(&mut state.awaiting_g);
    let control = key.modifiers.contains(KeyModifiers::CONTROL);

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return (state, Cmd::Quit),
        KeyCode::Char('g') if awaiting_g => with_source(&mut state, SourceView::go_to_top),
        KeyCode::Char('g') => state.awaiting_g = true,
        KeyCode::Char('G') => with_source(&mut state, SourceView::go_to_bottom),
        KeyCode::Char('j') | KeyCode::Down => with_source(&mut state, |s| s.move_cursor(1)),
        KeyCode::Char('k') | KeyCode::Up => with_source(&mut state, |s| s.move_cursor(-1)),
        KeyCode::Char('d') if control => with_source(&mut state, |s| s.move_cursor(s.half_page())),
        KeyCode::Char('u') if control => with_source(&mut state, |s| s.move_cursor(-s.half_page())),
        _ => {}
    }

    (state, Cmd::None)
}

fn source_loaded(
    mut state: AppState,
    path: std::path::PathBuf,
    contents: std::result::Result<String, String>,
) -> (AppState, Cmd) {
    match contents {
        Ok(contents) => {
            state.source = Some(SourceView::from_contents(path, &contents));
            state.notice = None;
        }
        Err(error) => {
            // Not fatal. The TUI without a file is still a TUI, and telling
            // the user why beats an empty pane they have to guess about.
            tracing::warn!(target: "tui.source", file = %path.display(), %error, "could not open the file");
            state.notice = Some(format!("{}: {error}", path.display()));
        }
    }
    (state, Cmd::None)
}

/// Do something to the open file, if there is one.
///
/// The `Option` is checked here rather than in every arm, so a key that moves
/// the cursor is one line whether or not a file happens to be loaded.
fn with_source(state: &mut AppState, action: impl FnOnce(&mut SourceView)) {
    if let Some(source) = state.source.as_mut() {
        action(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn loaded(lines: u32) -> AppState {
        let body: Vec<String> = (1..=lines).map(|line| format!("line {line}")).collect();
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/numbers.txt"),
                contents: Ok(body.join("\n")),
            },
        );
        state
    }

    fn press(state: AppState, code: KeyCode) -> (AppState, Cmd) {
        update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn press_control(state: AppState, code: KeyCode) -> (AppState, Cmd) {
        update(state, Msg::Key(KeyEvent::new(code, KeyModifiers::CONTROL)))
    }

    fn cursor(state: &AppState) -> u32 {
        state.source.as_ref().expect("a loaded file").cursor_line()
    }

    #[test]
    fn q_and_escape_ask_the_loop_to_stop() {
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            let (_, cmd) = press(loaded(3), code);
            assert_eq!(cmd, Cmd::Quit, "{code:?}");
        }
    }

    #[test]
    fn j_and_k_and_the_arrows_move_one_line() {
        let mut state = loaded(10);
        for code in [KeyCode::Char('j'), KeyCode::Down] {
            (state, _) = press(state, code);
        }
        assert_eq!(cursor(&state), 3);

        for code in [KeyCode::Char('k'), KeyCode::Up] {
            (state, _) = press(state, code);
        }
        assert_eq!(cursor(&state), 1);
    }

    #[test]
    fn shift_g_goes_to_the_end_and_gg_comes_back() {
        let (state, _) = press(loaded(10), KeyCode::Char('G'));
        assert_eq!(cursor(&state), 10);

        let (state, _) = press(state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 10, "one g on its own moves nothing");
        assert!(state.awaiting_g, "but it does arm the next one");

        let (state, _) = press(state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 1);
        assert!(!state.awaiting_g, "and disarms it again");
    }

    #[test]
    fn a_g_followed_by_anything_else_is_not_a_gg() {
        // Otherwise `gj` leaves the prefix armed and the *next* `g` — pressed
        // for some other reason entirely — jumps to the top.
        let (state, _) = press(loaded(10), KeyCode::Char('G'));
        let (state, _) = press(state, KeyCode::Char('g'));
        let (state, _) = press(state, KeyCode::Char('j'));
        assert!(!state.awaiting_g);

        let (state, _) = press(state, KeyCode::Char('g'));
        assert_eq!(cursor(&state), 10, "the j consumed the prefix");
    }

    #[test]
    fn control_d_and_control_u_move_by_half_a_page() {
        // Nothing has been drawn, so half a page is the one-line floor.
        let (state, _) = press_control(loaded(50), KeyCode::Char('d'));
        assert_eq!(cursor(&state), 2);

        let (state, _) = press_control(state, KeyCode::Char('u'));
        assert_eq!(cursor(&state), 1);
    }

    #[test]
    fn d_and_u_without_control_are_not_scrolls() {
        let (state, _) = press(loaded(50), KeyCode::Char('d'));
        let (state, _) = press(state, KeyCode::Char('u'));
        assert_eq!(cursor(&state), 1);
    }

    #[test]
    fn an_unbound_key_changes_nothing_and_asks_for_nothing() {
        let (state, cmd) = press(loaded(10), KeyCode::Char('z'));
        assert_eq!(cursor(&state), 1);
        assert_eq!(cmd, Cmd::None);
    }

    #[test]
    fn a_key_release_is_not_a_key_press() {
        let state = loaded(10);
        let mut release = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;

        let (state, _) = update(state, Msg::Key(release));
        assert_eq!(cursor(&state), 1, "one keystroke is one movement");
    }

    #[test]
    fn scrolling_with_no_file_open_is_a_no_op_rather_than_a_panic() {
        let mut state = AppState::default();
        for code in [KeyCode::Char('j'), KeyCode::Char('G'), KeyCode::Char('g')] {
            (state, _) = press(state, code);
        }
        assert!(state.source.is_none());
    }

    #[test]
    fn a_tick_and_a_resize_change_nothing() {
        let before = loaded(10);
        let (after, cmd) = update(before, Msg::Tick);
        assert_eq!(cmd, Cmd::None);

        let (after, cmd) = update(after, Msg::Resize);
        assert_eq!(cmd, Cmd::None);
        assert_eq!(cursor(&after), 1);
    }

    #[test]
    fn a_file_that_would_not_open_is_reported_rather_than_swallowed() {
        let (state, cmd) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("No such file or directory (os error 2)".to_string()),
            },
        );

        assert_eq!(cmd, Cmd::None);
        assert!(state.source.is_none());
        assert_eq!(
            state.notice.as_deref(),
            Some("/tmp/gone.c: No such file or directory (os error 2)"),
        );
    }

    #[test]
    fn loading_a_file_clears_the_complaint_about_the_last_one() {
        let (state, _) = update(
            AppState::default(),
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/gone.c"),
                contents: Err("no".to_string()),
            },
        );
        let (state, _) = update(
            state,
            Msg::SourceLoaded {
                path: PathBuf::from("/tmp/there.c"),
                contents: Ok("int main(void) {}".to_string()),
            },
        );

        assert!(state.notice.is_none());
        assert!(state.source.is_some());
    }
}
