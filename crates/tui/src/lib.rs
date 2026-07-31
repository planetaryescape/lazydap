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

use msg::{Cmd, Msg};
use ratatui::crossterm::event::{self, Event};
use state::AppState;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{self, UnboundedSender};

/// How the TUI gets a daemon back after the one it was talking to goes away.
///
/// A callback rather than a call, because starting a daemon means starting a
/// *process*, and this crate may not depend on the crate that can do that
/// (`ARCHITECTURE.md`) — the same boundary that stops the TUI reaching the
/// daemon's internals. `lazydap`'s entry point supplies one that runs the same
/// `ensure_daemon_running` every subcommand takes (D003), so a TUI left open
/// across `lazydap shutdown` recovers exactly as the next CLI command would.
pub type EnsureDaemon = Arc<
    dyn Fn() -> Pin<Box<dyn Future<Output = std::result::Result<(), String>> + Send>> + Send + Sync,
>;

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
pub async fn run(socket: &Path, ensure_daemon: EnsureDaemon) -> Result<()> {
    tracing::debug!(target: "tui.lifecycle", socket = %socket.display(), "entering the TUI");
    let socket = socket.to_path_buf();
    let (tx, mut rx) = mpsc::unbounded_channel();
    // A reconnection produces a whole new client, which only the loop may
    // install — it is what `dispatch` sends through. So the reconnecting task
    // hands it back rather than swapping it in from another thread.
    let (clients, mut reconnected) = mpsc::unbounded_channel();

    let mut ipc = ipc_client::connect(&socket, tx.clone()).await?;
    // Subscribing is a reducer decision, not a hard-coded first move, so that
    // a reconnection replays exactly the same opening. `Msg::Connected` is
    // what starts it.
    let _ = tx.send(Msg::Connected);

    spawn_input_pump(tx.clone());

    // `try_init` enables raw mode and enters the alternate screen *before* the
    // step that can fail (asking the terminal its size), so returning its error
    // straight out would leave the shell in raw mode with no prompt. Put it
    // back first, then complain.
    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            ratatui::restore();
            return Err(error.into());
        }
    };
    // After the terminal is ours, and undone before it is given back.
    //
    // Without it a paste is delivered as the keystrokes it looks like, so
    // pasting `counter\nc` into the add-watch prompt submitted `counter` on the
    // newline and then fed `c` to the global bindings — which resumed the
    // debuggee. Enabling it turns the whole paste into one event that can be
    // routed to whatever is being typed into.
    let bracketed = enable_bracketed_paste();
    let mut state = AppState::default();

    let mut tick = tokio::time::interval(TICK);
    let result = loop {
        if let Err(error) = terminal.draw(|frame| view::view(frame, &mut state)) {
            break Err(error.into());
        }

        // Every arm is cancellation-safe, so whichever loses the race is
        // dropped without losing anything (`docs/reference/tokio-patterns.md`).
        let msg = tokio::select! {
            received = rx.recv() => match received {
                Some(msg) => msg,
                // Only reachable if every sender is gone, which means the
                // input pump died. Carrying on would be a TUI no key can quit.
                None => break Ok(()),
            },
            // Installed before the reducer hears about it, so the requests it
            // asks for in reply go down the new connection rather than the dead
            // one — but only if this is still the attempt being waited on. A
            // connection from an attempt that lost its race would replace a
            // working one with an unsubscribed one, and every request after
            // that would go somewhere nobody is listening.
            Some((attempt, client)) = reconnected.recv() => {
                if !state.is_awaiting(attempt) {
                    tracing::debug!(
                        target: "tui.ipc",
                        attempt,
                        "dropping a connection from a superseded reconnection",
                    );
                    continue;
                }
                ipc = client;
                Msg::Reconnected { attempt, outcome: Ok(()) }
            },
            _ = tick.tick() => Msg::Tick,
        };

        let (next, cmd) = update::update(state, msg);
        state = next;

        if matches!(cmd, Cmd::Quit) {
            break Ok(());
        }
        Dispatcher {
            msgs: &tx,
            ipc: &ipc,
            socket: &socket,
            ensure_daemon: &ensure_daemon,
            clients: &clients,
        }
        .run(cmd);
    };

    // Unconditional: a loop that failed still borrowed the terminal. Paste mode
    // goes back first — leaving it on would have the user's shell receiving
    // `\x1b[200~` wrappers around everything they paste after quitting.
    if bracketed {
        disable_bracketed_paste();
    }
    ratatui::restore();
    tracing::debug!(target: "tui.lifecycle", "left the TUI");
    result
}

