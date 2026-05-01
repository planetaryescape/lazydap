# M2 — Initialize handshake

## What

Take M1 and structure it into a reusable `DapTransport` struct that:

1. Spawns codelldb, manages the child process and TCP connection.
2. Has a `request<T: Serialize, R: DeserializeOwned>(&mut self, command: &str, args: &T) -> Result<R>` method.
3. Has a `read_event(&mut self) -> Result<DapEvent>` method (for unprompted messages).
4. Sends `initialize`, parses the typed `Capabilities` response.
5. Prints the capabilities.

This is the start of `crates/dap/`. ~150 lines.

## Why

M1 read one message ad-hoc. M2 generalises into reusable transport code. Every later milestone uses this transport.

## How

### Step 1 — Create `crates/dap` crate

```bash
mkdir -p crates/dap/src
```

`crates/dap/Cargo.toml`:

```toml
[package]
name = "lazydap-dap"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
thiserror.workspace = true
tracing.workspace = true
```

Add to root `Cargo.toml` workspace members.

### Step 2 — Define types

`crates/dap/src/types.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct DapRequest<'a, T: Serialize> {
    pub seq: i64,
    #[serde(rename = "type")]
    pub message_type: &'static str, // "request"
    pub command: &'a str,
    pub arguments: &'a T,
}

#[derive(Debug, Deserialize)]
pub struct DapResponse<R> {
    pub seq: i64,
    pub request_seq: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub command: String,
    pub success: bool,
    pub message: Option<String>, // error message
    pub body: Option<R>,
}

#[derive(Debug, Deserialize)]
pub struct DapEvent {
    pub seq: i64,
    #[serde(rename = "type")]
    pub message_type: String,
    pub event: String,
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub supports_configuration_done_request: bool,
    pub supports_function_breakpoints: bool,
    pub supports_conditional_breakpoints: bool,
    pub supports_hit_conditional_breakpoints: bool,
    pub supports_evaluate_for_hovers: bool,
    pub supports_step_back: bool,
    pub supports_set_variable: bool,
    pub supports_restart_frame: bool,
    pub supports_goto_targets_request: bool,
    pub supports_step_in_targets_request: bool,
    pub supports_completions_request: bool,
    pub supports_modules_request: bool,
    pub supports_restart_request: bool,
    pub supports_exception_options: bool,
    pub supports_value_formatting_options: bool,
    pub supports_exception_info_request: bool,
    pub support_terminate_debuggee: bool,
    pub supports_delayed_stack_trace_loading: bool,
    pub supports_loaded_sources_request: bool,
    pub supports_log_points: bool,
    pub supports_terminate_threads_request: bool,
    pub supports_data_breakpoints: bool,
    pub supports_read_memory_request: bool,
    pub supports_write_memory_request: bool,
    pub supports_disassemble_request: bool,
    pub supports_cancel_request: bool,
    // ... add others as needed
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeArgs {
    pub client_id: String,
    pub client_name: String,
    pub adapter_id: String,
    pub lines_start_at1: bool,
    pub columns_start_at1: bool,
    pub path_format: String, // "path" | "uri"
    pub supports_variable_type: bool,
    pub supports_variable_paging: bool,
    pub supports_run_in_terminal_request: bool,
    pub locale: String,
}

impl Default for InitializeArgs {
    fn default() -> Self {
        Self {
            client_id: "lazydap".into(),
            client_name: "lazydap".into(),
            adapter_id: "lldb".into(),
            lines_start_at1: true,
            columns_start_at1: true,
            path_format: "path".into(),
            supports_variable_type: true,
            supports_variable_paging: true,
            supports_run_in_terminal_request: false,
            locale: "en-US".into(),
        }
    }
}
```

### Step 3 — Transport

`crates/dap/src/transport.rs`:

