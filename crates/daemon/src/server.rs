//! The daemon: one Unix socket, many clients, one debug session.

use crate::error::{CliError, Result};
use crate::handlers;
use crate::instance::Instance;
use crate::state::{DaemonState, SeqEvent};
use crate::wait::Abandoned;
use lazydap_core::EndReason;
use lazydap_protocol::{
    ErrorCode, Event, EventKind, IpcConnection, IpcError, IpcMessage, IpcPayload,
    LAZYDAP_PROTOCOL_VERSION, MAX_FRAME_BYTES, Request, Response, is_frame_error,
};
use lazydap_store::ProjectStore;
use std::collections::{HashSet, VecDeque};
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
    // Before the socket, deliberately. A hand-edited `state.toml` with a typo
    // in it is a normal way for this to fail, and a daemon that has already
    // bound and then dies leaves a socket nothing answers on — which costs
    // every later command the client's full spawn deadline before it gives up.
    // Nothing is on disk yet, so failing here costs nothing.
    let store = ProjectStore::load(&instance.project_root).map_err(|source| {
        CliError::general(anyhow::anyhow!(
            "cannot read {}: {source}",
            instance.project_root.display()
        ))
    })?;

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
    // From here on the socket and pid file exist, so every exit — including a
    // `?` on the way up — has to take them with it.
    let _files = BoundFiles {
        socket: &instance.socket,
        pid: &instance.pid,
    };
    // The socket is an unauthenticated control channel for a debugger. Nobody
    // else on the machine gets to open it.
    fs::set_permissions(&instance.socket, fs::Permissions::from_mode(0o600))
        .map_err(CliError::general)?;
    write_pid_file(&instance.pid)?;

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
    tracing::info!(target: "daemon.ipc", "daemon stopped");

    result
}

/// The socket and pid file, removed when the daemon stops for any reason.
///
/// A `Drop` rather than a line at the end of [`run_daemon`], because the
/// failures worth cleaning up after are the ones that leave through `?`.
struct BoundFiles<'a> {
    socket: &'a Path,
    pid: &'a Path,
}

impl Drop for BoundFiles<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.socket);
        clear_pid_file_if_ours(self.pid);
    }
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
/// Requests on a connection are handled one at a time and in the order they
/// arrived. A CLI client sends one and waits, so concurrency here would buy
/// nothing; separate clients already get separate tasks. The socket *is* read
/// while a request is in flight — that is how a client hanging up on a
/// `--wait` gets noticed (see [`answer`]) — so anything that turns out to be
/// another request waits its turn in `queued` rather than being dropped.
///
/// A client that has sent [`Request::Subscribe`] also gets events pushed at it
/// between replies, on the same connection. Sends happen in the *body* of a
/// `select!` arm and never as one: [`IpcConnection::send`] is not
/// cancellation-safe, and a cancelled send leaves half a frame on the wire.
pub async fn serve_client(stream: UnixStream, state: Arc<DaemonState>) {
    let mut connection = IpcConnection::new(stream);
    let mut shutdown = state.shutdown_receiver();
    let mut subscription: Option<Subscription> = None;
    // Requests read while an earlier one was still being answered. They are
    // still handled one at a time and in order — this is only where one waits
    // its turn, so that watching for a hang-up during a `--wait` cannot
    // swallow a frame that turned out to be a request after all.
    let mut queued: VecDeque<IpcMessage> = VecDeque::new();

    loop {
        if state.shutdown_requested() {
            break;
        }

        let incoming = if let Some(message) = queued.pop_front() {
            Ok(Some(message))
        } else {
            tokio::select! {
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
            }
        };

        let message = match incoming {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(error) => {
                let _ = connection.send(unreadable_frame(&error)).await;
                break;
            }
        };

        // Subscribing is the one request that changes the connection rather
        // than the daemon, so it is answered here instead of in `dispatch`.
        let (reply, unreadable) = match subscribe_request(&message) {
            Some(channels) => (
                subscribe(&state, &mut subscription, channels, message.id),
                None,
            ),
            None => answer(&state, message, &mut connection, &mut queued).await,
        };
        let id = reply.id;
        if let Err(error) = connection.send(reply).await {
            if is_frame_error(&error) {
                // Nothing reached the wire — the frame was never built — so
                // the socket is fine and the request can still be answered.
                // Hanging up instead is what a client read as "the daemon
                // closed the connection before answering" and reported as an
                // unreachable daemon (D091).
                tracing::warn!(
                    target: "daemon.ipc",
                    request_id = id,
                    %error,
                    "a reply could not be framed",
                );
                if connection.send(too_large(id)).await.is_err() {
                    break;
                }
                continue;
            }
            tracing::debug!(target: "daemon.ipc", %error, "client went away mid-reply");
            break;
        }

        // A frame read while the reply above was still being produced, that
        // this build cannot decode. The reply it interrupted was owed first;
        // this is the same refusal the idle path sends, in the same place in
        // the conversation (D092).
        if let Some(error) = unreadable {
            let _ = connection.send(unreadable_frame(&error)).await;
            break;
        }
    }
}

