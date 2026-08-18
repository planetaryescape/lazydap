//! The daemon's IPC surface, over a real Unix socket with real framing.
//!
//! Nothing here is mocked: a genuine `UnixListener`, the shipped codec, and
//! the same `serve_client` the accept loop runs (D014). The one thing these
//! tests do not do is launch a debuggee — that needs a live adapter and is
//! covered by the milestone's manual verification.

use lazydap_core::{
    AdapterKind, BreakpointId, BreakpointSelector, NewBreakpoint, OutputCategory, OutputChunk,
    PauseReason, SessionId,
};
use lazydap_daemon::client::DaemonClient;
use lazydap_daemon::server::serve_client;
use lazydap_daemon::state::{DaemonState, SeqEvent};
use lazydap_protocol::{
    ErrorCode, Event, EventKind, IpcConnection, IpcMessage, IpcPayload, LAZYDAP_PROTOCOL_VERSION,
    LaunchRequest, Request, Response,
};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::net::{UnixListener, UnixStream};

/// A daemon serving on its own socket, cleaned up when the test ends.
struct TestDaemon {
    dir: PathBuf,
    socket: PathBuf,
    state: Arc<DaemonState>,
}

impl TestDaemon {
    async fn start() -> Self {
        Self::named("lazydap-test".to_string()).await
    }

    async fn named(instance: String) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lazydap-ipc-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst),
        ));
        std::fs::create_dir_all(&dir).expect("create the test directory");

        let socket = dir.join("lazydap.sock");
        let listener = UnixListener::bind(&socket).expect("bind the test socket");
        // The test directory doubles as the project root, so nothing here
        // writes a `.lazydap/` into the repository the tests run from.
        let store = lazydap_store::ProjectStore::load(&dir).expect("load the store");
        let state = DaemonState::new(instance, store);

        let accept_state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(serve_client(stream, Arc::clone(&accept_state)));
            }
        });

        Self { dir, socket, state }
    }

    async fn client(&self) -> DaemonClient {
        DaemonClient::connect(&self.socket)
            .await
            .expect("connect and handshake")
    }

    /// A connection with no handshake, for saying things a client never would.
    async fn raw(&self) -> IpcConnection<UnixStream> {
        IpcConnection::new(UnixStream::connect(&self.socket).await.expect("connect"))
    }

    /// A request that takes a few hundred milliseconds and needs no session.
    ///
    /// Everything else the daemon answers resolves on its first poll, which
    /// makes it useless for testing what happens to a *second* frame while a
    /// first is still in flight — the read-ahead branch never runs at all.
    /// This launches with the adapter binary pointed at a script that sleeps
    /// and then exits without announcing a port, so the handshake fails
    /// honestly after a known interval. The script stands in for an external
    /// process, never for anything lazydap owns.
    fn slow_request(&self) -> Request {
        let script = self.dir.join("dawdling-adapter.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 0.3\n").expect("write the fake adapter");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("make the fake adapter executable");

        Request::Launch(LaunchRequest {
            adapter: AdapterKind::Codelldb,
            program: self.dir.join("no-such-program"),
            args: Vec::new(),
            cwd: self.dir.clone(),
            env: BTreeMap::new(),
            stop_on_entry: false,
            adapter_command: Some(script),
        })
    }

    /// A connection with every frame already on the wire, so what the daemon
    /// does with the second while the first is in flight is a fact rather than
    /// a race.
    async fn with_frames_waiting(&self, frames: &[Vec<u8>]) -> IpcConnection<UnixStream> {
        use tokio::io::AsyncWriteExt;

        let mut stream = UnixStream::connect(&self.socket).await.expect("connect");
        for frame in frames {
            stream.write_all(frame).await.expect("write a frame");
        }
        stream.flush().await.expect("flush");
        IpcConnection::new(stream)
    }

    /// A connection that has already asked for these event kinds.
    ///
    /// Its `Subscribe` reply is still unread — every test reads it, because
    /// what that reply *is* is part of the contract.
    async fn subscriber(&self, channels: &[EventKind]) -> Subscriber {
        let mut subscriber = Subscriber {
            connection: self.raw().await,
        };
        subscriber
            .send(
                1,
                Request::Subscribe {
                    channels: channels.to_vec(),
                },
            )
            .await;
        subscriber
    }

    /// Put an event on the daemon's broadcast, as a live session would.
    ///
    /// The sequence number is a session's business; nothing on the subscriber
    /// side reads it, which is why one can be made up here.
    ///
    /// A send with nobody listening is ignored, exactly as `Session::emit`
    /// ignores it — that is the normal state of a daemon between CLI calls.
    fn emit(&self, event: Event) {
        let _ = self.state.events().send(SeqEvent { seq: 1, event });
    }
}

