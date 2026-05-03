# codelldb quirks reference

Version-drift, install footguns, and runtime quirks of `codelldb` that affect lazydap. Add to this when you discover new ones; remove rows that get fixed upstream.

This doc is the canonical place for "this codelldb thing surprised us." Cross-link to it from milestone docs, book chapters, and the `crates/adapter-codelldb/` source once it lands.

## Quirk index

| # | Quirk | First hit by | Tested against |
|---|---|---|---|
| 1 | [Symlink install puts liblldb path off by one directory](#1-symlink-install-breaks-liblldb-resolution) | M0-1 / Chapter 04 | codelldb 1.12.2 |
| 2 | [Silent on stderr without `RUST_LOG=debug`](#2-silent-on-stderr-without-rust_logdebug) | M0-1 / Chapter 04 | codelldb 1.12.2 |
| 3 | [Speaks DAP only over TCP, not stdio](#3-tcp-only-not-stdio) | M0 milestone doc | codelldb 1.x |
| 4 | [`--version` flag is not recognised; use `--help`](#4---version-not-supported) | CONTRIBUTING.md authoring | codelldb 1.x |

---

## 1. Symlink install breaks liblldb resolution

### Symptom

Running `codelldb --port 0` (or any invocation) panics on startup:

```
thread 'main' panicked at src/codelldb/bin/main.rs:56:49:
called Result::unwrap() on an Err value:
"dlopen(/Users/<user>/.local/lldb/lib/liblldb.dylib, ...) (no such file)"
```

The path codelldb is searching (`~/.local/lldb/lib/liblldb.dylib`) doesn't exist. The actual `liblldb.dylib` is somewhere else, typically `~/.local/opt/codelldb/extension/lldb/lib/liblldb.dylib`.

### Root cause

codelldb computes the location of `liblldb.dylib` at runtime by:

1. Reading `argv[0]` (the path the process was invoked as)
2. Stripping the basename → directory of the invoker
3. Appending `../lldb/lib/liblldb.dylib`
4. `dlopen`ing that path

When invoked through a **symlink** at `~/.local/bin/codelldb` (which is what CONTRIBUTING.md historically recommended), `argv[0]` resolves to the symlink path itself on macOS. The relative-path computation gives `~/.local/lldb/lib/liblldb.dylib`, which is wrong.

When invoked via the **real binary path** (e.g., directly running `~/.local/opt/codelldb/extension/adapter/codelldb`), the relative path resolves correctly to `~/.local/opt/codelldb/extension/lldb/lib/liblldb.dylib`.

Verified via `otool -L ~/.local/opt/codelldb/extension/adapter/codelldb`: the binary doesn't have `liblldb.dylib` baked into its install names. The lookup is purely runtime via `dlopen` + path computation, not via the dynamic linker's `@executable_path`/`@rpath` system.

### Fix: wrapper script (NOT symlink)

Replace the symlink with a wrapper bash script that exec's the real binary using its absolute path:

```bash
cat > ~/.local/bin/codelldb <<'WRAPPER_EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
WRAPPER_EOF
chmod +x ~/.local/bin/codelldb
```

The wrapper sets `argv[0]` to the absolute path of the real binary, so codelldb's path computation lands at the correct sibling `lldb/lib/liblldb.dylib`.

This is the same pattern the Mason install of codelldb uses (`~/.local/share/nvim/mason/bin/codelldb` is a one-line bash wrapper).

### Pain anchor (why this exists)

C++ programs often resolve their dynamic libraries this way. The convention pre-dates `@rpath` linker tokens by decades. When you symlink them onto PATH and they break, this is why. Languages with native package managers (cargo, npm, go) sidestep this by static-linking or vendoring; codelldb hits it because LLDB itself is C++.

### Cross-references

- Issue: [`docs/issues/0001-codelldb-symlink-install-broken.md`](../issues/0001-codelldb-symlink-install-broken.md)
- Chapter: [`docs/book/04-hello-adapter.md`](../book/04-hello-adapter.md)
- Install instructions: [`CONTRIBUTING.md`](../../CONTRIBUTING.md) (codelldb section)

---

## 2. Silent on stderr without `RUST_LOG=debug`

### Symptom

Spawning codelldb with `--port 0` and reading from its stderr blocks indefinitely. No bytes ever appear. The process *is* running and *has* opened a TCP listener (verifiable via `lsof -p <pid>`). It just emits no console output.

### Root cause

Modern codelldb (≥ v1.10 confirmed; possibly earlier) uses Rust's `tracing` / `env_logger` ecosystem for all console output. Per `tracing` convention, no log lines are emitted unless the `RUST_LOG` env var sets a sufficient log level for the relevant tracing target.

Specifically:
- The "Loaded liblldb" message is at `INFO` level.
- The "Listening on port N" message is at `DEBUG` level.

So `RUST_LOG=info` gets you the load message; `RUST_LOG=debug` gets both. With no `RUST_LOG` set: silent.

### Fix: pass `RUST_LOG=debug` when spawning

```rust
let mut child = Command::new("codelldb")
    .arg("--port").arg("0")
    .env("RUST_LOG", "debug")     // <- this line
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()?;
```

### Verification

```bash
# Without RUST_LOG: silent
~/.local/bin/codelldb --port 0 > /tmp/o 2> /tmp/e &
sleep 1; kill %1; wait %1 2>/dev/null
wc -c /tmp/o /tmp/e
#       0 /tmp/o
#       0 /tmp/e

# With RUST_LOG=debug: 200+ bytes
RUST_LOG=debug ~/.local/bin/codelldb --port 0 > /tmp/o 2> /tmp/e &
sleep 1; kill %1; wait %1 2>/dev/null
cat /tmp/e
# [INFO  codelldb] Loaded "/Users/.../liblldb.dylib", version="lldb version 20.1.4-codelldb"
# [DEBUG codelldb] Listening on 127.0.0.1:NNNNN
```

### Why this surfaces in lazydap

For lazydap to know which port codelldb is listening on (so it can connect via TCP), it has to read the port number from codelldb's startup output. If codelldb is silent, lazydap can't determine the port.

The codelldb adapter implementation in `crates/adapter-codelldb/` (when M5+ lands) must set `RUST_LOG=debug` for spawned codelldb processes. Document it inline in the adapter code with a reference to this quirk.

### Cross-references

- Issue: [`docs/issues/0002-codelldb-version-drift-rust-log.md`](../issues/0002-codelldb-version-drift-rust-log.md)
- Chapter: [`docs/book/04-hello-adapter.md`](../book/04-hello-adapter.md)
- Milestone: [`docs/implementation/tasks/M00-hello-adapter.md`](../implementation/tasks/M00-hello-adapter.md)

---

## 3. TCP-only, not stdio

### Symptom

You might expect a DAP adapter to speak DAP over its stdin/stdout, like a language server. codelldb does *not*. It opens a TCP server.

### How

- `codelldb --port N`: opens a TCP listener on `127.0.0.1:N` and waits for one connection.
- `codelldb --port 0`: picks a free port (the OS assigns), reports it via the listening message (gated by `RUST_LOG`, see Quirk 2), waits for one connection.
- `codelldb --connect HOST:PORT`: connects *out* to a server that's listening (rare; usually for special remote-debug topologies).

The DAP protocol traffic flows over the TCP socket once a client connects. The child's stdio (stdout/stderr) is used only for log output, not protocol bytes.

### Implication for lazydap

The codelldb adapter has to:
1. Spawn codelldb with `--port 0`
2. Read codelldb's stderr to discover the port
3. Open a TCP socket to `127.0.0.1:<port>`
4. Speak DAP over the socket

Other adapters do this differently. `debugpy-adapter` speaks DAP over stdio directly. The lazydap adapter abstraction (`DebugAdapter` trait) hides this: each adapter crate handles its own transport setup.

### Cross-references

- Milestone: [`docs/implementation/tasks/M00-hello-adapter.md`](../implementation/tasks/M00-hello-adapter.md)
- Future milestone: M1 (TCP connect + first message read)
- DAP protocol cheatsheet: [`docs/reference/dap-protocol-cheatsheet.md`](dap-protocol-cheatsheet.md)

---

## 4. `--version` not supported

### Symptom

```bash
codelldb --version
# error: unexpected argument '--version' found
```

### Root cause

codelldb's CLI parser doesn't include a `--version` flag. The list of flags shows up under `--help`:

```
Options:
      --liblldb <LIBLLDB>
      --port <PORT>
      --connect <CONNECT>
      --auth-token <AUTH_TOKEN>
      --multi-session
      --settings <SETTINGS>
  -h, --help
```

To check the version: read it from the `--help` output is unhelpful (no version there either), or check `package.json` inside the `~/.local/opt/codelldb/extension/` directory after install.

### Fix

If you need to detect codelldb version programmatically (e.g., for adapter compat checks), parse the version field of:

```bash
cat ~/.local/opt/codelldb/extension/package.json | grep '"version"'
```

Or use the lldb version embedded in the load log line under `RUST_LOG=info`:

```
[INFO  codelldb] Loaded "...liblldb.dylib", version="lldb version 20.1.4-codelldb"
```

### Cross-references

- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — install verification step uses `codelldb --help`

---

## Adding a new quirk

When you discover a new codelldb behaviour worth documenting:

1. Add a row to the index table at the top.
2. Add a section below following the structure of existing entries: **Symptom**, **Root cause**, **Fix**, **Cross-references**.
3. Cross-link to a `docs/issues/` entry if it represents an upstream/contributor problem worth tracking, and to any chapters or milestones that hit it.
4. Update the "Tested against" version when you verify against a newer release.

The bar for inclusion: any quirk that takes more than 10 minutes to figure out the first time, or has bitten the project more than once. (This mirrors AGENTS.md's general rule: "Add to `docs/reference/` whenever a question takes more than 10 minutes to answer for the second time.")
