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

---

## Completed 2026-07-30

`lazydap launch`, `status`, `disconnect`, `shutdown` and `daemon` all work against a real daemon over a real Unix socket. Verified live against codelldb 1.12.2: launch with `--stop-on-entry` returns a `Launched` response, `status` shows the paused session with its buffered output count, a second `launch` is refused with `SessionAlreadyActive`, `disconnect` leaves no codelldb or debuggee process behind, and `shutdown` removes both the socket and the PID file.

### Decisions taken (all recorded in [`15-decision-log.md`](../../blueprint/15-decision-log.md))

- **D024–D027** resolve the open questions O01–O04: project-root detection walks one marker tier at a time; `doctor` is stdout-only (M6); adapter discovery is config > managed dir > `PATH`, with only `PATH` implemented here; the skill ships as a sibling ZIP (M7).
- **D028** — framing is `tokio_util`'s `LengthDelimitedCodec` (4-byte prefix, 16 MiB cap) rather than a hand-rolled `read_exact` pair. Adds `tokio-util` and `bytes`.
- **D029** — the adapter seam is the `crates/daemon/src/adapter/` module boundary, not a `DebugAdapter` trait. One implementor is not an abstraction; M18 promotes it when debugpy gives the interface a second example. No `crates/adapter-codelldb`.
- **D030** — `SessionId` is a UUID v4, not a ULID. Nothing sorts session ids, so sortability was not worth a dependency.

### Deviations from the plan above

- **The binary is `lazydap`, not `lazydap-daemon`** (D002). `crates/daemon` grew a `[lib]` and a `[[bin]] name = "lazydap"`; `main.rs` is six lines calling `run_cli`, so integration tests drive the same code the binary does.
- **`crates/dap` gained `DapTransport::split`.** The read pump has to own reads exclusively — `read_incoming` is not cancellation-safe, which M3 recorded as a follow-up — and that is impossible while one struct owns both halves. The transport now splits the `TcpStream` at spawn time into a `DapReader` and a `DapWriter` (which also carries the child process, because killing it is a control action). Purely additive; every existing caller and example is untouched.
- **`Request::Disconnect` takes a required `session_id`**, per D007, rather than an optional one meaning "the active session". The CLI resolves the active id with a `Status` call first, so `lazydap disconnect` still takes no arguments.
- **`Request::Subscribe` exists but answers `Unsupported`.** It gives the version handshake a real "this build cannot do that" path to exercise, and settles the wire shape before M11 needs it.
- **`Request::Shutdown` was added** to the Diagnostics bucket — `lazydap shutdown` needs it, and the blueprint's request list did not have one. No new bucket.
- **`--format` accepts `table` and `json` only.** Nothing M5 prints is a list, so `jsonl`/`csv`/`ids` would be three spellings of the same single row. They land with M6.
- **Runtime directory is `/tmp/lazydap-$UID`, not `std::env::temp_dir()`.** On macOS the latter is a ~50-character path, and a Unix socket path only gets ~104 bytes; combined with a long project name it overran. Caught by the length check, which stayed in as a guard.
- **PID and log files are instance-scoped** (`lazydap-{instance}.pid`, not `daemon.pid`), so two projects' daemons cannot overwrite each other's.
- **D015 fixed on the way past:** the old `main.rs` used `println!` with no subscriber. Tracing now initialises before anything else — `info` for the daemon, `warn` for subcommands so stdout stays a clean JSON pipeline.

### Follow-ups discovered

- **An ended session still occupies the slot.** After the debuggee exits, `launch` refuses until `disconnect` runs. That matches the lifecycle doc but is a papercut; M6 should decide whether a dead session is auto-reaped.
- **No `Subscribe` means no live event delivery.** Events are buffered per session (1000, drop-oldest) and broadcast, but nothing consumes the broadcast until M11.
- **Requests on one connection are handled sequentially.** Fine for a CLI that sends one and waits; M6's `--wait` should confirm a long request cannot starve a `status` on the same connection.
- **`env` is always empty in `LaunchRequest`.** The protocol carries it, the CLI has no `--env` flag yet.
- **Adapter discovery is `PATH`-only** (D026); the config and managed-directory tiers are M15/M18.
- **The spawn lock is an `O_EXCL` lock file**, with a 30-second staleness cutoff, rather than `flock`. It avoids a dependency and is correct on a local filesystem; a networked `/tmp` would want the real thing.