/// A raw connection used as a long-lived client.
struct Subscriber {
    connection: IpcConnection<UnixStream>,
}

impl Subscriber {
    async fn send(&mut self, id: u64, request: Request) {
        self.connection
            .send(IpcMessage::request(id, request))
            .await
            .expect("send");
    }

    async fn next(&mut self) -> IpcMessage {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.connection.recv())
            .await
            .expect("the daemon should answer within five seconds")
            .expect("recv")
            .expect("a frame")
    }

    async fn reply(&mut self) -> IpcPayload {
        self.next().await.payload
    }
}

/// One length-delimited frame, built the way the codec builds them.
fn frame(message: &IpcMessage) -> Vec<u8> {
    framed(&serde_json::to_vec(message).expect("serialise"))
}

/// A frame with an honest length prefix and a body that is not a message.
fn unreadable_frame() -> Vec<u8> {
    framed(b"{ not a message at all }")
}

fn framed(body: &[u8]) -> Vec<u8> {
    let mut bytes = (body.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(body);
    bytes
}

/// The next frame, or a failure that says the daemon went quiet.
async fn next_frame(connection: &mut IpcConnection<UnixStream>) -> IpcMessage {
    tokio::time::timeout(std::time::Duration::from_secs(10), connection.recv())
        .await
        .expect("the daemon should answer every frame it read")
        .expect("recv")
        .expect("a frame, not a closed connection")
}

fn stopped(session_id: SessionId) -> Event {
    Event::Stopped {
        session_id,
        thread_id: Some(1),
        adapter_thread_id: None,
        reason: PauseReason::Breakpoint,
        raw_reason: None,
        all_threads_stopped: true,
        hit_breakpoint_ids: Vec::new(),
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[tokio::test]
async fn a_client_learns_the_daemon_s_protocol_version_before_anything_else() {
    let daemon = TestDaemon::start().await;
    let client = daemon.client().await;

    assert_eq!(client.daemon_version, LAZYDAP_PROTOCOL_VERSION);
    assert_eq!(client.instance, "lazydap-test");
}

#[tokio::test]
async fn status_reports_a_daemon_with_no_session() {
    let daemon = TestDaemon::start().await;
    let mut client = daemon.client().await;

    let response = client.request(Request::Status).await.expect("status");
    match response {
        Response::Status(report) => {
            assert_eq!(report.instance, "lazydap-test");
            assert_eq!(report.protocol_version, LAZYDAP_PROTOCOL_VERSION);
            assert!(report.session.is_none(), "nothing has been launched");
        }
        other => unreachable!("expected a status report, got: {other:?}"),
    }
}

#[tokio::test]
async fn several_requests_share_one_connection_and_keep_their_ids_straight() {
    let daemon = TestDaemon::start().await;
    let mut client = daemon.client().await;

    for _ in 0..3 {
        assert!(matches!(
            client.request(Request::Status).await.expect("status"),
            Response::Status(_)
        ));
    }
}

#[tokio::test]
async fn subscribing_is_answered_with_the_state_the_subscription_starts_from() {
    // The snapshot and the stream are taken together on purpose (D038): a
    // client that asked "what is the state?" and "tell me when it changes" as
    // two questions would have a window between them to lose an event in.
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::Stopped]).await;

    match subscriber.reply().await {
        IpcPayload::Response(Response::Status(report)) => {
            assert_eq!(report.instance, "lazydap-test");
            assert!(report.session.is_none(), "nothing has been launched");
        }
        other => unreachable!("expected a status snapshot, got: {other:?}"),
    }
}

#[tokio::test]
async fn a_subscriber_is_pushed_events_as_they_happen() {
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::Stopped]).await;
    subscriber.reply().await;

    let session_id = SessionId::new();
    daemon.emit(stopped(session_id));

    match subscriber.reply().await {
        IpcPayload::Event(Event::Stopped { session_id: id, .. }) => assert_eq!(id, session_id),
        other => unreachable!("expected a stopped event, got: {other:?}"),
    }
}

