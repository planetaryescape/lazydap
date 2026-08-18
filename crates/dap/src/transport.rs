use crate::types::{DapEvent, DapRequest, DapResponse};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::ffi::OsStr;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
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

    #[error("adapter did not announce a port ({0})")]
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

/// How to start one TCP adapter and recognise the port it chose.
///
/// Every field differs between the two adapters that use this, which is why it
/// is a value the adapter module supplies rather than anything decided here.
/// codelldb is `codelldb --port 0` with `RUST_LOG=debug` set, announcing
/// `Listening on 127.0.0.1:1234` on **stderr**; delve is
/// `dlv dap --listen=127.0.0.1:0`, announcing
/// `DAP server listening at: 127.0.0.1:1234` on **stdout**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpSpawn {
    pub program: std::path::PathBuf,
    pub args: Vec<String>,
    /// Environment the adapter needs before it will announce anything at all.
    /// codelldb logs its port at debug level, so without `RUST_LOG=debug` it
    /// is silent and the line loop below waits forever.
    pub env: Vec<(String, String)>,
    /// Which of the child's streams the announcement arrives on.
    pub port_stream: AdapterStream,
    /// The text immediately before the address. What follows is read as either
    /// `port N` or a `host:port`.
    pub port_marker: &'static str,
}

/// Which of a child's two output streams something arrives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterStream {
    Stdout,
    Stderr,
}

pub struct DapTransport {
    reader: DapReader,
    writer: DapWriter,
}

/// Longest to wait for a TCP adapter to announce the port it chose.
///
/// A separate, earlier deadline than the daemon's launch timeout, which does
/// not begin until `initialize` is sent — after the port is known. This one
/// covers the gap before that: process spawned, socket held, port never
/// printed. Fifteen seconds matches the handshake's per-message timeout; a
/// slower cold start (a large adapter binary paging in) still fits.
const SPAWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

/// Largest DAP message body this will allocate for, from a `Content-Length` it
/// has not verified. Generous by two orders of magnitude: the biggest real
/// message is a `variables` answer for a large container, which is megabytes at
/// the very worst.
const MAX_MESSAGE_BYTES: usize = 256 * 1024 * 1024;

/// One line off a child's log stream, decoded lossily.
///
/// Nothing guarantees an adapter's log is UTF-8: codelldb runs with
/// `RUST_LOG=debug` and relays bytes the debuggee produced, and LLDB's own
/// diagnostics carry whatever a symbol name happens to contain. `read_line`
/// fails the whole read on the first byte that is not, and every caller here
/// treated that failure as end-of-stream — so one such byte stopped the drain,
/// the pipe filled, and the adapter blocked in a log call answered no more
/// requests. A garbled character costs one `?` instead.
///
/// `None` is the end of the stream. An I/O error is still an error: a pipe that
/// is genuinely broken must not be re-read in a loop.
async fn next_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<String>> {
    let mut line = Vec::new();
    if reader.read_until(b'\n', &mut line).await? == 0 {
        return Ok(None);
    }
    while let Some(b'\n' | b'\r') = line.last() {
        line.pop();
    }
    Ok(Some(String::from_utf8_lossy(&line).into_owned()))
}

/// Read lines until one carries the port marker, returning the port, or `None`
/// if the stream ends first.
///
/// The two spellings the adapters use: codelldb's `Listening on port 1234` and
/// an address, `127.0.0.1:1234`, which is what codelldb's other builds and
/// delve both print.
async fn read_announced_port(reader: &mut BufReader<Source>, marker: &str) -> Result<Option<u16>> {
    while let Some(line) = next_line(reader).await? {
        tracing::debug!(target: "dap.adapter.announce", "{line}");
        if let Some((_, rest)) = line.split_once(marker) {
            let port_str = rest
                .strip_prefix("port ")
                .unwrap_or_else(|| rest.rsplit(':').next().unwrap_or(rest));
            return Ok(Some(port_str.trim().parse()?));
        }
    }
    Ok(None)
}