/// Everything a [`Cmd`] might need to reach the world with.
///
/// A struct rather than six arguments threaded through a recursive call: the
/// set grows every time a command needs something new, and `Cmd::Batch` passes
/// all of it along to itself.
struct Dispatcher<'a> {
    msgs: &'a UnboundedSender<Msg>,
    ipc: &'a ipc_client::IpcClient,
    socket: &'a Path,
    ensure_daemon: &'a EnsureDaemon,
    /// Where a reconnection hands its new client back to the loop.
    clients: &'a UnboundedSender<(u32, ipc_client::IpcClient)>,
}

impl Dispatcher<'_> {
    /// Run what the reducer asked for.
    ///
    /// Everything here is fire-and-forget: a command's *result* comes back as
    /// a [`Msg`], which is what keeps the reducer the only place state changes.
    fn run(&self, cmd: Cmd) {
        match cmd {
            Cmd::None => {}
            // Handled by the loop, which is the only thing that can stop it.
            Cmd::Quit => {}
            Cmd::Batch(cmds) => cmds.into_iter().for_each(|cmd| self.run(cmd)),
            Cmd::SendIpc { id, request } => self.ipc.send(id, request),
            Cmd::LoadSource { id, path } => {
                let msgs = self.msgs.clone();
                tokio::spawn(async move {
                    let contents = tokio::fs::read_to_string(&path)
                        .await
                        .map_err(|error| error.to_string());
                    let _ = msgs.send(Msg::SourceLoaded { id, path, contents });
                });
            }
            Cmd::Reconnect { attempt, delay_ms } => spawn_reconnect(
                attempt,
                delay_ms,
                self.socket.to_path_buf(),
                self.msgs.clone(),
                self.ensure_daemon.clone(),
                self.clients.clone(),
            ),
        }
    }
}

/// Wait, start a daemon if there is none, and connect to it (M19).
///
/// A task rather than an inline await: the delay is up to four seconds, and a
/// loop that spent them blocked would stop drawing *and* stop reading keys —
/// so the user could not even quit while it waited.
fn spawn_reconnect(
    attempt: u32,
    delay_ms: u64,
    socket: PathBuf,
    tx: UnboundedSender<Msg>,
    ensure_daemon: EnsureDaemon,
    clients: UnboundedSender<(u32, ipc_client::IpcClient)>,
) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;

        if let Err(error) = ensure_daemon().await {
            let _ = tx.send(Msg::Reconnected {
                attempt,
                outcome: Err(error),
            });
            return;
        }
        match ipc_client::connect(&socket, tx.clone()).await {
            // The loop installs it and turns it into a `Msg::Reconnected`.
            // Sent this way round because a client cannot travel in a `Msg`:
            // messages are `Clone`, and a connection is not. The attempt rides
            // along so the loop can tell a current one from a superseded one.
            Ok(client) => {
                let _ = clients.send((attempt, client));
            }
            Err(error) => {
                let _ = tx.send(Msg::Reconnected {
                    attempt,
                    outcome: Err(error.to_string()),
                });
            }
        }
    });
}

/// Ask the terminal to wrap pasted text so it can be told from typing.
///
/// Returns whether it took. Not every terminal supports it, and one that does
/// not is not a reason to refuse to start — it only means a paste arrives as
/// keystrokes, which is what happened everywhere before this.
fn enable_bracketed_paste() -> bool {
    use ratatui::crossterm::event::EnableBracketedPaste;
    match ratatui::crossterm::execute!(std::io::stdout(), EnableBracketedPaste) {
        Ok(()) => true,
        Err(error) => {
            tracing::debug!(
                target: "tui.input",
                %error,
                "this terminal will not bracket pastes; they arrive as keystrokes",
            );
            false
        }
    }
}

fn disable_bracketed_paste() {
    use ratatui::crossterm::event::DisableBracketedPaste;
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableBracketedPaste);
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
                Ok(Some(Event::Paste(text))) => Msg::Paste(text),
                Ok(Some(Event::Resize(..))) => Msg::Resize,
                // Nothing to read, or a mouse or focus event nothing acts on.
                Ok(_) => continue,
                Err(error) => {
                    // Say so before going. The render loop holds other senders,
                    // so it would not notice this channel's producer leaving —
                    // it would keep drawing, in raw mode, with no key able to
                    // reach it and therefore no way to quit.
                    tracing::warn!(target: "tui.input", %error, "stopped reading the terminal");
                    let _ = tx.send(Msg::InputClosed);
                    return;
                }
            };

            if tx.send(msg).is_err() {
                return;
            }
        }
    });
}