#[tokio::test]
async fn an_event_the_client_did_not_ask_for_is_not_pushed_at_it() {
    // "Subscribe to a small set of events" is only worth saying if the daemon
    // honours it — otherwise every client pays for the chattiest one.
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::Stopped]).await;
    subscriber.reply().await;

    let session_id = SessionId::new();
    daemon.emit(Event::Output {
        session_id,
        chunk: OutputChunk::new(OutputCategory::Stdout, "not subscribed to this\n"),
    });
    daemon.emit(stopped(session_id));

    match subscriber.reply().await {
        IpcPayload::Event(Event::Stopped { .. }) => {}
        other => unreachable!("the output event should have been filtered, got: {other:?}"),
    }
}

#[tokio::test]
async fn a_subscriber_can_still_ask_questions_and_get_its_own_answers_back() {
    // Events share the connection with replies. A client that could not tell
    // them apart would have to open a second one, and the whole point of the
    // id on an envelope is that it does not.
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::Stopped]).await;
    subscriber.reply().await;

    daemon.emit(stopped(SessionId::new()));
    subscriber.send(2, Request::Status).await;

    let mut saw_event = false;
    let mut saw_status = false;
    for _ in 0..2 {
        let message = subscriber.next().await;
        match message.payload {
            IpcPayload::Event(_) => {
                assert_eq!(message.id, 0, "events belong to nobody's request");
                saw_event = true;
            }
            IpcPayload::Response(Response::Status(_)) => {
                assert_eq!(message.id, 2, "a reply carries the id that asked");
                saw_status = true;
            }
            other => unreachable!("unexpected frame: {other:?}"),
        }
    }
    assert!(saw_event && saw_status, "both should arrive");
}

#[tokio::test]
async fn subscribing_again_replaces_the_kinds_rather_than_adding_to_them() {
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::Output]).await;
    subscriber.reply().await;

    subscriber
        .send(
            9,
            Request::Subscribe {
                channels: vec![EventKind::Stopped],
            },
        )
        .await;
    subscriber.reply().await;

    let session_id = SessionId::new();
    daemon.emit(Event::Output {
        session_id,
        chunk: OutputChunk::new(OutputCategory::Stdout, "no longer watched\n"),
    });
    daemon.emit(stopped(session_id));

    match subscriber.reply().await {
        IpcPayload::Event(Event::Stopped { .. }) => {}
        other => unreachable!("the first subscription should be gone, got: {other:?}"),
    }
}

#[tokio::test]
async fn a_client_that_never_subscribed_is_sent_no_events_at_all() {
    let daemon = TestDaemon::start().await;
    let mut client = daemon.client().await;

    daemon.emit(stopped(SessionId::new()));

    // If events leaked to every connection, this reply would be an event.
    assert!(matches!(
        client.request(Request::Status).await.expect("status"),
        Response::Status(_)
    ));
}

#[tokio::test]
async fn a_ping_is_answered_whatever_version_it_arrives_on() {
    let daemon = TestDaemon::start().await;
    let mut connection = daemon.raw().await;

    // How a client from another build discovers the mismatch at all.
    connection
        .send(IpcMessage {
            version: 9999,
            id: 1,
            payload: IpcPayload::Request(Request::Ping),
        })
        .await
        .expect("send");

    let reply = connection.recv().await.expect("recv").expect("a reply");
    match reply.payload {
        IpcPayload::Response(Response::Pong { version, .. }) => {
            assert_eq!(version, LAZYDAP_PROTOCOL_VERSION);
        }
        other => unreachable!("expected a pong, got: {other:?}"),
    }
}

