# M1 — Read one message

## What

A small example binary that:

1. Spawns codelldb (same as M0)
2. Parses the port from stderr
3. Connects via TCP
4. Sends a minimal `initialize` request (just to make codelldb send something)
5. Reads ONE complete DAP message from the TCP stream
6. Parses the `Content-Length: N\r\n\r\n` framing
7. Pretty-prints the JSON body
8. Exits

You'll see a real DAP response on screen. ~80 lines.

## Why

The wire format is the foundation. Every other milestone reads or writes framed JSON. If M1 is solid, M2+ become trivial. If it's hacky, M2+ inherit the hack.

## How

### Step 1 — Parse the port from stderr

```rust
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

async fn spawn_codelldb_and_get_port() -> anyhow::Result<(tokio::process::Child, u16)> {
    let mut child = Command::new("codelldb")
        .arg("--port").arg("0")
        .env("RUST_LOG", "debug")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stderr = child.stderr.take().expect("stderr piped");
    let mut lines = BufReader::new(stderr).lines();

    while let Some(line) = lines.next_line().await? {
        // Modern codelldb (20.x): "Listening on HOST:PORT". Older: "Listening on port NNNNN".
        // See docs/issues/0002-codelldb-version-drift-rust-log.md.
        let Some((_, rest)) = line.split_once("Listening on ") else {
            continue;
        };
        let port_str = rest
            .strip_prefix("port ")
            .unwrap_or_else(|| rest.rsplit(':').next().unwrap_or(rest));
        let port: u16 = port_str.trim().parse()?;
        // Drain the rest of stderr in the background so codelldb doesn't block on a full pipe.
        tokio::spawn(async move {
            while let Ok(Some(_)) = lines.next_line().await {}
        });
        return Ok((child, port));
    }
    anyhow::bail!("codelldb didn't print 'Listening on' line");
}
```

### Step 2 — Connect via TCP, send `initialize`, read one message

```rust
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (mut child, port) = spawn_codelldb_and_get_port().await?;
    println!("codelldb listening on port {port}");

    // Connect TCP.
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;

    // Send initialize.
    let body = serde_json::json!({
        "seq": 1,
        "type": "request",
        "command": "initialize",
        "arguments": {
            "clientID": "lazydap",
            "clientName": "lazydap",
            "adapterID": "lldb",
            "linesStartAt1": true,
            "columnsStartAt1": true,
            "pathFormat": "path",
        },
    });
    let body_bytes = serde_json::to_vec(&body)?;
    let header = format!("Content-Length: {}\r\n\r\n", body_bytes.len());
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body_bytes).await?;
    stream.flush().await?;

    // Read the response: parse Content-Length header, then read body.
    let mut reader = tokio::io::BufReader::new(&mut stream);
    let mut header_buf = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        header_buf.clear();
        let n = AsyncBufReadExt::read_line(&mut reader, &mut header_buf).await?;
        if n == 0 {
            anyhow::bail!("EOF before headers");
        }
        let trimmed = header_buf.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break; // end of headers
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse()?);
        }
    }

    let len = content_length.ok_or_else(|| anyhow::anyhow!("no Content-Length header"))?;
    let mut body_bytes = vec![0u8; len];
    reader.read_exact(&mut body_bytes).await?;

    let body: serde_json::Value = serde_json::from_slice(&body_bytes)?;
    println!("---- DAP response ----");
    println!("{}", serde_json::to_string_pretty(&body)?);

    child.kill().await?;
    Ok(())
}
```

### Step 3 — Run

```bash
cargo run --example m1_read_one_message
```

Expected output:

```
codelldb listening on port 53274
---- DAP response ----
{
  "type": "response",
  "request_seq": 1,
  "success": true,
  "command": "initialize",
  "body": {
    "supportsConfigurationDoneRequest": true,
    "supportsFunctionBreakpoints": true,
    ...
  },
  "seq": 1
}
```

You're reading real DAP. Big win.

## Success criteria

- Example binary runs and prints the `initialize` response from codelldb.
- The response includes `"success": true` and a `body` with capability flags.
- Example exits cleanly (no zombie codelldb processes — verify with `pgrep codelldb` after).

## Files

- `crates/daemon/examples/m1_read_one_message.rs` (new)
- `crates/daemon/Cargo.toml` — add the `[[example]]` entry

## Verify

```bash
cargo run --example m1_read_one_message
# Confirm pretty-printed JSON response with "command": "initialize", "success": true.

pgrep codelldb
# Should print nothing — process cleaned up.
```

## Depends on

- [`M00-hello-adapter`](M00-hello-adapter.md) — basic adapter spawning.

## Notes

- **Don't extract this into a `crates/dap/` library yet.** That's M5's job. Keep it inline and explicit while you're learning.
- **Don't worry about the `seq` field carefully yet.** We send `seq: 1`. Adapters sometimes use it loosely. M2 will tighten this up.
- **The "drain stderr in a task" pattern is critical.** If you don't drain stderr, codelldb's stderr buffer fills up and the adapter blocks. This bites silently — adapter just stops responding.
- **`BufReader` for line-reading headers is fine.** The body is read with `read_exact` after.
