//! The TUI's connection to the daemon.
//!
//! The same socket, the same framing and the same requests the CLI uses — the
//! TUI has no private channel to the daemon and no request of its own
//! (non-negotiable 2). What is different is the shape of the conversation: the
//! CLI asks one question and waits, while the TUI subscribes and is talked at.
//! So the stream is split in two, a task per direction, and everything either
//! task learns arrives at the reducer as a [`Msg`].
//!
//! Requests carry an id chosen by the reducer, not by this module (D040).
//! That is what lets an answer be matched to the thing that asked for it — a
//! `Variables` reply is a bare list of variables with nothing in it saying
//! which node was being expanded, and a stack trace for a stop the program has
//! already left is indistinguishable from the current one.

use crate::error::{Result, TuiError};
use crate::msg::Msg;
use lazydap_protocol::{
    IpcConnection, IpcMessage, IpcPayload, LAZYDAP_PROTOCOL_VERSION, Request, Response,
};
use std::path::Path;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

/// The id the handshake uses.
///
/// Reserved: [`crate::state::RESERVED_IDS`] is what keeps the reducer from
/// handing out the same one and mistaking a `Pong` for its own answer.
const HANDSHAKE_ID: u64 = 1;

/// A live connection to the daemon.
///
/// Sending is infallible from the caller's side: a daemon that has gone away
/// is reported as [`Msg::DaemonGone`] by the read pump, which is the one place
/// that can tell the difference between "not yet" and "never".
pub struct IpcClient {
    requests: UnboundedSender<(u64, Request)>,
}

impl IpcClient {
    pub fn send(&self, id: u64, request: Request) {
        if self.requests.send((id, request)).is_err() {
            tracing::debug!(target: "tui.ipc", id, "dropped a request; the connection has closed");
        }
    }
}

/// Connect, check versions, and start pumping in both directions.
///
/// The daemon must already be running: starting one means spawning a process,
/// and this crate cannot reach the code that does that — by design, since that
/// is the same boundary that stops the TUI from reaching the daemon's
/// internals. `lazydap`'s own entry point ensures a daemon first and hands the
/// socket over.
pub async fn connect(socket: &Path, msgs: UnboundedSender<Msg>) -> Result<IpcClient> {
    let stream = UnixStream::connect(socket)
        .await
        .map_err(|source| TuiError::Connect {
            socket: socket.display().to_string(),
            source,
        })?;

    // Split before the handshake, so both halves are already the long-lived
    // ones the pumps will own.
    let (read, write) = stream.into_split();
    let mut reader = IpcConnection::new(read);
    let mut writer = IpcConnection::new(write);
    handshake(&mut reader, &mut writer).await?;

    let (requests, outgoing) = mpsc::unbounded_channel();
    tokio::spawn(write_pump(writer, outgoing));
    tokio::spawn(read_pump(reader, msgs));

    Ok(IpcClient { requests })
}

/// Say hello, and refuse to go on with a daemon from another build.
///
/// The same first move the CLI makes, and for the same reason: every request
/// after this one assumes both ends agree on what the words mean.
async fn handshake(
    reader: &mut IpcConnection<OwnedReadHalf>,
    writer: &mut IpcConnection<OwnedWriteHalf>,
) -> Result<()> {
    writer
        .send(IpcMessage::request(HANDSHAKE_ID, Request::Ping))
        .await
        .map_err(TuiError::Ipc)?;

    let reply = reader
        .recv()
        .await
        .map_err(TuiError::Ipc)?
        .ok_or(TuiError::DaemonGone)?;
    match reply.payload {
        IpcPayload::Response(Response::Pong { version, .. })
            if version == LAZYDAP_PROTOCOL_VERSION =>
        {
            Ok(())
        }
        IpcPayload::Response(Response::Pong { version, .. }) => Err(TuiError::VersionMismatch {
            daemon: version,
            ours: LAZYDAP_PROTOCOL_VERSION,
        }),
        IpcPayload::Error(error) => Err(TuiError::Protocol(error)),
        other => Err(TuiError::UnexpectedFrame(format!("{other:?}"))),
    }
}

/// Requests out, one at a time, in the order the reducer asked for them.
async fn write_pump(
    mut writer: IpcConnection<OwnedWriteHalf>,
    mut outgoing: UnboundedReceiver<(u64, Request)>,
) {
    while let Some((id, request)) = outgoing.recv().await {
        tracing::debug!(target: "tui.ipc", id, ?request, "sending");

        if let Err(error) = writer.send(IpcMessage::request(id, request)).await {
            tracing::warn!(target: "tui.ipc", %error, "could not reach the daemon");
            return;
        }
    }
}