#[tokio::test]
async fn a_request_from_another_build_is_refused_with_both_versions() {
    let daemon = TestDaemon::start().await;
    let mut connection = daemon.raw().await;

    connection
        .send(IpcMessage {
            version: 9999,
            id: 1,
            payload: IpcPayload::Request(Request::Status),
        })
        .await
        .expect("send");

    let reply = connection.recv().await.expect("recv").expect("a reply");
    match reply.payload {
        IpcPayload::Error(error) => {
            assert_eq!(error.code, ErrorCode::VersionMismatch);
            assert_eq!(error.details["client_version"], 9999);
            assert_eq!(error.details["daemon_version"], LAZYDAP_PROTOCOL_VERSION);
        }
        other => unreachable!("expected a version mismatch, got: {other:?}"),
    }
}

#[tokio::test]
async fn shutdown_crosses_protocol_versions_so_an_upgrade_can_land() {
    let daemon = TestDaemon::start().await;
    let mut connection = daemon.raw().await;

    // An upgraded client stops the old daemon before starting its own. If the
    // daemon refused this for being from the wrong version, the upgrade would
    // deadlock: the client cannot stop it, and it will not talk to the client.
    connection
        .send(IpcMessage {
            version: 9999,
            id: 1,
            payload: IpcPayload::Request(Request::Shutdown),
        })
        .await
        .expect("send");

    let reply = connection.recv().await.expect("recv").expect("a reply");
    match reply.payload {
        IpcPayload::Response(Response::ShuttingDown { .. }) => {}
        other => unreachable!("expected an acknowledgement, got: {other:?}"),
    }
    assert!(daemon.state.shutdown_requested());
}

#[tokio::test]
async fn a_frame_the_daemon_cannot_read_is_answered_before_it_hangs_up() {
    use tokio::io::AsyncWriteExt;

    let daemon = TestDaemon::start().await;
    let mut stream = UnixStream::connect(&daemon.socket).await.expect("connect");

    let body = b"{ not json at all }";
    stream
        .write_all(&(body.len() as u32).to_be_bytes())
        .await
        .expect("write the length prefix");
    stream.write_all(body).await.expect("write the body");
    stream.flush().await.expect("flush");

    let mut connection = IpcConnection::new(stream);
    let reply = connection.recv().await.expect("recv").expect("a reply");
    assert_eq!(reply.id, 0, "an unreadable frame has no id to answer on");
    match reply.payload {
        IpcPayload::Error(error) => assert_eq!(error.code, ErrorCode::BadRequest),
        other => unreachable!("expected a bad-request error, got: {other:?}"),
    }
}

/// A request read while another is still in flight is answered, after it.
///
/// The daemon reads ahead while a request runs, so it can notice a client
/// hanging up on a `--wait` and stop holding the session's execution permit
/// for it (D-WP3-5). Reading ahead must not cost anybody a request: the frame
/// it finds waits its turn rather than being dropped, which would hang the
/// client on a reply that never comes.
///
/// The first request has to be genuinely slow or this proves nothing. A
/// `Status` resolves on its first poll and the read-ahead branch never runs,
/// which is how an earlier version of this test passed just as happily against
/// a daemon that had no read-ahead in it at all.
#[tokio::test]
async fn a_request_read_while_another_is_in_flight_is_answered_after_it() {
    let daemon = TestDaemon::start().await;
    let mut connection = daemon
        .with_frames_waiting(&[
            frame(&IpcMessage::request(1, daemon.slow_request())),
            frame(&IpcMessage::request(2, Request::Status)),
        ])
        .await;

    let first = next_frame(&mut connection).await;
    assert_eq!(first.id, 1, "the slow request is answered first");
    assert!(
        matches!(first.payload, IpcPayload::Error(_)),
        "the fake adapter never announces a port, so the launch fails: {:?}",
        first.payload,
    );

    let second = next_frame(&mut connection).await;
    assert_eq!(second.id, 2, "and the frame read behind it is not lost");
    assert!(matches!(
        second.payload,
        IpcPayload::Response(Response::Status(_))
    ));
}

