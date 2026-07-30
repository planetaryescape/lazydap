//! `lazydap`'s terminal UI.
//!
//! A **client**, not a peer. The crate graph enforces that: this crate may
//! depend on `core`, `protocol` and `config` and nothing else
//! (`ARCHITECTURE.md`, checked by `scripts/check_architecture_boundaries.sh`),
//! so a feature that only the TUI has is not something that can be built here
//! — it has to go through the protocol, which means the CLI gets it too
//! (non-negotiable 2).
//!
//! # Shape
//!
//! A hand-written Elm architecture (D012). Four pieces, and every change to
//! the TUI belongs to exactly one of them:
//!
//! - [`state::AppState`] — everything known.
//! - [`msg::Msg`] — everything that can happen.
//! - [`update::update`] — the pure `(State, Msg) -> (State, Cmd)` reducer.
//! - [`view::view`] — the drawing, which decides nothing.
//!
//! The loop below owns the terminal, turns the world into `Msg`s, and runs the
//! [`msg::Cmd`]s the reducer asks for. It is the only part that does I/O, so
//! it is the only part that is awkward to test — which is why it is kept this
//! small.

mod error;
mod ipc_client;
mod msg;
mod panes;
mod state;
#[cfg(test)]
mod testing;
mod update;
mod view;

pub use error::{Result, TuiError};

use lazydap_protocol::{EventKind, Request};
use msg::{Cmd, Msg};
use ratatui::crossterm::event::{self, Event};
use state::AppState;
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc::{self, UnboundedSender};

/// How long the input thread waits for a key before looking around.
///
/// It is a blocking read, so this is also how long it takes the thread to
/// notice the TUI has quit.
const INPUT_POLL: Duration = Duration::from_millis(100);

/// How often the loop redraws with nothing else to go on.
///
/// Ten a second. A debugger's screen changes when the program does or when a
/// key is pressed, and both of those wake the loop directly; the tick only has
/// to cover whatever ages on its own.
const TICK: Duration = Duration::from_millis(100);

/// What the TUI watches.
///
/// Deliberately not everything. `Output` is the chatty one and there is no
/// pane to put it in until M17; `BreakpointUpdated` and `ThreadChanged` land
/// with the panes that show them. A client that subscribed to everything would
/// make the daemon do work nobody reads.
const CHANNELS: [EventKind; 4] = [
    EventKind::SessionStarted,
    EventKind::SessionEnded,
    EventKind::Stopped,
    EventKind::Continued,
];

/// Run the TUI until the user quits.
///
/// Connects to the daemon at `socket`, which must already be running — see
/// [`ipc_client::connect`] for why starting one is the caller's job. Connecting
/// happens *before* the terminal is taken over, so a daemon that cannot be
/// reached is an ordinary error message on an ordinary terminal rather than a
/// banner inside a UI the user then has to quit.
///
/// Once running, the terminal is given back whatever happens, including a
/// panic: `ratatui::try_init` installs a hook that restores it before
/// unwinding. A debugger that leaves your shell in raw mode when it crashes is
/// worse than no debugger.
pub async fn run(socket: &Path) -> Result<()> {
    tracing::debug!(target: "tui.lifecycle", socket = %socket.display(), "entering the TUI");
    let (tx, mut rx) = mpsc::unbounded_channel();

    let ipc = ipc_client::connect(socket, tx.clone()).await?;
    // Subscribing is also how the TUI learns what is already going on: the
    // reply is a snapshot taken at the moment the stream starts (D038).
    ipc.send(Request::Subscribe {
        channels: CHANNELS.to_vec(),
    });

    spawn_input_pump(tx.clone());

    let mut terminal = ratatui::try_init()?;
    let mut state = AppState::default();

    let mut tick = tokio::time::interval(TICK);
    let result = loop {
        if let Err(error) = terminal.draw(|frame| view::view(frame, &mut state)) {
            break Err(error.into());
        }

        // Both arms are cancellation-safe, so whichever loses the race is
        // dropped without losing anything (`docs/reference/tokio-patterns.md`).
        let msg = tokio::select! {
            received = rx.recv() => match received {
                Some(msg) => msg,
                // Only reachable if every sender is gone, which means the
                // input pump died. Carrying on would be a TUI no key can quit.
                None => break Ok(()),
            },
            _ = tick.tick() => Msg::Tick,
        };

        let (next, cmd) = update::update(state, msg);
        state = next;

        if matches!(cmd, Cmd::Quit) {
            break Ok(());
        }
        dispatch(cmd, &tx, &ipc);
    };

    // Unconditional: a loop that failed still borrowed the terminal.
    ratatui::restore();
    tracing::debug!(target: "tui.lifecycle", "left the TUI");
    result
}

/// Run what the reducer asked for.
///
/// Everything here is fire-and-forget: a command's *result* comes back as a
/// [`Msg`], which is what keeps the reducer the only place state changes.
fn dispatch(cmd: Cmd, tx: &UnboundedSender<Msg>, ipc: &ipc_client::IpcClient) {
    match cmd {
        Cmd::None => {}
        // Handled by the loop, which is the only thing that can stop it.
        Cmd::Quit => {}
        Cmd::SendIpc(request) => ipc.send(request),
        Cmd::LoadSource(path) => {
            let tx = tx.clone();
            tokio::spawn(async move {
                let contents = tokio::fs::read_to_string(&path)
                    .await
                    .map_err(|error| error.to_string());
                let _ = tx.send(Msg::SourceLoaded { path, contents });
            });
        }
    }
}

/// Turn keystrokes into messages.
///
/// `spawn_blocking` because `event::poll` blocks a whole thread, and blocking
/// a runtime worker would stall every other task (`tokio-patterns.md`).
///
/// The closed-channel check at the top of the loop is load-bearing, and was
/// missing at first: a blocking task cannot be aborted, and dropping the
/// runtime *waits* for it. Noticing the TUI had quit only when the next
/// keystroke failed to send meant `q` left the process sitting there until
/// somebody pressed another key. Polling for it costs one check per
/// [`INPUT_POLL`] and bounds the exit at that.
fn spawn_input_pump(tx: UnboundedSender<Msg>) {
    tokio::task::spawn_blocking(move || {
        loop {
            if tx.is_closed() {
                tracing::debug!(target: "tui.input", "the TUI has quit; stopping the input pump");
                return;
            }

            let ready = event::poll(INPUT_POLL).and_then(|ready| match ready {
                true => event::read().map(Some),
                false => Ok(None),
            });

            let msg = match ready {
                Ok(Some(Event::Key(key))) => Msg::Key(key),
                Ok(Some(Event::Resize(..))) => Msg::Resize,
                // Nothing to read, or a mouse or focus event nothing acts on.
                Ok(_) => continue,
                Err(error) => {
                    tracing::warn!(target: "tui.input", %error, "stopped reading the terminal");
                    return;
                }
            };

            if tx.send(msg).is_err() {
                return;
            }
        }
    });
}
