use crate::types::{DapEvent, DapRequest, DapResponse};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::ffi::OsStr;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

/// The reading half of whatever carries DAP, once the transport owns it.
///
/// Boxed rather than generic because the two adapters lazydap ships reach
/// their adapter over different things — codelldb over a TCP socket, debugpy
/// over a child's stdout — and every type that holds a reader would otherwise
/// grow a parameter it does nothing with. The cost is one virtual call per
/// read of a stream that is already crossing a process boundary.
type Source = Box<dyn AsyncRead + Send + Unpin>;

/// The writing half, boxed for the same reason.
type Sink = Box<dyn AsyncWrite + Send + Unpin>;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid header: {0}")]
    Header(String),

    #[error("adapter exited unexpectedly")]
    AdapterExited,

    #[error("dap error: {0}")]
    Dap(String),

    #[error("port parse: {0}")]
    PortParse(#[from] std::num::ParseIntError),

    #[error("adapter did not announce a port on stderr ({0})")]
    NoPortFromAdapter(String),

    #[error("response for request_seq {request_seq} while waiting on {expected}")]
    UnexpectedResponse { request_seq: i64, expected: i64 },
}

pub type Result<T> = std::result::Result<T, TransportError>;

/// One message read off the adapter connection. Responses answer a request we
/// sent; events are adapter-initiated and can arrive at any time, including
/// between a request and its response.
#[derive(Debug)]
pub enum Incoming {
    Response(DapResponse<serde_json::Value>),
    Event(DapEvent),
    /// The adapter is asking *us* for something (`runInTerminal`,
    /// `startDebugging`). lazydap advertises support for neither, so every one
    /// of these is answered with a refusal — see
    /// [`DapWriter::refuse`]. It is a variant rather than an
    /// error because an adapter that asks anyway has not malfunctioned, and
    /// killing the session over a question we can simply answer "no" to is a
    /// debug session lost to politeness.
    ReverseRequest(DapRequest),
}

pub struct DapTransport {
    reader: DapReader,
    writer: DapWriter,
}

