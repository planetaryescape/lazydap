# M0 — Hello, adapter

## What

A small example binary `examples/m0_hello_adapter.rs` that:

1. Spawns `codelldb` as a child process.
2. Reads the first chunk of bytes from its stdout.
3. Prints them raw.
4. Exits.

That's it. ~30 lines. One afternoon.

## Why

Every later milestone depends on being able to spawn the adapter and read its output. Doing this in isolation, with nothing else going on, lets you confirm:

- codelldb is installed and findable
- you understand `tokio::process::Command`
- you know what the adapter's first output looks like

Skipping straight to "parse messages" introduces three unknowns at once (process spawning, stdio plumbing, framing). M0 isolates the first.

## How

### Step 1 — Verify codelldb is installed

```bash
which codelldb
# Expected: a path. If not, install via Mason / VS Code extension / `cargo install codelldb` / brew.

codelldb --help
# Expected: usage info. Confirms it runs.
```

If you don't have it, install via `~/.local/share/nvim/mason/bin/codelldb` (Mason) or `~/.vscode/extensions/vadimcn.vscode-lldb-*/adapter/codelldb`.

### Step 2 — Add the example binary

`examples/m0_hello_adapter.rs`:

```rust
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use std::process::Stdio;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // codelldb requires --port 0 (TCP, not stdio). On port 0 it picks a free port
    // and prints "Listening on port N" to stderr.
    let mut child = Command::new("codelldb")
        .arg("--port").arg("0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    // Read first chunk from stderr (where codelldb announces the port).
    let mut stderr = child.stderr.take().expect("stderr piped");
    let mut buf = [0u8; 256];
    let n = stderr.read(&mut buf).await?;
    let s = std::str::from_utf8(&buf[..n])?;
    println!("first stderr chunk: {s:?}");

    // Don't bother with stdout — it's only used after we connect via TCP.
    // Phase A's M1 handles the actual TCP connection.

    child.kill().await?;
    Ok(())
}
```

`Cargo.toml` (root, add to `[workspace.dependencies]` if not already):

```toml
anyhow = "1"
```

`crates/daemon/Cargo.toml` already has `tokio` workspace-true. Examples can use them directly.

To run examples from a workspace, add to the daemon crate's `Cargo.toml`:

```toml
[[example]]
name = "m0_hello_adapter"
path = "../../examples/m0_hello_adapter.rs"
```

(Or put the example directly in `crates/daemon/examples/m0_hello_adapter.rs`. Either works; pick the latter for less boilerplate. Updating this milestone — use `crates/daemon/examples/`.)

### Step 3 — Run

```bash
cargo run --example m0_hello_adapter
```

Expected output (something like):

```
first stderr chunk: "Listening on port 53274\n"
```

Different port number every run. The point is: codelldb spoke to us.

### Step 4 — Notice the surprise

codelldb is **not** stdio-DAP. It's a TCP server. `--port 0` means "pick a free port, tell me what it was on stderr." The actual DAP traffic happens on TCP after we connect to that port.

This is the first adapter quirk. Encode it later in `crates/adapter-codelldb/`.

## Success criteria

- `cargo run --example m0_hello_adapter` runs, prints a "Listening on port N" line, exits cleanly.
- Run it 3 times: port number changes each time.
- Comment in the source explains why we read stderr, not stdout.

## Files

- `crates/daemon/examples/m0_hello_adapter.rs` (new)

## Verify

```bash
cargo run --example m0_hello_adapter
# Expected output: a line containing "Listening on port" with a port number.

# Repeat — should still work, different port:
cargo run --example m0_hello_adapter
cargo run --example m0_hello_adapter
```

If the port doesn't print, check:

- `which codelldb` returns a path
- `codelldb --port 0` run manually prints to stderr (not stdout)

## Depends on

- [`00-workspace-setup`](../00-workspace-setup.md) — workspace exists.
- codelldb installed somewhere on PATH.

## Notes

- **Don't try to connect via TCP yet.** That's M1.
- **Don't parse the port number.** Just observe the output. M1 parses it.
- **`kill_on_drop(true)`** is important — without it, codelldb processes leak if the example errors out.
- **Don't add error handling beyond `?`.** Phase A is exploratory. Tighten in Phase B.
