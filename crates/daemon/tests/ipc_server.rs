//! The daemon's IPC surface, over a real Unix socket with real framing.
//!
//! Nothing here is mocked: a genuine `UnixListener`, the shipped codec, and
//! the same `serve_client` the accept loop runs (D014). The one thing these
//! tests do not do is launch a debuggee — that needs a live adapter and is
//! covered by the milestone's manual verification.

use lazydap_daemon::client::DaemonClient;
use lazydap_daemon::server::serve_client;
use lazydap_daemon::state::DaemonState;
use lazydap_protocol::{
    ErrorCode, IpcConnection, IpcMessage, IpcPayload, LAZYDAP_PROTOCOL_VERSION, Request, Response,
};
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
        let state = DaemonState::new("lazydap-test".to_string(), store);

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
async fn an_unimplemented_request_is_an_error_the_connection_survives() {
    let daemon = TestDaemon::start().await;
    let mut client = daemon.client().await;

    let error = client
        .request(Request::Subscribe { channels: vec![] })
        .await
        .expect_err("subscription lands at M11");
    assert_eq!(error.label, "Unsupported", "got: {error}");

    // The point of answering rather than hanging up: the client can carry on.
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
            payload: IpcPayload::Request(Request::Shutdown { dry_run: false }),
        })
        .await
        .expect("send");

    let reply = connection.recv().await.expect("recv").expect("a reply");
    match reply.payload {
        IpcPayload::Response(Response::ShuttingDown { dry_run, .. }) => assert!(!dry_run),
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

#[tokio::test]
async fn shutdown_is_acknowledged_and_tells_the_daemon_to_stop() {
    let daemon = TestDaemon::start().await;
    let mut client = daemon.client().await;

    assert!(!daemon.state.shutdown_requested());
    let response = client
        .request(Request::Shutdown { dry_run: false })
        .await
        .expect("shutdown");

    assert!(matches!(
        response,
        Response::ShuttingDown { dry_run: false, .. }
    ));
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