impl DapTransport {
    /// Start an adapter that listens on a TCP port and announces it on stderr,
    /// then connect to it.
    ///
    /// One of the two shapes a DAP adapter comes in, and the one codelldb
    /// uses. The other is [`spawn_stdio`](Self::spawn_stdio). Which one an
    /// adapter wants is a property of that adapter, so the choice is made in
    /// the adapter module rather than here.
    pub async fn spawn_tcp(adapter_path: &str) -> Result<Self> {
        // codelldb's "Listening on HOST:PORT" line is logged at debug level — without
        // RUST_LOG=debug in its env, the adapter is silent on stderr and our line-loop
        // hangs forever. See docs/issues/0002-codelldb-version-drift-rust-log.md.
        let mut child = Command::new(adapter_path)
            .arg("--port")
            .arg("0")
            .env("RUST_LOG", "debug")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stderr = child.stderr.take().expect("stderr piped");
        let mut lines = BufReader::new(stderr).lines();

        let mut port: Option<u16> = None;
        while let Some(line) = lines.next_line().await? {
            tracing::debug!(target: "dap.adapter.stderr", "{line}");
            if let Some((_, rest)) = line.split_once("Listening on ") {
                let port_str = rest
                    .strip_prefix("port ")
                    .unwrap_or_else(|| rest.rsplit(':').next().unwrap_or(rest));
                port = Some(port_str.trim().parse()?);
                break;
            }
        }
        let Some(port) = port else {
            // A missing port line usually means the adapter died on startup
            // (e.g. the liblldb path footgun in docs/reference/codelldb-quirks.md).
            // Report its exit status rather than a bare "no port".
            let detail = match child.try_wait()? {
                Some(status) => format!("adapter exited: {status}"),
                None => "adapter still running but never announced a port".to_string(),
            };
            return Err(TransportError::NoPortFromAdapter(detail));
        };

        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "dap.adapter.stderr", "{line}");
            }
        });

        let stream = TcpStream::connect(("127.0.0.1", port)).await?;
        Ok(Self::from_tcp(child, stream))
    }

    /// Start an adapter that speaks DAP over its own stdin and stdout.
    ///
    /// The framing is identical to the TCP case — `Content-Length` headers and
    /// a JSON body — so only the pipes differ. debugpy works this way
    /// (`python3 -m debugpy.adapter`), which is why the command is a program
    /// *and arguments* rather than a bare path: the adapter is a module of an
    /// interpreter, not an executable of its own.
    ///
    /// stderr is drained into the log rather than left unread. A child whose
    /// stderr pipe fills up blocks writing to it, and an adapter blocked in a
    /// log call answers no requests.
    pub async fn spawn_stdio(program: &OsStr, args: &[String]) -> Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "dap.adapter.stderr", "{line}");
            }
        });

        Ok(Self::from_parts(child, Box::new(stdout), Box::new(stdin)))
    }

    /// Send a request and read until its response arrives, discarding events.
    /// Only safe when nothing else is in flight: a response for another request
    /// is reported as [`TransportError::UnexpectedResponse`] rather than
    /// dropped. Drive concurrent traffic with
    /// [`send_request`](Self::send_request) + [`read_incoming`](Self::read_incoming).
    pub async fn request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        command: &str,
        args: &T,
    ) -> Result<R> {
        let seq = self.send_request(command, args).await?;

        loop {
            match self.reader.read_incoming().await? {
                Incoming::Response(resp) => {
                    if resp.request_seq != seq {
                        return Err(TransportError::UnexpectedResponse {
                            request_seq: resp.request_seq,
                            expected: seq,
                        });
                    }
                    if !resp.success {
                        return Err(TransportError::Dap(resp.message.unwrap_or_default()));
                    }
                    // `configurationDone`, `disconnect` and some `continue`
                    // implementations legitimately succeed with no body at
                    // all. Deserialise `R` from JSON null in that case, so
                    // `()` and `serde_json::Value` both work while a response
                    // type that genuinely needs fields still fails loudly.
                    return Ok(serde_json::from_value(
                        resp.body.unwrap_or(serde_json::Value::Null),
                    )?);
                }
                Incoming::Event(event) => {
                    tracing::debug!(target: "dap.recv.event", event_name = event.event, "ignoring event");
                }
                Incoming::ReverseRequest(request) => self.writer.refuse(&request).await?,
            }
        }
    }

    /// Write a request and return its `seq` without waiting for the response.
    /// The caller correlates the response itself via [`read_incoming`](Self::read_incoming).
    pub async fn send_request<T: Serialize>(&mut self, command: &str, args: &T) -> Result<i64> {
        self.writer.send_request(command, args).await
    }

    /// Read the next message off the socket, in receipt order, whatever it is.
    ///
    /// **Not cancellation-safe** — see [`DapReader::read_incoming`].
    pub async fn read_incoming(&mut self) -> Result<Incoming> {
        self.reader.read_incoming().await
    }

    /// Answer a reverse request with "no" — see [`DapWriter::refuse`].
    pub async fn refuse(&mut self, request: &DapRequest) -> Result<()> {
        self.writer.refuse(request).await
    }

    /// Hand reads and writes to separate owners.
    ///
    /// The daemon needs this: a long-lived task owns [`DapReader`] and does
    /// nothing but read, which is the only way to consume the adapter's stream
    /// without ever cancelling a read mid-frame, while request handlers write
    /// through the shared [`DapWriter`].
    pub fn split(self) -> (DapReader, DapWriter) {
        (self.reader, self.writer)
    }

    /// Kill the adapter process.
    pub async fn shutdown(self) -> Result<()> {
        self.writer.shutdown().await
    }

    fn from_tcp(child: Child, stream: TcpStream) -> Self {
        // Split before the first read so the buffered reader owns its half from
        // the start: splitting later would strand whatever bytes the buffer had
        // already pulled off the socket.
        let (read_half, write_half) = stream.into_split();
        Self::from_parts(child, Box::new(read_half), Box::new(write_half))
    }

    fn from_parts(child: Child, source: Source, sink: Sink) -> Self {
        Self {
            reader: DapReader {
                stream: BufReader::new(source),
            },
            writer: DapWriter {
                child,
                stream: sink,
                seq: AtomicI64::new(1),
            },
        }
    }
}

/// The read side of an adapter connection.
pub struct DapReader {
    stream: BufReader<Source>,
}