/// The refusal for a frame the daemon could not read, and the end of the
/// conversation.
///
/// Answered on id `0`, because a frame nobody can decode carries no id to
/// answer on, and followed by a hang-up: a client left waiting for a reply
/// that can never correlate is worse than a closed socket.
fn unreadable_frame(error: &std::io::Error) -> IpcMessage {
    tracing::warn!(target: "daemon.ipc", %error, "unreadable frame from a client");
    IpcMessage::error(0, IpcError::new(ErrorCode::BadRequest, error.to_string()))
}

/// Answer one request, giving up on it if the client that asked goes away.
///
/// `--wait` is the reason this is not a plain `await`. A `continue --wait
/// --timeout 0` against a program that never stops blocks for as long as its
/// caller lets it, and it holds the session's execution permit for all of it —
/// so a Ctrl-C left every later `continue` and `step`, from every client,
/// queued behind a request nobody was waiting for.
///
/// What is raced is the *read*, never the handler's own work. `recv` is
/// cancellation-safe, so losing that branch costs no bytes, and the hang-up it
/// detects only reaches the wait loop (see [`handlers::dispatch`]). A DAP
/// request half-written to an adapter, or a mutation half-applied, is never
/// what gets abandoned: those run to completion and are answered into a socket
/// that is no longer read, which is what the reply's send failure then ends
/// the connection on (D092).
///
/// Only three things can come back from that read, and the difference between
/// them is the whole of this function:
///
/// - **Another request.** It waits its turn in `queued`. Dropping it would
///   hang a client on a reply that never comes.
/// - **A frame this build cannot decode** — malformed, or a `Request` variant
///   from a newer client. The peer is still there and the reply in flight is
///   still owed, so this is *not* a hang-up: it is handed back for the caller
///   to answer on id `0` after that reply. Reading it as one cut a `--wait`
///   short with a fabricated `timeout` and swallowed the bad frame in silence.
/// - **Anything else** — a clean end of stream, a vanished peer, a real I/O
///   failure. That is the hang-up, and the only thing that ends the wait.
///
/// The second of the pair is a decode failure to answer after the reply.
async fn answer(
    state: &Arc<DaemonState>,
    message: IpcMessage,
    connection: &mut IpcConnection<UnixStream>,
    queued: &mut VecDeque<IpcMessage>,
) -> (IpcMessage, Option<std::io::Error>) {
    let (hangup, abandoned) = tokio::sync::oneshot::channel();
    let mut handled = std::pin::pin!(handle_message(state, message, abandoned));

    loop {
        // One frame ahead and no further. Reading is what notices a hang-up,
        // and one frame is all that takes; draining the socket for as long as
        // a `--wait` runs would take the kernel's own backpressure off a
        // client that never stops sending and let `queued` grow unbounded.
        //
        // The cost is that a client which pipelines *behind* a `--wait` is not
        // noticed hanging up until that wait ends on its own. No lazydap
        // client does — a `--wait` is the last thing its caller says — and an
        // unbounded read is a worse thing to be wrong about.
        if !queued.is_empty() {
            return (handled.await, None);
        }

        tokio::select! {
            biased;
            reply = &mut handled => return (reply, None),
            read = connection.recv() => match read {
                Ok(Some(message)) => queued.push_back(message),
                Err(error) if is_frame_error(&error) => return (handled.await, Some(error)),
                _ => {
                    drop(hangup);
                    return (handled.await, None);
                }
            },
        }
    }
}

/// The refusal for an answer too big to put on the socket.
///
/// `BadRequest` because it is the caller's knob that decides: the request that
/// reaches this is one asking for everything of something unbounded, and
/// narrowing it is both possible and the documented way to read a large
/// container.
fn too_large(id: u64) -> IpcMessage {
    IpcMessage::error(
        id,
        IpcError::new(
            ErrorCode::BadRequest,
            format!(
                "the answer is larger than the {} MiB a single frame carries; \
                 ask for less of it (`--max`, `--start`/`--count`, `--since`)",
                MAX_FRAME_BYTES / (1024 * 1024),
            ),
        )
        .with_details(serde_json::json!({ "max_frame_bytes": MAX_FRAME_BYTES })),
    )
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

async fn handle_message(
    state: &Arc<DaemonState>,
    message: IpcMessage,
    abandoned: Abandoned,
) -> IpcMessage {
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

    match handlers::dispatch(state, request, Some(abandoned)).await {
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
        // A session that had already exited never sent the `disconnect` above,
        // so delve never ran its own cleanup and its compiled binary is still
        // on disk (finding 4). The live case is belt-and-braces — delve deleted
        // it when it handled the disconnect — but this path is the one that
        // actually leaked: exit cleanly, then `shutdown`.
        session.clean_compiled_artifact();
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
