use crate::error::{CliError, Result};
use lazydap_protocol::{
    ErrorCode, IpcConnection, IpcError, IpcMessage, IpcPayload, LAZYDAP_PROTOCOL_VERSION, Request,
    Response,
};
use std::io;
use std::path::Path;
use std::time::Duration;
use tokio::net::UnixStream;

/// How long to wait for one response.
///
/// Generous because it also covers `launch`, which waits on an adapter that is
/// starting a process. The daemon has its own, shorter, adapter timeouts, so
/// hitting this one means the daemon itself is wedged.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A connection to the daemon, with the version handshake already done.
pub struct DaemonClient {
    connection: IpcConnection<UnixStream>,
    next_id: u64,
    /// What the daemon said its version was, kept for error messages.
    pub daemon_version: u32,
    pub instance: String,
}

impl DaemonClient {
    /// Connect and exchange versions.
    ///
    /// The first thing on any connection is `Ping`, so a client never sends a
    /// real request to a daemon it cannot understand.
    pub async fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket).await.map_err(|source| {
            CliError::unreachable(anyhow::anyhow!(
                "cannot connect to the daemon socket at {}: {source}",
                socket.display(),
            ))
        })?;

        let mut client = Self {
            connection: IpcConnection::new(stream),
            next_id: 1,
            daemon_version: 0,
            instance: String::new(),
        };

        match client.request(Request::Ping).await? {
            Response::Pong {
                version, instance, ..
            } => {
                client.daemon_version = version;
                client.instance = instance;
            }
            other => {
                return Err(CliError::general(anyhow::anyhow!(
                    "the daemon answered a ping with {other:?}"
                )));
            }
        }

        if client.daemon_version != LAZYDAP_PROTOCOL_VERSION {
            return Err(version_mismatch(client.daemon_version));
        }
        Ok(client)
    }

    /// Send one request and wait for the response with the same id.
    pub async fn request(&mut self, request: Request) -> Result<Response> {
        self.request_within(request, Some(REQUEST_TIMEOUT)).await
    }

    /// The same, for a request the caller knows will take a while.
    ///
    /// `continue --wait --timeout 300` asks the daemon to block for five
    /// minutes; a client that gave up after its own sixty seconds would report
    /// a timeout that never happened and abandon a perfectly healthy wait.
    ///
    /// `None` means no client-side limit at all, for `--timeout 0`. It is an
    /// `Option` rather than an enormous `Duration` because "an enormous
    /// duration" is not a deadline: `Instant + Duration` panics on overflow,
    /// and a sentinel large enough to mean "never" is large enough to do it.
    pub async fn request_within(
        &mut self,
        request: Request,
        timeout: Option<Duration>,
    ) -> Result<Response> {
        let id = self.next_id;
        self.next_id += 1;

        self.connection
            .send(IpcMessage::request(id, request))
            .await?;

        let deadline = timeout.map(|timeout| tokio::time::Instant::now() + timeout);
        loop {
            let received = match deadline {
                Some(deadline) => tokio::time::timeout_at(deadline, self.connection.recv())
                    .await
                    .map_err(|_| {
                        CliError::general(anyhow::anyhow!(
                            "the daemon did not answer within {}s",
                            timeout.unwrap_or_default().as_secs(),
                        ))
                    })?,
                None => self.connection.recv().await,
            };

            let message = received
                .map_err(|source| classify_read_error(source, id))?
                .ok_or_else(|| {
                    CliError::unreachable(anyhow::anyhow!(
                        "the daemon closed the connection before answering"
                    ))
                })?;

            // Events are unsolicited and carry id 0. M5 never subscribes, but
            // a daemon that starts sending them must not derail a reply.
            if message.id != id {
                tracing::debug!(
                    target: "daemon.ipc",
                    frame_id = message.id,
                    awaiting = id,
                    "ignoring an unsolicited frame",
                );
                continue;
            }

            return match message.payload {
                IpcPayload::Response(response) => Ok(response),
                IpcPayload::Error(error) => Err(error.into()),
                other => Err(CliError::general(anyhow::anyhow!(
                    "expected a response, got {other:?}"
                ))),
            };
        }
    }
}

/// The client and the daemon were built against different protocols.
pub fn version_mismatch(daemon_version: u32) -> CliError {
    IpcError::new(
        ErrorCode::VersionMismatch,
        format!(
            "this lazydap speaks protocol v{LAZYDAP_PROTOCOL_VERSION}, \
             the running daemon speaks v{daemon_version}"
        ),
    )
    .with_details(serde_json::json!({
        "client_version": LAZYDAP_PROTOCOL_VERSION,
        "daemon_version": daemon_version,
    }))
    .into()
}

/// A daemon from another build can fail to decode our frame and hang up. Say
/// so, rather than reporting a bare I/O error.
fn classify_read_error(source: io::Error, id: u64) -> CliError {
    match source.kind() {
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
            CliError::unreachable(anyhow::anyhow!(
                "the daemon sent a frame this build cannot read while awaiting response {id} \
                 ({source}). Run `lazydap shutdown` and try again."
            ))
        }
        _ => CliError::general(source),
    }
}