impl DapReader {
    /// Read the next message off the socket, in receipt order, whatever it is.
    ///
    /// **Not cancellation-safe.** Cancelling this future — dropping it, or
    /// wrapping it in `tokio::time::timeout` and having the timer fire — can
    /// leave the stream mid-frame, part way through a header or a body. After
    /// a cancelled read the reader must not be reused: the next read starts
    /// mid-frame and misparses, and the session is corrupted from there on.
    /// Shut the adapter down instead. Owning this half in a dedicated task
    /// that only ever reads is the cancellation-safe pattern, and is what the
    /// daemon's session pump does.
    pub async fn read_incoming(&mut self) -> Result<Incoming> {
        let body = self.read_message_body().await?;
        let value: serde_json::Value = serde_json::from_slice(&body)?;
        match value.get("type").and_then(|v| v.as_str()) {
            Some("response") => {
                let resp: DapResponse<serde_json::Value> = serde_json::from_slice(&body)?;
                tracing::debug!(
                    target: "dap.recv.response",
                    request_seq = resp.request_seq,
                    command = resp.command,
                    success = resp.success,
                    "response",
                );
                Ok(Incoming::Response(resp))
            }
            Some("event") => {
                let event: DapEvent = serde_json::from_slice(&body)?;
                tracing::debug!(target: "dap.recv.event", event = event.event, "event");
                Ok(Incoming::Event(event))
            }
            // A reverse request. Reading it as a message we simply do not
            // understand would kill the session over a question, which is how
            // this used to behave.
            Some("request") => {
                let request: DapRequest = serde_json::from_slice(&body)?;
                tracing::debug!(
                    target: "dap.recv.request",
                    seq = request.seq,
                    command = request.command,
                    "reverse request",
                );
                Ok(Incoming::ReverseRequest(request))
            }
            other => Err(TransportError::Dap(format!(
                "unknown message type: {other:?}"
            ))),
        }
    }

    async fn read_message_body(&mut self) -> Result<Vec<u8>> {
        let mut header_buf = String::new();
        let mut content_length: Option<usize> = None;
        loop {
            header_buf.clear();
            let n = self.stream.read_line(&mut header_buf).await?;
            if n == 0 {
                return Err(TransportError::AdapterExited);
            }
            let trimmed = header_buf.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(v) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(
                    v.trim()
                        .parse()
                        .map_err(|_| TransportError::Header(trimmed.into()))?,
                );
            }
        }
        let len =
            content_length.ok_or_else(|| TransportError::Header("no Content-Length".into()))?;
        let mut body = vec![0u8; len];
        self.stream.read_exact(&mut body).await?;
        Ok(body)
    }
}

/// The write side of an adapter connection, and the adapter process itself.
///
/// The process lives here because killing it is a control action, and control
/// actions travel with the writer: whoever can send `disconnect` is also the
/// one that gets to give up and pull the plug.
pub struct DapWriter {
    child: Child,
    stream: Sink,
    seq: AtomicI64,
}

