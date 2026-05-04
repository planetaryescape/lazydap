use crate::types::DapResponse;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

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

    #[error("adapter did not announce a port on stderr")]
    NoPortFromAdapter,
}

pub type Result<T> = std::result::Result<T, TransportError>;

pub struct DapTransport {
    child: Child,
    stream: BufReader<TcpStream>,
    seq: AtomicI64,
}

impl DapTransport {
    pub async fn spawn(adapter_path: &str) -> Result<Self> {
        // codelldb's "Listening on HOST:PORT" line is logged at debug level — without
        // RUST_LOG=debug in its env, the adapter is silent on stderr and our line-loop
        // hangs forever. See docs/issues/0002-codelldb-version-drift-rust-log.md.
        // Adapter-specific; will be revisited per-adapter in M18.
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
        let port = port.ok_or(TransportError::NoPortFromAdapter)?;

        tokio::spawn(async move {
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "dap.adapter.stderr", "{line}");
            }
        });

        let stream = TcpStream::connect(("127.0.0.1", port)).await?;
        Ok(Self {
            child,
            stream: BufReader::new(stream),
            seq: AtomicI64::new(1),
        })
    }

    pub async fn request<T: Serialize, R: DeserializeOwned>(
        &mut self,
        command: &str,
        args: &T,
    ) -> Result<R> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);

        let outbound = serde_json::json!({
            "seq": seq,
            "type": "request",
            "command": command,
            "arguments": args,
        });
        let body = serde_json::to_vec(&outbound)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stream.get_mut().write_all(header.as_bytes()).await?;
        self.stream.get_mut().write_all(&body).await?;
        self.stream.get_mut().flush().await?;
        tracing::debug!(target: "dap.send", seq, command, "request");

        loop {
            let body = self.read_message_body().await?;
            let value: serde_json::Value = serde_json::from_slice(&body)?;
            let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match kind {
                "response" => {
                    let resp: DapResponse<R> = serde_json::from_slice(&body)?;
                    if resp.request_seq != seq {
                        tracing::warn!(
                            request_seq = resp.request_seq,
                            expected = seq,
                            "out-of-order response, ignoring",
                        );
                        continue;
                    }
                    if !resp.success {
                        return Err(TransportError::Dap(resp.message.unwrap_or_default()));
                    }
                    return resp
                        .body
                        .ok_or_else(|| TransportError::Dap("empty response body".into()));
                }
                "event" => {
                    let event_name = value.get("event").and_then(|v| v.as_str()).unwrap_or("?");
                    tracing::debug!(target: "dap.recv.event", event_name, "ignoring event");
                }
                other => {
                    tracing::warn!(kind = other, "unknown message type");
                }
            }
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
        let len = content_length.ok_or_else(|| TransportError::Header("no Content-Length".into()))?;
        let mut body = vec![0u8; len];
        self.stream.read_exact(&mut body).await?;
        Ok(body)
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }
}
