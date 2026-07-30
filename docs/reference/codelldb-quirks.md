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
| 5 | [Hangs at `_dyld_start` after a macOS update](#5-hangs-at-_dyld_start-after-a-macos-update-stale-gatekeeper-inode-cache) | Ship-mode Wave 0 (2026-07-30) | codelldb 1.12.2 / Darwin 25.5.0 |
| 6 | [`--stop-on-entry` stops with reason `exception`, not `entry`](#6---stop-on-entry-reports-reason-exception-not-entry-on-macos) | M5 (2026-07-30) | codelldb 1.12.2 / Darwin 25.5.0 |
| 7 | [`evaluate` with context `repl` runs an LLDB *command*, not an expression](#7-evaluate-with-context-repl-runs-an-lldb-command-not-an-expression) | M6 (2026-07-30) | codelldb 1.12.2 / Darwin 25.5.0 |
| 8 | [Breakpoints never bind for a debuggee under `/tmp` on macOS](#8-breakpoints-never-bind-for-a-debuggee-under-tmp-on-macos) | Ship-mode Wave 5 (2026-07-30) | codelldb 1.12.2 / Darwin 25.5.0 |

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

## 5. Hangs at `_dyld_start` after a macOS update (stale Gatekeeper inode cache)

### Symptom

Every invocation — even `codelldb --help` — hangs forever, producing zero output on stdout
and stderr. `--port 0` stays alive but never listens (nothing in `lsof -i TCP`) and never
prints "Listening on". `sample <pid>` shows 100% of samples at `_dyld_start` — the process
never reaches `main()`. `codesign -v` passes; no `com.apple.quarantine` xattr is present;
nothing appears in the unified log from `syspolicyd`/`amfid`.

To lazydap's transport this presents as `NoPortFromAdapter` (or an infinite hang in the
port-scan loop if no timeout wraps it) — indistinguishable from quirk 1's crashed-adapter
symptom until you run `codelldb --help` by hand.

### Root cause

macOS keeps a per-inode security-evaluation record for launched binaries. An OS update can
leave that record stale/wedged, after which dyld blocks indefinitely during pre-main launch
for that specific file. The proof: copying the binary to a new path (new inode) makes the
copy run instantly, while the original at its old path still hangs — same bytes, same
signature, same (absent) quarantine state. Hit 2026-07-30 on Darwin 25.5.0 with codelldb
1.12.2, on **both** the VS Code extension copy and the Mason-managed copy, months after
both had worked.

### Fix

Re-copy the install so every file gets a fresh inode, then repoint the wrapper:

```bash
rm -rf ~/.local/opt/codelldb
cp -R <source>/extension ~/.local/opt/codelldb/extension   # e.g. the Mason package dir
# wrapper (quirk 1) at ~/.local/bin/codelldb already points at this path
timeout 8 codelldb --help   # must print usage and exit 0
```

Verify end-to-end with `cargo run --example m2_initialize`.

### Cross-references

- Quirk 1 — the wrapper-script install this fix rebuilds
- [`docs/issues/0001-codelldb-symlink-install-broken.md`](../issues/0001-codelldb-symlink-install-broken.md) — the adjacent install footgun

---

## 6. `--stop-on-entry` reports reason `exception`, not `entry`, on macOS

### Symptom

Launching with `stopOnEntry: true` does stop the debuggee at its entry point, exactly as
asked. But the `stopped` event that reports it says the reason was an **exception**:

```json
{"seq":12,"type":"event","event":"stopped",
 "body":{"reason":"exception","description":"signal SIGSTOP","threadId":26187878,
         "allThreadsStopped":true}}
```

So `lazydap launch --stop-on-entry --format json` returns `"reason": "exception"` where a
reader of the DAP specification would expect `"reason": "entry"`. The pause itself is
correct — the program is stopped before `main` — only the label is surprising.

First seen during M5 in a sandboxed shell, where it looked like the sandbox had trapped the
debuggee. It is not sandbox-related: the same event was captured unsandboxed on a normal
terminal.

### Root cause

codelldb implements entry-stop by letting the process start and immediately sending it
`SIGSTOP`, rather than by using a dedicated entry breakpoint. LLDB classifies a stop caused
by a signal as an exception-class stop, and codelldb reports the stop reason it gets from
LLDB. The `description` field is the giveaway: `"signal SIGSTOP"`.

DAP's `entry` reason is therefore not used by this adapter on macOS at all.

### Implication for lazydap

lazydap passes the adapter's reason through unchanged, which is why `PauseReason`
serialises as a bare string and keeps unmodelled reasons verbatim — a reason we did not
anticipate reaches the client as the reason the adapter gave, rather than being coerced into
something tidier and wrong.

**Whether to normalise this is an M6 decision**, and it belongs with the `--wait` design
because `--wait`'s response is where agents actually read stop reasons. The options:

1. Leave it. Honest, and `description` carries the detail; but every agent has to learn that
   "exception" sometimes means "entry".
2. Map it in the adapter module: a `stopped` with reason `exception` and description
   `signal SIGSTOP`, arriving while a `stop_on_entry` launch is still settling, becomes
   `PauseReason::Entry`. Narrow enough to be safe, and puts adapter-specific knowledge in
   the adapter seam where it belongs. Costs a lie-by-omission unless the raw reason is kept
   alongside.
3. Report both — a normalised `reason` plus the adapter's own `raw_reason`.

Option 3 fits "JSON output is a product feature" best, at the cost of one more field.

### Verification

```bash
lazydap launch ./examples/c-hello/build/hello --stop-on-entry --format json
# "state": "paused", "reason": "exception"
```

Raw event capture: set `LAZYDAP_LOG=dap.recv.event=debug` and read the daemon log at
`{data_dir}/lazydap-{instance}.log`.

### Cross-references

- Milestone: [`docs/implementation/tasks/M05-ipc-protocol-daemon.md`](../implementation/tasks/M05-ipc-protocol-daemon.md) — follow-ups
- [`docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md) — where the `--wait` reason semantics get decided

---

## 7. `evaluate` with context `repl` runs an LLDB command, not an expression

### Symptom

`lazydap eval "x"` on a paused C program, where `x` is an `int` holding `5`, fails:

```
error: memory read takes a start address expression with an optional end address expression.
warning: Expressions should be quoted if they contain spaces or other special characters.
```

And `lazydap eval "x + 2"` fails differently:

```
error: invalid start address expression.
error: address expression "+" evaluation failed
```

Both are LLDB *command* errors. `x` is LLDB's built-in alias for `memory read`, so asking
for a variable called `x` reads memory instead — and reads it badly, because `x` on its own
is not a valid address.

### Root cause

DAP's `evaluate` request takes a `context` field: `repl`, `watch`, `hover`, or absent.
codelldb reads `repl` literally — "this is a line the user typed at the debug console" — and
sends it to LLDB's command interpreter rather than its expression evaluator. The console
announces this at launch, in an `output` event nobody reads:

```
Console is in 'commands' mode, prefix expressions with '?'.
```

`watch` and `hover` both evaluate the string as an expression in the debuggee's language,
which is what "evaluate this expression" normally means.

Captured live on 2026-07-30 against the `examples/c-hello` debuggee, paused at `main.c:19`:

| `--context` | `eval "x"` | `eval "x + 2"` |
|---|---|---|
| `repl` | error (memory read) | error (address expression) |
| `watch` | `5` (`int`) | `7` (`long long`) |
| `hover` | `5` (`int`) | `7` (`long long`) |

### Fix

`lazydap eval` defaults to `--context watch`, and `EvalContext::default()` in
`lazydap-core` matches it. `--context repl` is still there for callers who genuinely want
to run an adapter command — with codelldb that is a real feature, not a mistake — but it is
no longer what you get by accident. See D034.

The alternative, prefixing `repl` expressions with `?` as the console suggests, was
rejected: it is codelldb-specific syntax leaking into what a caller types, and it would
make `lazydap eval` mean something different per adapter.

### Cross-references

- [`docs/blueprint/15-decision-log.md`](../blueprint/15-decision-log.md) — D034
- Milestone: [`docs/implementation/tasks/M06-cli-subcommands.md`](../implementation/tasks/M06-cli-subcommands.md)

---

## 8. Breakpoints never bind for a debuggee under `/tmp` on macOS

### Symptom

You put a scratch program in `/tmp`, set a breakpoint on a line you know is reached, and the
program runs straight to completion without stopping. `break` looks like it worked:

```json
{
  "action": "added",
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6,
      "source": "/private/tmp/lazydap-demo/hello.c", "verified": false }
  ]
}
```

`launch` is where it says so, in a `message` that reads like it is reporting a success:

```json
{
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6,
      "message": "Breakpoint at /private/tmp/lazydap-demo/hello.c:6 could not be resolved, but a valid location was found at /tmp/lazydap-demo/hello.c:6",
      "source": "/private/tmp/lazydap-demo/hello.c", "verified": false }
  ],
  "state": "running"
}
```

Then `continue --wait` returns `"state": "exited"`, `"exit_code": 0`, with the program's whole
output in `captured_output` and `"hit_breakpoint_ids": []`.

The two paths in that message differ only by the `/private` prefix, which is easy to read past.
`verified: false` is the real signal, and quirk 1's crashed-adapter symptom aside, this is the
main way a breakpoint silently does nothing.

### Root cause

On macOS `/tmp` is a symlink to `/private/tmp`. Two components disagree about which spelling is
the file's name:

1. **lazydap canonicalises.** `resolve_source` calls `Path::canonicalize`, which resolves
   symlinks, so a breakpoint set from `/tmp/lazydap-demo` is stored and sent as
   `/private/tmp/lazydap-demo/hello.c` (`crates/daemon/src/commands/mod.rs:129`). It does this so
   the daemon and the adapter agree on a path regardless of either one's working directory, and
   so a typo fails immediately rather than as a silent `verified: false` later.
2. **The compiler doesn't.** DWARF records the path as it was typed on the command line. Build
   with `gcc /tmp/lazydap-demo/hello.c` and the debug info says `/tmp/...`.
3. **codelldb matches the literal string.** It compares the `setBreakpoints` source path against
   the compilation unit's path textually, without resolving either. `/private/tmp/...` and
   `/tmp/...` are different strings, so nothing matches.

Neither canonicalising nor not canonicalising is wrong on its own. Disagreeing is.

codelldb does find the line — that is what "a valid location was found at /tmp/..." means — and
then declines to bind to it, because the location it found is not in the file it was asked about.
It reports this at `verified: false` rather than as an error, which is correct per DAP and
unhelpful in practice.

This is not specific to `/tmp`. Any symlinked directory on the path to a source file will do it;
`/tmp` is just the one everybody reaches for when writing a scratch program. `/var` and
`/etc` are symlinks on macOS too.

### Fix

**Keep debuggees out of `/tmp`.** A directory under `$HOME` has no symlink on the way to it and
the problem does not arise. This is the whole fix for scratch work and what `CONTRIBUTING.md`
tells contributors.

**If you must work under a symlinked path**, make the two spellings agree by compiling with the
resolved path, so the debug info matches what lazydap will send:

```bash
gcc -g -O0 /private/tmp/scratch/hello.c -o /private/tmp/scratch/hello
```

**Check `verified` rather than assuming.** `lazydap break --list` shows the column, and the
`breakpoints` array in `launch`'s response carries both `verified` and the adapter's `message`. A
script or an agent that reads `verified: false` and stops is spared the confusion; one that
ignores it debugs a program that never pauses.

### The real fix, not yet made

lazydap knows both spellings at the moment it matters and could reconcile them: when a
breakpoint comes back `verified: false` with a resolved location that differs from the requested
source only by symlink resolution, re-send it under the path the adapter found. That is a change
to how the store and the launch configuration phase handle source paths, which is
**[M15](../implementation/tasks/M15-config-file.md)'s code half** — the same milestone that owns
`launch.json` import and its `${workspaceFolder}` substitution, and therefore the place where
path handling gets thought about properly rather than patched here.

Until then this is documented rather than fixed, which is the honest trade: the workaround is one
directory move.

### Cross-references

- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — the warning where somebody about to do this will see it
- Milestone: [`M15-config-file.md`](../implementation/tasks/M15-config-file.md) — where source-path handling belongs
- Quirk 1 — the other way a breakpoint silently fails to do anything

---

## Adding a new quirk

When you discover a new codelldb behaviour worth documenting:

1. Add a row to the index table at the top.
2. Add a section below following the structure of existing entries: **Symptom**, **Root cause**, **Fix**, **Cross-references**.
3. Cross-link to a `docs/issues/` entry if it represents an upstream/contributor problem worth tracking, and to any chapters or milestones that hit it.
4. Update the "Tested against" version when you verify against a newer release.

The bar for inclusion: any quirk that takes more than 10 minutes to figure out the first time, or has bitten the project more than once. (This mirrors AGENTS.md's general rule: "Add to `docs/reference/` whenever a question takes more than 10 minutes to answer for the second time.")