/// A frame the daemon cannot decode, arriving mid-request, is not a hang-up.
///
/// The read that watches for a client going away also sees malformed frames
/// and `Request` variants from newer builds. Treating one as a hang-up ended
/// the request in flight — a `--wait` came back `state: "timeout"` after a
/// second, on `--timeout 0` — and swallowed the bad frame without the
/// `BadRequest` on id 0 that an unreadable frame has always been answered
/// with. The peer is still there, and both answers are owed, in order.
#[tokio::test]
async fn an_unreadable_frame_mid_request_costs_neither_answer() {
    let daemon = TestDaemon::start().await;
    let mut connection = daemon
        .with_frames_waiting(&[
            frame(&IpcMessage::request(1, daemon.slow_request())),
            unreadable_frame(),
        ])
        .await;

    let first = next_frame(&mut connection).await;
    assert_eq!(first.id, 1, "the request in flight is still answered");
    assert!(
        matches!(first.payload, IpcPayload::Error(_)),
        "on its own terms — the fake adapter never announces a port: {:?}",
        first.payload,
    );

    let second = next_frame(&mut connection).await;
    assert_eq!(second.id, 0, "an unreadable frame has no id to answer on");
    match second.payload {
        IpcPayload::Error(error) => assert_eq!(error.code, ErrorCode::BadRequest, "got: {error}"),
        other => unreachable!("expected a bad-request error, got: {other:?}"),
    }

    assert!(
        connection.recv().await.expect("recv").is_none(),
        "and then it hangs up, as it always has for a frame it cannot read",
    );
}

/// A client that vanishes mid-request does not take the daemon with it.
#[tokio::test]
async fn a_client_that_hangs_up_leaves_the_daemon_serving_everybody_else() {
    let daemon = TestDaemon::start().await;

    let mut leaving = daemon.raw().await;
    leaving
        .send(IpcMessage::request(1, Request::Status))
        .await
        .expect("send");
    drop(leaving);

    let mut staying = daemon.client().await;
    assert!(matches!(
        staying.request(Request::Status).await.expect("status"),
        Response::Status(_)
    ));
}

/// A reply too large to frame is an answer, not a hang-up.
///
/// It never reaches the wire — the codec refuses to build the frame — so the
/// socket is fine and the request can still be refused in words. Breaking the
/// connection instead is what a client reported as "the daemon closed the
/// connection before answering", exit 3: an unreachable daemon, for a request
/// the daemon understood perfectly and simply could not fit (D-WP3-4).
///
/// The instance name is the cheapest reply that can be made too big; the
/// requests that reach this in practice are `variables --max 0` on a huge
/// container and `output` on a session that printed a lot.
#[tokio::test]
async fn a_reply_that_cannot_be_framed_is_refused_rather_than_hung_up_on() {
    let daemon = TestDaemon::named("x".repeat(lazydap_protocol::MAX_FRAME_BYTES + 1)).await;
    let mut connection = daemon.raw().await;

    connection
        .send(IpcMessage::request(4, Request::Status))
        .await
        .expect("send");

    let reply = tokio::time::timeout(std::time::Duration::from_secs(5), connection.recv())
        .await
        .expect("the daemon must answer rather than go quiet")
        .expect("recv")
        .expect("a frame, not a closed connection");

    assert_eq!(reply.id, 4, "answered in context, on the request's own id");
    match reply.payload {
        IpcPayload::Error(error) => {
            assert_eq!(error.code, ErrorCode::BadRequest, "got: {error}");
            assert!(error.message.contains("16 MiB"), "got: {}", error.message);
        }
        other => unreachable!("expected a refusal, got: {other:?}"),
    }

    // And the connection is still usable, which is the whole point of not
    // hanging up: a client can narrow its request and ask again.
    connection
        .send(IpcMessage::request(5, Request::Version))
        .await
        .expect("send");
    let reply = connection.recv().await.expect("recv").expect("a frame");
    assert_eq!(reply.id, 5);
}

#[tokio::test]
async fn shutdown_is_acknowledged_and_tells_the_daemon_to_stop() {
    let daemon = TestDaemon::start().await;
    let mut client = daemon.client().await;

    assert!(!daemon.state.shutdown_requested());
    let response = client.request(Request::Shutdown).await.expect("shutdown");

    assert!(matches!(response, Response::ShuttingDown { .. }));
    assert!(daemon.state.shutdown_requested());
}

#[tokio::test]
async fn connecting_to_a_socket_with_nothing_behind_it_fails_as_unreachable() {
    let missing = Path::new("/tmp/lazydap-there-is-nothing-here.sock");
    let error = match DaemonClient::connect(missing).await {
        Err(error) => error,
        Ok(_) => unreachable!("there is no daemon at {}", missing.display()),
    };

    assert_eq!(error.label, "DaemonUnreachable", "got: {error}");
    assert_eq!(error.exit_code, 3);
}