/// Read a child stream into the log until it ends.
///
/// Every pipe lazydap opens needs one of these. A child whose pipe fills up
/// blocks writing to it, and an adapter blocked in a log call answers no
/// requests.
fn drain(stream: impl AsyncRead + Send + Unpin + 'static) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        while let Ok(Some(line)) = next_line(&mut reader).await {
            tracing::debug!(target: "dap.adapter.stderr", "{line}");
        }
    });
}

impl DapTransport {
    /// Start an adapter that listens on a TCP port and announces it as it
    /// starts, then connect to it.
    ///
    /// One of the two shapes a DAP adapter comes in; the other is
    /// [`spawn_stdio`](Self::spawn_stdio). Which one an adapter wants is a
    /// property of that adapter, so the choice is made in the adapter module
    /// rather than here — and so is every part of *how*, which is what
    /// [`TcpSpawn`] carries. The two TCP adapters lazydap ships agree on
    /// nothing about their own startup: different flags, different environment,
    /// and the announcement on a different stream under different words.
    pub async fn spawn_tcp(spawn: &TcpSpawn) -> Result<Self> {
        Self::spawn_tcp_within(spawn, SPAWN_DEADLINE).await
    }

    /// [`spawn_tcp`](Self::spawn_tcp) with the announcement deadline injected,
    /// so a test can bound it to milliseconds rather than waiting out the real
    /// fifteen seconds.
    async fn spawn_tcp_within(spawn: &TcpSpawn, deadline: std::time::Duration) -> Result<Self> {
        let (stdout, stderr) = match spawn.port_stream {
            AdapterStream::Stdout => (Stdio::piped(), Stdio::piped()),
            AdapterStream::Stderr => (Stdio::null(), Stdio::piped()),
        };
        let mut command = Command::new(&spawn.program);
        command
            .args(&spawn.args)
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true);
        for (key, value) in &spawn.env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;

        // The stream the announcement is *not* on still has to be drained: a
        // child whose pipe fills up blocks writing to it, and an adapter
        // blocked in a log call answers no requests.
        let announcing: Source = match spawn.port_stream {
            AdapterStream::Stdout => {
                drain(child.stderr.take().expect("stderr piped"));
                Box::new(child.stdout.take().expect("stdout piped"))
            }
            AdapterStream::Stderr => Box::new(child.stderr.take().expect("stderr piped")),
        };
        let mut announcing = BufReader::new(announcing);

        // Bound the wait for the announcement, not just the reads inside the
        // handshake that follows it. The launch deadline in the daemon only
        // starts once `initialize` is sent — which is *after* this returns — so
        // an adapter that starts, holds the socket, and never prints its port
        // would otherwise hang the client here with no deadline at all, while
        // the daemon keeps the session slot reserved (D007) and bricks every
        // later launch until `shutdown`. On timeout the child is killed and an
        // honest error is returned; the caller's reservation frees on that
        // error like any other launch failure.
        let announced = match tokio::time::timeout(
            deadline,
            read_announced_port(&mut announcing, spawn.port_marker),
        )
        .await
        {
            Ok(result) => result?,
            Err(_elapsed) => {
                let _ = child.kill().await;
                return Err(TransportError::NoPortFromAdapter(format!(
                    "adapter did not announce a port within {}s",
                    deadline.as_secs(),
                )));
            }
        };
        let Some(port) = announced else {
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
            while let Ok(Some(line)) = next_line(&mut announcing).await {
                tracing::debug!(target: "dap.adapter.announce", "{line}");
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
        drain(child.stderr.take().expect("stderr piped"));

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
        // A header is the adapter's word for how much to allocate, and this
        // allocates it before a single byte of the body has arrived. A
        // desynchronised stream — a reverse request answered late, a frame read
        // half way — puts arbitrary bytes where the length belongs, and
        // `Content-Length: 9999999999999` then takes the daemon down with an
        // allocation failure rather than with an error anybody can read. No DAP
        // message is anywhere near this size.
        if len > MAX_MESSAGE_BYTES {
            return Err(TransportError::Header(format!(
                "Content-Length {len} exceeds the {MAX_MESSAGE_BYTES}-byte cap",
            )));
        }
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
        let seq = self.next_seq();
        self.send_request_with_seq(seq, command, args).await?;
        Ok(seq)
    }

    /// The `seq` the next request may carry.
    ///
    /// Split from the write for one caller: the daemon registers a waiter for
    /// the response *before* writing, and holding the map that the read pump
    /// delivers into across a write the adapter may be slow to accept is how a
    /// busy socket deadlocks a session. Ordering is still the writer's:
    /// `&mut self` means nothing else can write between this and the send.
    pub fn next_seq(&mut self) -> i64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    /// Write a request under a `seq` already taken from [`next_seq`](Self::next_seq).
    pub async fn send_request_with_seq<T: Serialize>(
        &mut self,
        seq: i64,
        command: &str,
        args: &T,
    ) -> Result<()> {
        let outbound = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": args,
        });
        self.write_message(&outbound).await?;
        tracing::debug!(target: "dap.send", seq, command, "request");