```rust
use crate::types::{DapEvent, DapRequest, DapResponse};
use serde::{de::DeserializeOwned, Serialize};
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
}

pub type Result<T> = std::result::Result<T, TransportError>;

pub struct DapTransport {
    child: Child,
    stream: BufReader<TcpStream>,
    seq: AtomicI64,
}

impl DapTransport {
    pub async fn spawn(adapter_path: &str) -> Result<Self> {
        let mut child = Command::new(adapter_path)
            .arg("--port")
            .arg("0")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stderr = child.stderr.take().expect("stderr piped");
        let mut lines = BufReader::new(stderr).lines();

        let mut port: Option<u16> = None;
        while let Some(line) = lines.next_line().await? {
            tracing::debug!(target: "dap.adapter.stderr", "{line}");
            if let Some(rest) = line.strip_prefix("Listening on port ") {
                port = Some(rest.trim().parse()?);
                break;
            }
        }
        let port = port.ok_or(TransportError::Dap("no port from adapter".into()))?;

        // Spawn a task to drain remaining stderr so adapter doesn't block.
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
        let req = DapRequest {
            seq,
            message_type: "request",
            command,
            arguments: args,
        };
        let body = serde_json::to_vec(&req)?;
        let header = format!("Content-Length: {}\r\n\r\n", body.len());
        self.stream.get_mut().write_all(header.as_bytes()).await?;
        self.stream.get_mut().write_all(&body).await?;
        self.stream.get_mut().flush().await?;
        tracing::debug!(target: "dap.send", seq, command, "request");

        // Read until we see the matching response. Discard events for now.
        loop {
            let body = self.read_message_body().await?;
            // Check type — could be a response or an event.
            let value: serde_json::Value = serde_json::from_slice(&body)?;
            let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "response" {
                let resp: DapResponse<R> = serde_json::from_slice(&body)?;
                if resp.request_seq != seq {
                    tracing::warn!(?resp.request_seq, "out-of-order response, ignoring");
                    continue;
                }
                if !resp.success {
                    return Err(TransportError::Dap(resp.message.unwrap_or_default()));
                }
                return resp.body.ok_or_else(|| TransportError::Dap("empty response body".into()));
            } else {
                tracing::debug!(target: "dap.recv", "ignoring event during request");
                // For M2 we discard events. M3 will keep them.
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
                content_length = Some(v.trim().parse().map_err(|_| TransportError::Header(trimmed.into()))?);
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
```

### Step 4 — `lib.rs` and example

`crates/dap/src/lib.rs`:

```rust
pub mod transport;
pub mod types;

pub use transport::{DapTransport, TransportError};
pub use types::*;
```

`crates/daemon/examples/m2_initialize.rs`:

```rust
use lazydap_dap::{Capabilities, DapTransport, InitializeArgs};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let mut transport = DapTransport::spawn("codelldb").await?;
    let caps: Capabilities = transport.request("initialize", &InitializeArgs::default()).await?;

    println!("--- capabilities ---");
    println!("{}", serde_json::to_string_pretty(&caps)?);

    transport.shutdown().await?;
    Ok(())
}
```

Add `lazydap-dap = { path = "../dap" }` to `crates/daemon/Cargo.toml`.

### Step 5 — Run

```bash
cargo run --example m2_initialize
RUST_LOG=dap.send=debug,dap.recv=debug cargo run --example m2_initialize
```

Capabilities printed.

## Success criteria

- `cargo run --example m2_initialize` prints a Capabilities struct showing real values from codelldb.
- The transport handles:
  - Spawning the adapter
  - Parsing the port
  - Connecting TCP
  - Sending a typed request
  - Reading the matching response by seq

## Files

- `crates/dap/Cargo.toml` (new)
- `crates/dap/src/lib.rs`, `transport.rs`, `types.rs` (new)
- `crates/daemon/examples/m2_initialize.rs` (new)
- `crates/daemon/Cargo.toml` — add `lazydap-dap` dep

## Verify

```bash
cargo build -p lazydap-dap
cargo run --example m2_initialize
# Output: pretty-printed Capabilities struct.

# Invariant: no leaked codelldb processes
pgrep codelldb || echo "(none)"
```

## Depends on

- [`M01-read-one-message`](M01-read-one-message.md) — you understand framing.

## Notes

- **Events are ignored for now.** M3 will start collecting them.
- **No reconnection logic.** If the adapter dies, the transport errors. M5 hardens this.
- **The `Capabilities` struct is incomplete.** Add fields as you need them in later milestones. Don't try to enumerate the entire DAP spec now.
- **Use `tracing` from M2 onward.** Don't `println!` debug calls. The transport already does this.