/// Everything the daemon says, turned into messages.
async fn read_pump(mut reader: IpcConnection<OwnedReadHalf>, msgs: UnboundedSender<Msg>) {
    loop {
        let received = match reader.recv().await {
            Ok(Some(message)) => message,
            // A clean hang-up and a broken one are the same news to the user:
            // the daemon is not there any more.
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(target: "tui.ipc", %error, "the connection failed");
                break;
            }
        };

        if let Some(msg) = classify(received)
            && msgs.send(msg).is_err()
        {
            // The TUI has quit. Nothing left to deliver to.
            return;
        }
    }

    let _ = msgs.send(Msg::DaemonGone);
}

/// One frame from the daemon as one message, or nothing for a frame that makes
/// no sense coming from that direction.
fn classify(message: IpcMessage) -> Option<Msg> {
    let id = message.id;
    match message.payload {
        IpcPayload::Event(event) => Some(Msg::DaemonEvent(event)),
        IpcPayload::Response(response) => Some(Msg::DaemonResponse {
            id,
            response: Box::new(response),
        }),
        IpcPayload::Error(error) => Some(Msg::DaemonFailed { id, error }),
        // Daemons answer; they do not ask.
        IpcPayload::Request(request) => {
            tracing::warn!(target: "tui.ipc", ?request, "the daemon sent a request");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazydap_core::{AdapterKind, SessionId};
    use lazydap_protocol::{ErrorCode, Event, IpcError};

    #[test]
    fn an_event_reaches_the_reducer_as_an_event() {
        let session_id = SessionId::new();
        let msg = classify(IpcMessage::event(Event::SessionStarted {
            session_id,
            adapter: AdapterKind::Codelldb,
        }));

        match msg {
            Some(Msg::DaemonEvent(Event::SessionStarted { session_id: id, .. })) => {
                assert_eq!(id, session_id);
            }
            other => unreachable!("expected an event, got: {other:?}"),
        }
    }

    #[test]
    fn a_failure_keeps_the_id_that_asked_for_it() {
        let msg = classify(IpcMessage::error(
            7,
            IpcError::new(ErrorCode::SessionNotFound, "no session"),
        ));

        match msg {
            Some(Msg::DaemonFailed { id, error }) => {
                assert_eq!(id, 7);
                assert_eq!(error.code, ErrorCode::SessionNotFound);
            }
            other => unreachable!("expected a failure, got: {other:?}"),
        }
    }

    #[test]
    fn a_request_arriving_from_the_daemon_is_dropped_rather_than_acted_on() {
        assert!(classify(IpcMessage::request(1, Request::Shutdown)).is_none());
    }

    /// The connection over a real Unix socket, with the shipped codec at both
    /// ends (D014). The daemon on the other side is a stand-in — this crate
    /// cannot depend on the real one — but nothing between here and the wire
    /// is faked, which is where the mistakes are.
    mod over_a_real_socket {
        use super::*;
        use lazydap_core::PauseReason;
        use lazydap_protocol::{Event, IpcConnection, StatusReport};
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU32, Ordering};
        use tokio::net::UnixListener;

        /// A socket path nothing else in this test run will pick.
        fn socket_path() -> PathBuf {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            std::env::temp_dir().join(format!(
                "lazydap-tui-ipc-{}-{}.sock",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst),
            ))
        }

        /// Something that answers a handshake and then says `script`.
        ///
        /// `pong_version` is what it claims to speak, so the mismatch path is
        /// reachable without another build of lazydap.
        fn spawn_daemon(pong_version: u32, script: Vec<IpcMessage>) -> PathBuf {
            let socket = socket_path();
            let listener = UnixListener::bind(&socket).expect("bind");

            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut connection = IpcConnection::new(stream);

                let hello = connection.recv().await.expect("recv").expect("a ping");
                connection
                    .send(IpcMessage::response(
                        hello.id,
                        Response::Pong {
                            version: pong_version,
                            instance: "lazydap-test".to_string(),
                            uptime_ms: 0,
                        },
                    ))
                    .await
                    .expect("send the pong");

                for message in script {
                    connection.send(message).await.expect("send");
                }
                // Held open until the test drops the listener's task, so the
                // client sees a connection that is alive rather than one that
                // closed the moment it was made.
                std::future::pending::<()>().await;
            });

            socket
        }

        fn status() -> IpcMessage {
            IpcMessage::response(
                2,
                Response::Status(StatusReport {
                    instance: "lazydap-test".to_string(),
                    daemon_pid: 1,
                    uptime_ms: 0,
                    protocol_version: LAZYDAP_PROTOCOL_VERSION,
                    lazydap_version: "0.1.0".to_string(),
                    session: None,
                }),
            )
        }

        fn stopped(session_id: SessionId) -> IpcMessage {
            IpcMessage::event(Event::Stopped {
                session_id,
                thread_id: Some(1),
                adapter_thread_id: None,
                reason: PauseReason::Breakpoint,
                raw_reason: None,
                all_threads_stopped: true,
                hit_breakpoint_ids: Vec::new(),
            })
        }

        #[tokio::test]
        async fn a_subscription_snapshot_and_the_events_after_it_both_arrive_as_messages() {
            let session_id = SessionId::new();
            let socket = spawn_daemon(
                LAZYDAP_PROTOCOL_VERSION,
                vec![status(), stopped(session_id)],
            );
            let (tx, mut rx) = mpsc::unbounded_channel();

            let client = connect(&socket, tx).await.expect("connect");
            client.send(
                2,
                Request::Subscribe {
                    channels: vec![lazydap_protocol::EventKind::Stopped],
                },
            );

            match rx.recv().await.expect("a snapshot") {
                Msg::DaemonResponse { response, .. } => {
                    assert!(matches!(*response, Response::Status(_)));
                }
                other => unreachable!("expected the snapshot first, got: {other:?}"),
            }
            match rx.recv().await.expect("an event") {
                Msg::DaemonEvent(Event::Stopped { session_id: id, .. }) => {
                    assert_eq!(id, session_id);
                }
                other => unreachable!("expected a stopped event, got: {other:?}"),
            }
        }

        #[tokio::test]
        async fn a_daemon_from_another_build_is_refused_before_anything_is_asked_of_it() {
            // The alternative is a TUI that starts, draws, and then fails on
            // every request for reasons it cannot explain.
            let socket = spawn_daemon(LAZYDAP_PROTOCOL_VERSION + 1, Vec::new());
            let (tx, _rx) = mpsc::unbounded_channel();

            match connect(&socket, tx).await.err() {
                Some(TuiError::VersionMismatch { daemon, ours }) => {
                    assert_eq!(daemon, LAZYDAP_PROTOCOL_VERSION + 1);
                    assert_eq!(ours, LAZYDAP_PROTOCOL_VERSION);
                }
                other => unreachable!("expected a version mismatch, got: {other:?}"),
            }
        }

        #[tokio::test]
        async fn a_socket_with_nothing_behind_it_is_reported_as_such() {
            let (tx, _rx) = mpsc::unbounded_channel();
            let missing = std::env::temp_dir().join("lazydap-tui-there-is-nothing-here.sock");

            match connect(&missing, tx).await.err() {
                Some(TuiError::Connect { socket, .. }) => {
                    assert!(socket.contains("nothing-here"), "got: {socket}");
                }
                other => unreachable!("expected a connect failure, got: {other:?}"),
            }
        }

        #[tokio::test]
        async fn a_daemon_that_hangs_up_becomes_one_message_and_then_silence() {
            let socket = socket_path();
            let listener = UnixListener::bind(&socket).expect("bind");
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.expect("accept");
                let mut connection = IpcConnection::new(stream);
                let hello = connection.recv().await.expect("recv").expect("a ping");
                connection
                    .send(IpcMessage::response(
                        hello.id,
                        Response::Pong {
                            version: LAZYDAP_PROTOCOL_VERSION,
                            instance: "lazydap-test".to_string(),
                            uptime_ms: 0,
                        },
                    ))
                    .await
                    .expect("send the pong");
                // And now go away, as a daemon that has been shut down does.
            });

            let (tx, mut rx) = mpsc::unbounded_channel();
            let _client = connect(&socket, tx).await.expect("connect");

            assert!(matches!(rx.recv().await, Some(Msg::DaemonGone)));
            assert!(
                rx.recv().await.is_none(),
                "the pump should have stopped, not gone on reporting",
            );
        }
    }
}