impl DapWriter {
    /// Write a request and return its `seq` without waiting for the response.
    pub async fn send_request<T: Serialize>(&mut self, command: &str, args: &T) -> Result<i64> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        let outbound = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": args,
        });
        self.write_message(&outbound).await?;
        tracing::debug!(target: "dap.send", seq, command, "request");

        Ok(seq)
    }

    /// Answer a reverse request with "no".
    ///
    /// lazydap advertises neither `runInTerminal` nor `startDebugging`, so an
    /// adapter should not be asking — but adapters do, and the alternative to
    /// answering is silence. Silence is the worse failure of the two: the
    /// adapter waits for a reply that is never coming, the debuggee never
    /// starts, and the session dies at a timeout that names the wrong thing.
    /// A refusal it can read leaves the adapter free to fall back or fail
    /// with its own words.
    pub async fn refuse(&mut self, request: &DapRequest) -> Result<()> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        let outbound = serde_json::json!({
            "seq": seq,
            "type": "response",
            "request_seq": request.seq,
            "command": request.command,
            "success": false,
            "message": format!("lazydap does not support `{}`", request.command),
        });
        self.write_message(&outbound).await?;
        tracing::warn!(
            target: "dap.send",
            seq,
            command = request.command,
            "refused a reverse request lazydap never advertised support for",
        );

        Ok(())
    }

    async fn write_message(&mut self, message: &serde_json::Value) -> Result<()> {
        let body = serde_json::to_vec(message)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stream.write_all(header.as_bytes()).await?;
        self.stream.write_all(&body).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Whether the adapter process has already exited, without blocking.
    pub fn has_exited(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }

    /// Kill the adapter process.
    pub async fn shutdown(mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;

    /// Connect to a loopback socket that replays `frames` as DAP messages.
    /// `child` is a real stand-in process: the transport owns an adapter
    /// lifetime, and the tests exercise the wire, not a mocked socket.
    async fn scripted_adapter(frames: Vec<String>) -> DapTransport {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("local addr");

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            for frame in frames {
                let header = format!("Content-Length: {}\r\n\r\n", frame.len());
                stream
                    .write_all(header.as_bytes())
                    .await
                    .expect("write header");
                stream
                    .write_all(frame.as_bytes())
                    .await
                    .expect("write body");
            }
            stream.flush().await.expect("flush");
            // Keep the connection open so the client sees EOF only if it
            // over-reads the script.
            tokio::time::sleep(Duration::from_secs(5)).await;
        });

        let stream = TcpStream::connect(addr).await.expect("connect");
        let child = Command::new("sleep")
            .arg("30")
            .kill_on_drop(true)
            .spawn()
            .expect("spawn stand-in adapter process");

        DapTransport::from_tcp(child, stream)
    }

    #[tokio::test]
    async fn read_incoming_demuxes_events_from_responses() {
        let mut transport = scripted_adapter(vec![
            r#"{"seq":1,"type":"event","event":"output","body":{"category":"stdout","output":"hello\n"}}"#.to_string(),
            r#"{"seq":2,"type":"response","request_seq":1,"command":"launch","success":true}"#
                .to_string(),
        ])
        .await;

        let event = match transport.read_incoming().await.expect("read event") {
            Incoming::Event(event) => event,
            other => unreachable!("expected an event, got: {other:?}"),
        };
        assert_eq!(event.event, "output");
        assert_eq!(
            event.body.as_ref().and_then(|b| b["output"].as_str()),
            Some("hello\n"),
        );

        let resp = match transport.read_incoming().await.expect("read response") {
            Incoming::Response(resp) => resp,
            other => unreachable!("expected a response, got: {other:?}"),
        };
        assert_eq!(resp.command, "launch");
        assert_eq!(resp.request_seq, 1);
        assert!(resp.success);
    }

    #[tokio::test]
    async fn request_succeeds_when_the_response_carries_no_body() {
        let mut transport = scripted_adapter(vec![
            r#"{"seq":1,"type":"response","request_seq":1,"command":"configurationDone","success":true}"#
                .to_string(),
        ])
        .await;

        transport
            .request::<_, ()>("configurationDone", &serde_json::json!({}))
            .await
            .expect("empty-body success is not an error");
    }

    #[tokio::test]
    async fn request_surfaces_a_response_meant_for_another_request() {
        let mut transport = scripted_adapter(vec![
            r#"{"seq":1,"type":"response","request_seq":99,"command":"launch","success":true}"#
                .to_string(),
        ])
        .await;

        let err = transport
            .request::<_, serde_json::Value>("threads", &serde_json::json!({}))
            .await
            .expect_err("mismatched request_seq must not be swallowed");
        assert!(
            matches!(
                err,
                TransportError::UnexpectedResponse {
                    request_seq: 99,
                    expected: 1
                }
            ),
            "got: {err}",
        );
    }

    #[tokio::test]
    async fn send_request_hands_back_increasing_seqs() {
        let mut transport = scripted_adapter(vec![]).await;

        let first = transport
            .send_request("launch", &serde_json::json!({}))
            .await
            .expect("send launch");
        let second = transport
            .send_request("setBreakpoints", &serde_json::json!({}))
            .await
            .expect("send setBreakpoints");

        assert_eq!(first, 1, "got: {first}");
        assert_eq!(second, 2, "got: {second}");
    }

    #[tokio::test]
    async fn a_split_transport_reads_and_writes_from_separate_owners() {
        let transport = scripted_adapter(vec![
            r#"{"seq":1,"type":"event","event":"initialized"}"#.to_string(),
        ])
        .await;
        let (mut reader, mut writer) = transport.split();

        // The read half blocks in its own task while the write half is still
        // usable — the whole point of the split.
        let pump = tokio::spawn(async move { reader.read_incoming().await });

        let seq = writer
            .send_request("configurationDone", &serde_json::json!({}))
            .await
            .expect("write while the reader is blocked");
        assert_eq!(seq, 1, "got: {seq}");

        let event = match pump.await.expect("join").expect("read event") {
            Incoming::Event(event) => event,
            other => unreachable!("expected an event, got: {other:?}"),
        };
        assert_eq!(event.event, "initialized");
    }
}
