//! The daemon: one Unix socket, many clients, one debug session.

use crate::error::{CliError, Result};
use crate::handlers;
use crate::instance::Instance;
use crate::state::{DaemonState, SeqEvent};
use lazydap_core::EndReason;
use lazydap_protocol::{
    ErrorCode, Event, EventKind, IpcConnection, IpcError, IpcMessage, IpcPayload,
    LAZYDAP_PROTOCOL_VERSION, Request, Response,
};
use lazydap_store::ProjectStore;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::task::JoinSet;

/// How long to let in-flight client connections finish during shutdown.
const CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Bind the socket and serve until asked to stop.
pub async fn run_daemon(instance: Instance) -> Result<()> {
    // Refuse rather than steal: a socket someone answers on belongs to a
    // daemon that is already doing this job.
    if is_reachable(&instance.socket).await {
        return Err(CliError::general(anyhow::anyhow!(
            "a daemon is already running for instance `{}` at {}",
            instance.name,
            instance.socket.display(),
        )));
    }
    if instance.socket.exists() {
        // Unanswered, so it is left over from a daemon that is gone.
        fs::remove_file(&instance.socket).map_err(CliError::general)?;
    }

    let listener = UnixListener::bind(&instance.socket).map_err(|source| {
        CliError::general(anyhow::anyhow!(
            "cannot bind {}: {source}",
            instance.socket.display()
        ))
    })?;
    // The socket is an unauthenticated control channel for a debugger. Nobody
    // else on the machine gets to open it.
    fs::set_permissions(&instance.socket, fs::Permissions::from_mode(0o600))
        .map_err(CliError::general)?;
    write_pid_file(&instance.pid)?;

    let store = ProjectStore::load(&instance.project_root).map_err(|source| {
        CliError::general(anyhow::anyhow!(
            "cannot read {}: {source}",
            instance.project_root.display()
        ))
    })?;
    // One flusher per store, for the life of the daemon. Mutations only mark
    // the state dirty; this is what actually writes it (debounced, D006).
    tokio::spawn(Arc::clone(&store).run_flusher());

    let state = DaemonState::new(instance.name.clone(), store);
    spawn_signal_watch(Arc::clone(&state));

    tracing::info!(
        target: "daemon.ipc",
        instance = %instance.name,
        socket = %instance.socket.display(),
        pid = std::process::id(),
        "daemon listening",
    );

    let result = accept_loop(&listener, &state).await;

    // Teardown runs whatever the loop did, so a failure cannot leave adapters
    // running or a socket file lying around for the next client to trust.
    shut_down_sessions(&state).await;
    // The debounce window is 500ms and the daemon is about to stop: whatever
    // is still only in memory has to reach the disk now or never.
    if let Err(error) = state.store.flush_now() {
        tracing::warn!(target: "daemon.store", %error, "could not persist project state on shutdown");
    }
    drop(listener);
    let _ = fs::remove_file(&instance.socket);
    clear_pid_file_if_ours(&instance.pid);
    tracing::info!(target: "daemon.ipc", "daemon stopped");

    result
}

async fn accept_loop(listener: &UnixListener, state: &Arc<DaemonState>) -> Result<()> {
    let mut shutdown = state.shutdown_receiver();
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                // A closed channel means the state is going away; either way,
                // stop accepting.
                if changed.is_err() || *shutdown.borrow_and_update() {
                    break;
                }
            }
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        connections.spawn(serve_client(stream, Arc::clone(state)));
                    }
                    Err(error) => {
                        tracing::error!(target: "daemon.ipc", %error, "accept failed");
                        return Err(CliError::general(error));
                    }
                }
            }
            Some(joined) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = joined {
                    tracing::warn!(target: "daemon.ipc", %error, "a client task failed");
                }
            }
        }
    }

    drain(&mut connections).await;
    Ok(())
}

