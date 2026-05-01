# M5 — IPC protocol + daemon binary

## What

1. Define lazydap IPC protocol types in `crates/protocol/`.
2. Build the daemon: spawns adapters, holds session, listens on Unix socket, dispatches IPC requests.
3. First subcommand `lazydap launch <program>` works end-to-end via daemon.

By the end, `lazydap launch ./examples/c-hello/build/hello --stop-on-entry --format json` returns a JSON `Launched` response.

## Why

Phase A was scripts. M5 is the first lazydap. Everything from here builds on this skeleton.

## How

### Step 1 — `crates/protocol`

```bash
mkdir -p crates/protocol/src
```

`crates/protocol/Cargo.toml`:

```toml
[package]
name = "lazydap-protocol"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
lazydap-core = { path = "../core" }
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
```

`crates/protocol/src/lib.rs`: define `IpcMessage`, `Request`, `Response`, `Event`, `IpcError` per [`/docs/blueprint/04-protocol.md`](../../blueprint/04-protocol.md). For M5, only the variants we need:

```rust
pub const LAZYDAP_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcMessage {
    pub version: u32,
    pub id: u64,
    pub payload: IpcPayload,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum IpcPayload {
    Request(Request),
    Response(Response),
    Event(Event),
    Error(IpcError),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Ping,
    Launch(LaunchRequest),
    Disconnect { session_id: SessionId, terminate: bool },
}

// ... LaunchRequest, Response, Event, IpcError, etc.
```

### Step 2 — Codec

`crates/protocol/src/codec.rs` — length-prefixed JSON framing:

```rust
pub async fn write_message<W: AsyncWrite + Unpin>(w: &mut W, msg: &IpcMessage) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    let len = (body.len() as u32).to_be_bytes();
    w.write_all(&len).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

pub async fn read_message<R: AsyncRead + Unpin>(r: &mut R) -> io::Result<IpcMessage> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(serde_json::from_slice(&body)?)
}
```

### Step 3 — Daemon server

`crates/daemon/src/server.rs`: `tokio::net::UnixListener` accept loop. For each accepted client, `tokio::spawn(handle_client(stream, daemon_state))`. Handler reads `IpcMessage`s, dispatches by `Request` variant, writes responses.

For M5: only `Ping` and `Launch` are real. Everything else returns `Error::Unsupported`.

### Step 4 — Daemon state

`crates/daemon/src/state.rs`:

```rust
pub struct DaemonState {
    pub sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
    pub event_tx: broadcast::Sender<Event>,
}

pub struct Session {
    pub id: SessionId,
    pub transport: Mutex<DapTransport>,
    pub state: RwLock<SessionState>,
    // ...
}
```

For M5, single-session enforcement: handlers reject `Launch` if `sessions` is non-empty.

### Step 5 — Daemon binary

`crates/daemon/src/main.rs` becomes:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Daemon { foreground }) => {
            run_daemon(foreground).await?;
        }
        Some(Commands::Launch(args)) => {
            run_subcommand_launch(args).await?;
        }
        Some(Commands::Status) => {
            run_subcommand_status().await?;
        }
        None => {
            // bare lazydap → eventually TUI; for M5, print help.
            print_help();
        }
    }
    Ok(())
}
```

Subcommand handlers do: `ensure_daemon_running()`, connect to socket, send `Request`, format response, exit.

### Step 6 — `ensure_daemon_running`

`crates/daemon/src/auto_spawn.rs`:

```rust
pub async fn ensure_daemon_running() -> Result<()> {
    let socket = socket_path()?;
    if probe_daemon(&socket).await.is_ok() {
        return Ok(());
    }
    fork_daemon().await?;
    // Probe with retry until socket appears.
    for _ in 0..20 {
        if probe_daemon(&socket).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("daemon failed to start within 2s".into())
}
```

`fork_daemon` = spawn `std::env::current_exe()` with `daemon` subcommand, detach, return.

### Step 7 — Run end-to-end

```bash
gcc -g -O0 examples/c-hello/main.c -o examples/c-hello/build/hello
cargo install --path crates/daemon
lazydap launch ./examples/c-hello/build/hello --stop-on-entry --format json
```

Expected JSON output (something like):

```json
{
  "session_id": "01ABC...",
  "state": "Paused",
  "reason": "Entry",
  "frame": { "name": "main", "source": "main.c", "line": 4 }
}
```

## Success criteria

- `lazydap launch <prog>` returns a JSON `Launched` response.
- `lazydap status --format json` returns the active session.
- `lazydap disconnect` ends the session cleanly.
- Daemon auto-spawns on first command.
- A second `lazydap launch` while a session exists returns `Error::SessionAlreadyActive`.
- No leaked daemon or codelldb processes after `lazydap shutdown`.

## Files

- `crates/protocol/Cargo.toml`, `src/lib.rs`, `src/codec.rs` (new)
- `crates/daemon/src/server.rs`, `state.rs`, `auto_spawn.rs`, `cli/`, `handlers/` (new)
- Workspace `Cargo.toml` updated

## Verify

```bash
cargo build --workspace
cargo install --path crates/daemon
lazydap launch ./examples/c-hello/build/hello --stop-on-entry --format json | jq
lazydap status --format json | jq
lazydap disconnect
pgrep -f "lazydap daemon" || echo "(daemon stopped — for our purposes, OK)"
```

## Depends on

- [`M04-pause-on-breakpoint`](M04-pause-on-breakpoint.md) — full DAP comprehension.

## Notes

- **Single-session enforcement here.** Per [`/docs/blueprint/15-decision-log.md`](../../blueprint/15-decision-log.md) D007. The protocol carries `session_id`; the daemon rejects `Launch` if non-empty.
- **`SessionId` is already in every IPC message.** Future-proof.
- **Don't add subcommands beyond `launch`/`status`/`disconnect`/`daemon`/`shutdown` here.** That's M6.
- **PID file at `{data_dir}/daemon.pid`**, socket at `{runtime_dir}/lazydap-{instance}.sock`. Define the helpers in `crates/config/src/paths.rs` (create the crate now if not yet).
- **Connect to broadcast channel for events.** When the adapter emits `stopped`, the daemon broadcasts via `event_tx`. Subscribed clients (none in M5; TUI in M11) receive.