/// A breakpoint set with nothing running still reaches whoever is watching.
///
/// The gap this closes: `lazydap break` between sessions persisted the
/// breakpoint and announced nothing, so an open TUI's gutter went on drawing
/// the old set indefinitely — `break --list` and the screen disagreeing, which
/// is exactly the thing M14's success criteria forbid in the other direction.
#[tokio::test]
async fn a_breakpoint_set_with_no_session_is_announced_to_subscribers() {
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::BreakpointUpdated]).await;
    subscriber.reply().await;

    subscriber
        .send(
            2,
            Request::BreakpointAdd {
                breakpoint: NewBreakpoint {
                    source: PathBuf::from("/p/main.c"),
                    line: 19,
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                    enabled: true,
                },
                dry_run: false,
            },
        )
        .await;

    let mut announced = None;
    let mut answered = false;
    for _ in 0..2 {
        match subscriber.next().await.payload {
            IpcPayload::Event(Event::BreakpointUpdated {
                session_id,
                breakpoint,
            }) => announced = Some((session_id, breakpoint)),
            IpcPayload::Response(Response::Breakpoints(_)) => answered = true,
            other => unreachable!("unexpected frame: {other:?}"),
        }
    }

    assert!(answered, "the caller still gets its own reply");
    let (session_id, breakpoint) = announced.expect("an announcement");
    assert_eq!(
        session_id, None,
        "project scope: there is no adapter opinion in this one",
    );
    assert_eq!(breakpoint.id, Some(BreakpointId(1)));
    assert_eq!(breakpoint.line, Some(19));
}

/// The same for a removal, which no adapter ever reports.
///
/// An adapter is handed the new list for a file and says nothing at all about
/// what is no longer in it, so a client watching only adapter events keeps
/// drawing a breakpoint that is gone — with or without a live session.
#[tokio::test]
async fn a_breakpoint_removed_with_no_session_is_announced_too() {
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::BreakpointUpdated]).await;
    subscriber.reply().await;

    subscriber
        .send(
            2,
            Request::BreakpointAdd {
                breakpoint: NewBreakpoint {
                    source: PathBuf::from("/p/main.c"),
                    line: 19,
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                    enabled: true,
                },
                dry_run: false,
            },
        )
        .await;
    // Drain the add's reply and its announcement.
    for _ in 0..2 {
        subscriber.next().await;
    }

    subscriber
        .send(
            3,
            Request::BreakpointRemove {
                selector: BreakpointSelector::Location {
                    source: PathBuf::from("/p/main.c"),
                    line: 19,
                },
                dry_run: false,
            },
        )
        .await;

    let mut announced = false;
    for _ in 0..2 {
        if let IpcPayload::Event(Event::BreakpointUpdated { session_id, .. }) =
            subscriber.next().await.payload
        {
            assert_eq!(session_id, None);
            announced = true;
        }
    }
    assert!(
        announced,
        "a removal has to be announced or it is invisible"
    );
}

/// A preview changes nothing, so it announces nothing.
#[tokio::test]
async fn a_dry_run_announces_nothing_because_nothing_happened() {
    let daemon = TestDaemon::start().await;
    let mut subscriber = daemon.subscriber(&[EventKind::BreakpointUpdated]).await;
    subscriber.reply().await;

    subscriber
        .send(
            2,
            Request::BreakpointAdd {
                breakpoint: NewBreakpoint {
                    source: PathBuf::from("/p/main.c"),
                    line: 19,
                    column: None,
                    condition: None,
                    hit_condition: None,
                    log_message: None,
                    enabled: true,
                },
                dry_run: true,
            },
        )
        .await;

    match subscriber.next().await.payload {
        IpcPayload::Response(Response::Breakpoints(report)) => assert!(report.dry_run),
        other => unreachable!("expected the preview's reply first, got: {other:?}"),
    }
    // Nothing else should be waiting. `Status` is answered on the same
    // connection, so its reply arriving next proves no event came between.
    subscriber.send(3, Request::Status).await;
    match subscriber.next().await.payload {
        IpcPayload::Response(Response::Status(_)) => {}
        other => unreachable!("a preview announced something: {other:?}"),
    }
}