/// Read requests from one client until it hangs up or the daemon stops.
///
/// Requests on a connection are handled one at a time. A CLI client sends one
/// and waits, so concurrency here would buy nothing; separate clients already
/// get separate tasks.
///
/// A client that has sent [`Request::Subscribe`] also gets events pushed at it
/// between replies, on the same connection. Sends happen in the *body* of a
/// `select!` arm and never as one: [`IpcConnection::send`] is not
/// cancellation-safe, and a cancelled send leaves half a frame on the wire.
pub async fn serve_client(stream: UnixStream, state: Arc<DaemonState>) {
    let mut connection = IpcConnection::new(stream);
    let mut shutdown = state.shutdown_receiver();
    let mut subscription: Option<Subscription> = None;

    loop {
        if state.shutdown_requested() {
            break;
        }

        let incoming = tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            incoming = connection.recv() => incoming,
            event = next_event(subscription.as_mut()) => {
                match event {
                    Some(event) => {
                        if let Err(error) = connection.send(IpcMessage::event(event)).await {
                            tracing::debug!(target: "daemon.ipc", %error, "subscriber went away");
                            break;
                        }
                    }
                    // The broadcast closed, which only happens as the daemon
                    // goes away. Stop pushing; the shutdown arm ends the loop.
                    None => subscription = None,
                }
                continue;
            }
        };

        let message = match incoming {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(error) => {
                // A frame we cannot read is not a frame we can answer in
                // context, so say what happened on id 0 and hang up rather
                // than leaving the client waiting for a reply that will never
                // correlate.
                tracing::warn!(target: "daemon.ipc", %error, "unreadable frame from a client");
                let _ = connection
                    .send(IpcMessage::error(
                        0,
                        IpcError::new(ErrorCode::BadRequest, error.to_string()),
                    ))
                    .await;
                break;
            }
        };

        // Subscribing is the one request that changes the connection rather
        // than the daemon, so it is answered here instead of in `dispatch`.
        let reply = match subscribe_request(&message) {
            Some(channels) => subscribe(&state, &mut subscription, channels, message.id),
            None => handle_message(&state, message).await,
        };
        if let Err(error) = connection.send(reply).await {
            tracing::debug!(target: "daemon.ipc", %error, "client went away mid-reply");
            break;
        }
    }
}

/// What one client is watching.
struct Subscription {
    events: broadcast::Receiver<SeqEvent>,
    channels: HashSet<EventKind>,
}

/// The channels a `Subscribe` asked for, if that is what this message is.
fn subscribe_request(message: &IpcMessage) -> Option<Vec<EventKind>> {
    match &message.payload {
        IpcPayload::Request(Request::Subscribe { channels })
            if message.version == LAZYDAP_PROTOCOL_VERSION =>
        {
            Some(channels.clone())
        }
        _ => None,
    }
}

/// Start pushing events, and say what the daemon looks like right now.
///
/// The answer is a [`Response::Status`] rather than a variant of its own, and
/// that is the whole design (D038): the snapshot and the subscription are
/// taken under the same call, so there is no window between "what is the state
/// now?" and "tell me when it changes" for an event to fall into. A client
/// that asked those as two questions would have to reconcile the answers.
///
/// It follows that nothing buffered is replayed. A subscriber is told where
/// things stand and then what happens next; re-sending history would report a
/// `Stopped` the snapshot has already accounted for, and would send the TUI
/// chasing a position the program left long ago. Debuggee output produced
/// before the subscription is still readable — `Request::Output` reads the
/// buffer without draining it — and, unlike this stream, that is a request the
/// CLI makes too.
fn subscribe(
    state: &Arc<DaemonState>,
    subscription: &mut Option<Subscription>,
    channels: Vec<EventKind>,
    id: u64,
) -> IpcMessage {
    // Subscribing again replaces the previous set rather than adding to it, so
    // a client can narrow what it is watching without reconnecting.
    let channels: HashSet<EventKind> = channels.into_iter().collect();
    tracing::debug!(
        target: "daemon.ipc",
        channels = channels.len(),
        "a client subscribed to events",
    );
    *subscription = Some(Subscription {
        events: state.events().subscribe(),
        channels,
    });

    IpcMessage::response(id, Response::Status(state.status()))
}

/// The next event this client asked to see.
///
/// Never resolves when there is no subscription, so the arm simply never wins
/// — which is what keeps `select!` from spinning on a client that has not
/// subscribed.
async fn next_event(subscription: Option<&mut Subscription>) -> Option<Event> {
    let Some(subscription) = subscription else {
        return std::future::pending().await;
    };

    loop {
        match subscription.events.recv().await {
            Ok(sequenced) if subscription.channels.contains(&sequenced.event.kind()) => {
                return Some(sequenced.event);
            }
            Ok(_) => continue,
            // The client reads more slowly than the session produces. Dropping
            // the oldest is the design (`EVENT_CHANNEL_CAPACITY`); what a
            // client does about it is resynchronise, which for the TUI is the
            // stack fetch that follows the next stop.
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                tracing::warn!(target: "daemon.ipc", missed, "a subscriber fell behind");
            }
            Err(broadcast::error::RecvError::Closed) => return None,
        }
    }
}