        Ok(())
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

    /// Wait for the adapter to exit of its own accord.
    ///
    /// For the caller that asked it to leave and would rather it went by
    /// itself: an adapter killed mid-`disconnect` never gets to detach from its
    /// debuggee. Cancellation-safe, so a caller can bound it with a timeout and
    /// pull the plug when the patience runs out. Closing stdin is part of
    /// going: a stdio adapter is waiting on it.
    pub async fn wait(&mut self) -> Result<()> {
        self.child.wait().await?;
        Ok(())
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
    async fn a_tcp_adapter_that_never_announces_a_port_times_out_rather_than_hanging() {
        // Finding 3: an adapter that spawns, holds its streams open, and never
        // prints its port would otherwise hang the client forever — the
        // daemon's launch deadline does not start until after the port is
        // known. `sleep` is exactly that adapter: alive, silent on stdout. The
        // deadline is injected short so the test does not wait out the real
        // fifteen seconds.
        let spawn = TcpSpawn {
            program: "sleep".into(),
            args: vec!["30".to_string()],
            env: Vec::new(),
            port_stream: AdapterStream::Stdout,
            port_marker: "Listening on ",
        };

        let started = std::time::Instant::now();
        let result = DapTransport::spawn_tcp_within(&spawn, Duration::from_millis(300)).await;

        match result {
            Err(TransportError::NoPortFromAdapter(detail)) => {
                assert!(
                    detail.contains("within"),
                    "should name the deadline: {detail}"
                );
            }
            Err(other) => {
                unreachable!("expected a spawn timeout, got a different error: {other:?}")
            }
            // `DapTransport` is not `Debug`, so this arm cannot print it — the
            // message is what matters, and it must not have connected at all.
            Ok(_) => unreachable!("a silent adapter must not produce a live transport"),
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "it must give up at the deadline, not hang: {:?}",
            started.elapsed(),
        );
    }

    #[tokio::test]
    async fn a_content_length_bigger_than_the_cap_is_refused_rather_than_allocated() {
        // The header is the adapter's word for how much to allocate, and a
        // desynchronised stream can put anything there. Reserving it first and
        // asking questions later takes the daemon out with an allocation
        // failure rather than an error.
        let header = format!("Content-Length: {}\r\n\r\n", u32::MAX);
        let source: Source = Box::new(std::io::Cursor::new(header.into_bytes()));
        let mut reader = DapReader {
            stream: BufReader::new(source),
        };

        match reader.read_message_body().await {
            Err(TransportError::Header(detail)) => {
                assert!(detail.contains("cap"), "should name the cap: {detail}")
            }
            other => unreachable!("expected a header error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_line_that_is_not_utf8_does_not_end_the_drain() {
        // codelldb runs with `RUST_LOG=debug` and relays bytes it was given.
        // `read_line` fails the whole read on the first one that is not UTF-8,
        // and the loop that treated that as end-of-stream stopped draining —
        // after which the pipe fills and the adapter blocks in a log call.
        let mut reader = BufReader::new(&b"first\n\xff\xfe not text\nafter\n"[..]);

        let mut lines = Vec::new();
        while let Some(line) = next_line(&mut reader).await.expect("no io error") {
            lines.push(line);
        }

        assert_eq!(lines.len(), 3, "got: {lines:?}");
        assert_eq!(lines[0], "first");
        assert_eq!(lines[2], "after", "the drain kept going: {lines:?}");
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