async fn handle_message(state: &Arc<DaemonState>, message: IpcMessage) -> IpcMessage {
    let id = message.id;

    let request = match message.payload {
        IpcPayload::Request(request) => request,
        other => {
            return IpcMessage::error(
                id,
                IpcError::new(
                    ErrorCode::BadRequest,
                    format!("expected a request, got {other:?}"),
                ),
            );
        }
    };

    // Two requests are answered whatever version they arrive on. `Ping` is how
    // a client *discovers* the mismatch, and `Shutdown` is how it resolves
    // one: an upgraded client stops the old daemon and starts its own. Reject
    // `Shutdown` for being from the wrong version and the upgrade path
    // deadlocks — the client cannot stop the daemon, and the daemon will not
    // talk to the client. Anything else is refused.
    let version_exempt = matches!(request, Request::Ping | Request::Shutdown);
    if message.version != LAZYDAP_PROTOCOL_VERSION && !version_exempt {
        return IpcMessage::error(
            id,
            IpcError::new(
                ErrorCode::VersionMismatch,
                format!(
                    "this daemon speaks protocol v{LAZYDAP_PROTOCOL_VERSION}, \
                     the client speaks v{}",
                    message.version
                ),
            )
            .with_details(serde_json::json!({
                "daemon_version": LAZYDAP_PROTOCOL_VERSION,
                "client_version": message.version,
            })),
        );
    }

    match handlers::dispatch(state, request).await {
        Ok(response) => IpcMessage::response(id, response),
        Err(error) => {
            tracing::warn!(target: "daemon.ipc", request_id = id, %error, "request failed");
            IpcMessage::error(id, error)
        }
    }
}

/// End every session, so no adapter outlives the daemon that owns it.
async fn shut_down_sessions(state: &Arc<DaemonState>) {
    for session in state.drain_sessions() {
        tracing::info!(target: "daemon.session", session_id = %session.id, "ending session on shutdown");
        if session.state().is_live() {
            let _ = session.adapter().disconnect(true).await;
        }
        session.adapter().kill().await;
        session.end_once(EndReason::Disconnected);
    }
}

async fn drain(connections: &mut JoinSet<()>) {
    if tokio::time::timeout(CONNECTION_DRAIN_TIMEOUT, async {
        while connections.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tracing::warn!(target: "daemon.ipc", "client connections did not finish in time");
        connections.abort_all();
    }
}

/// SIGTERM and Ctrl-C both mean "stop", and both should let the teardown run.
fn spawn_signal_watch(state: Arc<DaemonState>) {
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(signal) => signal,
            Err(error) => {
                tracing::error!(target: "daemon.ipc", %error, "cannot listen for SIGTERM");
                return;
            }
        };
        let mut interrupt = match signal(SignalKind::interrupt()) {
            Ok(signal) => signal,
            Err(error) => {
                tracing::error!(target: "daemon.ipc", %error, "cannot listen for SIGINT");
                return;
            }
        };

        let name = tokio::select! {
            _ = terminate.recv() => "SIGTERM",
            _ = interrupt.recv() => "SIGINT",
        };
        tracing::info!(target: "daemon.ipc", signal = name, "stopping");
        state.request_shutdown();
    });
}

/// Whether something is listening and accepting on `socket`.
async fn is_reachable(socket: &Path) -> bool {
    socket.exists() && UnixStream::connect(socket).await.is_ok()
}

fn write_pid_file(path: &Path) -> Result<()> {
    fs::write(path, std::process::id().to_string()).map_err(|source| {
        CliError::general(anyhow::anyhow!(
            "cannot write the pid file at {}: {source}",
            path.display()
        ))
    })
}

/// Only remove a pid file that still names us. A successor daemon may have
/// started and overwritten it, and deleting that would hide it from the next
/// client that goes looking.
fn clear_pid_file_if_ours(path: &Path) {
    let ours = fs::read_to_string(path)
        .ok()
        .and_then(|contents| contents.trim().parse::<u32>().ok())
        .is_some_and(|pid| pid == std::process::id());
    if ours {
        let _ = fs::remove_file(path);
    }
}
