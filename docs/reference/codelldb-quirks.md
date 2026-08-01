# codelldb quirks reference

Version-drift, install footguns, and runtime quirks of `codelldb` that affect lazydap. Add to this when you discover new ones; remove rows that get fixed upstream.

This doc is the canonical place for "this codelldb thing surprised us." Cross-link to it from milestone docs, book chapters, and the `crates/daemon/src/adapter/codelldb.rs` source.

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
| 9 | [No `process` event, so the debuggee's pid is only in console text](#9-no-process-event-the-debuggees-pid-is-only-in-console-text) | Review round after M19 (2026-07-30) | codelldb 1.12.2 / Darwin 25.5.0 |
| 10 | [Rust and C++ type summaries need `sourceLanguages`](#10-rust-and-c-type-summaries-need-sourcelanguages-in-the-launch) | Dogfooding lazydap on itself (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 11 | [Struct summaries drop struct-typed fields, with no ellipsis](#11-struct-summaries-drop-struct-typed-fields-with-no-ellipsis) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 12 | [`Duration`'s summary drops `nanos`, so the number is wrong](#12-durations-summary-drops-nanos-so-the-number-is-wrong) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 13 | [`BTreeMap` has no formatter and is uninspectable](#13-btreemap-has-no-formatter-and-is-uninspectable) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 14 | [Out-of-bounds indexing in `eval` returns `0`, successfully](#14-out-of-bounds-indexing-in-eval-returns-0-successfully) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 15 | [`map[key]` is a positional index, not a key lookup](#15-mapkey-is-a-positional-index-not-a-key-lookup) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 16 | [A `&str` containing a NUL is truncated at the NUL](#16-a-str-containing-a-nul-is-truncated-at-the-nul) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 17 | [`[u8; N]` renders as an escaped C string](#17-u8-n-renders-as-an-escaped-c-string) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 18 | [Rust primitives report C type names](#18-rust-primitives-report-c-type-names) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 19 | [`eval` cannot call methods](#19-eval-cannot-call-methods) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 20 | [Unknown-identifier errors open with an alarming irrelevant banner](#20-unknown-identifier-errors-open-with-an-alarming-irrelevant-banner) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |
| 21 | [The adapter's own chatter is emitted as `stderr`](#21-the-adapters-own-chatter-is-emitted-as-stderr) | Dogfooding campaign (2026-08-01) | codelldb 1.12.2 / Darwin 25.5.0 |

Quirks 11 to 20 are all about **reading values** — the summary strings in `value`, and what
`eval` will and will not do. They were found in one sitting against a Rust fixture holding one
of everything, and they share a moral: `value` is a *display* string written for a human
glancing at a debugger pane, and an agent that parses it as data will be wrong in ways that do
not announce themselves. `variables --reference` is the data.

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

The codelldb adapter implementation in `crates/daemon/src/adapter/codelldb.rs` sets `RUST_LOG=debug` for spawned codelldb processes, with a comment inline pointing back at this quirk.

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

**lazydap handles this itself since M15** (D048). When a `setBreakpoints` response comes back
with nothing bound and the adapter's message names a location it *could* have used, lazydap
re-sends that file's breakpoints under the name the adapter offered, and takes the second answer
as final. Both places that talk to `setBreakpoints` do it: the configuration phase of a launch
(`crates/daemon/src/adapter/codelldb.rs`) and a breakpoint set during a live session
(`AdapterHandle::set_breakpoints`). What a caller sees under `/tmp` now:

```json
{
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6, "message": "Resolved locations: 1",
      "source": "/private/tmp/lzdemo/hello.c", "verified": true }
  ]
}
```

and `continue --wait` returns `"reason": "breakpoint"` with `"hit_breakpoint_ids": [1]`. The
stored source stays canonical — only the spelling on the wire changes — so ids, `break --list`
and the state file are unaffected.

Two guards, both deliberate. The retry happens **only when nothing in that file bound**, because
re-sending a whole list under a second path while the first is still live would leave the adapter
holding two breakpoints for one of ours. And the suggested path is accepted **only if it resolves
to the same file**, checked through the filesystem rather than compared as text: an adapter
pointing somewhere else is offering to break in code nobody asked about.

codelldb's original complaint still appears in the session's console output, because it is what
codelldb said. It is now followed by a breakpoint that works.

**On an older build**, or if the retry does not apply: keep debuggees out of `/tmp` — a directory
under `$HOME` has no symlink on the way to it. Or make the two spellings agree by compiling with
the resolved path:

```bash
gcc -g -O0 /private/tmp/scratch/hello.c -o /private/tmp/scratch/hello
```

**Check `verified` rather than assuming**, either way. `lazydap break --list` shows the column,
and the `breakpoints` array in `launch`'s response carries both `verified` and the adapter's
`message`. A script or an agent that reads `verified: false` and stops is spared the confusion;
one that ignores it debugs a program that never pauses.

### Cross-references

- [`CONTRIBUTING.md`](../../CONTRIBUTING.md) — the warning where somebody about to do this will see it
- D048 in [`docs/blueprint/15-decision-log.md`](../blueprint/15-decision-log.md) — the retry, and what bounds it
- `crates/daemon/src/adapter/mod.rs::rebind_source` — the two guards, in code
- Milestone: [`M15-config-file.md`](../implementation/tasks/M15-config-file.md) — where source-path handling landed
- Quirk 1 — the other way a breakpoint silently fails to do anything

---

---

## 9. No `process` event; the debuggee's pid is only in console text

### Symptom

lazydap needs the debuggee's process id so it can clean the program up if the adapter dies
without stopping it (D045). DAP defines exactly the event for this — `process`, carrying
`systemProcessId` — and codelldb never sends it.

### Root cause

It is not implemented. The string does not appear anywhere in the binary:

```console
$ strings ~/.local/opt/codelldb/extension/adapter/codelldb | grep -c systemProcessId
0
```

A full launch-to-exit event stream confirms it. Every event codelldb produced for a trivial
C program, in order:

```
output, initialized, output, output, module (x40), continued, output, exited, terminated
```

No `process`. What it does print, once, to the `console` output category:

```
Launched process 56254 from '/path/to/program'
```

### Fix

Scrape it — `crates/daemon/src/adapter/codelldb.rs::launched_pid`. Two things make that
tolerable:

- **It is best-effort.** A parse that fails costs only the cleanup, and the session behaves
  exactly as it did before the cleanup existed.
- **Nothing is killed on the strength of the pid alone.** Before the recorded pid is
  signalled, `ps` is asked whether it still names the program we launched; a recycled pid
  belongs to a stranger, and killing one would be far worse than leaking the process we
  were looking for.

The line arrives *during the launch handshake*, not on the session read pump — the
handshake owns the transport until the session is live — so it is read out of the launch
outcome rather than by the pump.

### Cross-references

- D045 in [`docs/blueprint/15-decision-log.md`](../blueprint/15-decision-log.md)
- `crates/daemon/src/debuggee.rs` — the identity check and the kill
- If codelldb ever gains a `process` event, prefer it and delete the scrape.

---

## 10. Rust and C++ type summaries need `sourceLanguages` in the launch

### Symptom

`eval` (or reading a local through `variables`) on a Rust `&str` returns garbage instead of
the string. Evaluating a `&str` holding `"0.1.0"`:

```json
{ "type_name": "&str",
  "value": "{data_ptr:\"0.1.0lazydapunsafe precondition(s) violated: Layout::from_size_align_unchecked ...\", ...}" }
```

The value runs far past the five bytes of `"0.1.0"` into whatever read-only data sits next to
it. `String`, `Vec`, `Option` and the rest of Rust's types render the same raw way, and
`String`/`Vec` method calls in `eval` fail.

### Root cause

A Rust `&str` is a fat pointer — a data pointer plus a length. codelldb only loads LLDB's
Rust type-summary formatters (the ones that know to read exactly `len` bytes) when the launch
request names the language in `sourceLanguages`. Without it, LLDB falls back to a generic
pointer rendering and reads `data_ptr` as a null-terminated C string; Rust string data is not
null-terminated, so the read spills into adjacent rodata. The C and C++ formatters are gated
the same way.

Found dogfooding lazydap on its own Rust binary — the C fixtures use `int` and `double`,
which render correctly without the formatters, so no earlier test caught it. Rust is a target
language, so this was a real defect, not cosmetic.

### Fix

lazydap's codelldb launch sends `sourceLanguages: ["rust", "cpp", "c"]`
(`crates/daemon/src/adapter/codelldb.rs`). codelldb ignores names for languages it has no
formatters for, so listing all three is safe for any LLDB debuggee. With it, the same `eval`
returns `"0.1.0"`.

### Cross-references

- `crates/dap/src/types.rs` — `LaunchArgs.source_languages`
- [codelldb MANUAL, launch settings](https://github.com/vadimcn/codelldb/blob/master/MANUAL.md)

---
## 11. Struct summaries drop struct-typed fields, with no ellipsis

### Symptom

A struct with six fields renders with four of them. Given

```rust
struct Inner { x: i32, y: i32 }
struct Multi { a: Inner, b: Inner, n: i32, s: String, v: Vec<i32>, o: Option<i32> }
```

`variables` on the local scope says:

```json
{ "name": "m",
  "type_name": "rustq::Multi",
  "value": "{n:3, s:\"ss\", v:size=2, o:Some(4)}",
  "variables_reference": 1007 }
```

`a` and `b` — the two struct-typed fields — are not there, and **nothing in the string says
anything is missing.** No `...`, no count, no marker of any kind. The summary is a complete,
well-formed rendering of a struct that does not exist.

The fields are real and readable. Expanding reference 1007:

```json
{ "variables": [
    { "name": "a", "type_name": "rustq::Inner", "value": "{x:1, y:2}", "variables_reference": 1020 },
    { "name": "b", "type_name": "rustq::Inner", "value": "{x:3, y:4}", "variables_reference": 1021 },
    { "name": "n", "type_name": "int", "value": "3", "variables_reference": 0 },
    { "name": "s", "type_name": "alloc::string::String", "value": "\"ss\"", "variables_reference": 1022 },
    { "name": "v", "type_name": "alloc::vec::Vec<int, alloc::alloc::Global>", "value": "size=2", "variables_reference": 1023 },
    { "name": "o", "type_name": "core::option::Option<i32>", "value": "Some(4)", "variables_reference": 1024 },
    { "name": "[raw]", "value": "rustq::Multi", "variables_reference": 1025 } ] }
```

Note `a` and `b` each render their *own* children inline as `{x:1, y:2}` — so the omission is
not "structs cannot be summarised", it is one level of nesting being dropped silently.

### Root cause

The ellipsis convention exists — codelldb applies it elsewhere. A `BTreeMap` in the same
frame summarises as

```json
"value": "{root:Some({height:0, node:{pointer:0x00000001006219b0}, _marker:()}), ...}"
```

which does end in `, ...}`. So a reader who has seen a truncated summary reasonably concludes
that a summary *without* `...` is complete. For struct-typed fields it is not, and the two
behaviours are produced by paths that do not agree with one another.

This is the most misleading entry in this file. The others produce a value that is visibly odd,
or an error. This one produces a plausible, tidy, confident answer to a question nobody asked,
and an agent reading it concludes **the field does not exist** — then reports that to a user as
a finding about their program.

### Fix or workaround

Do not read a struct's fields out of `value`. Ever. Expand it:

```bash
lazydap variables --reference 1007 --format json
```

The rule generalises past this quirk: `value` is a display string, `variables --reference` is
the data. Quirks 12, 13, 16 and 17 are each a different way the same assumption fails.

### Cross-references

- Quirk 10 — without `sourceLanguages` there are no Rust summaries at all, which is the
  louder failure of the same machinery
- Quirks 12, 13 — the other two summaries that are wrong rather than merely partial
- The docs-site guide *Write one script for four languages*, for the cross-adapter version of
  this rule

---

## 12. `Duration`'s summary drops `nanos`, so the number is wrong

### Symptom

`Duration::from_millis(1500)` — one and a half seconds — summarises as one second:

```json
{ "name": "dur",
  "type_name": "core::time::Duration",
  "value": "{secs:1}",
  "variables_reference": 1008 }
```

`eval "dur"` gives the same `{secs:1}`.

Expanding reference 1008 finds the missing third of the value:

```json
{ "variables": [
    { "name": "secs", "type_name": "unsigned long", "value": "1", "variables_reference": 0 },
    { "name": "nanos", "type_name": "core::num::niche_types::Nanoseconds", "value": "{0:500000000}", "variables_reference": 1026 },
    { "name": "[raw]", "value": "core::time::Duration", "variables_reference": 1027 } ] }
```

### Root cause

`nanos` is not a plain integer — it is a niche-optimised newtype, `Nanoseconds`, whose own
summary is `{0:500000000}`. It is a struct-typed field, so quirk 11 applies and it is dropped
from the parent summary without an ellipsis.

What makes this worth its own entry is the consequence rather than the mechanism. Quirk 11
loses you a field you can go and fetch. Here the field that survives is a *number*, and the
number that survives is **wrong** — 1 where the truth is 1.5, an error of 33% in this example
and up to 100% for any duration under a second. `Duration::from_millis(999)` summarises as
`{secs:0}`.

Every Rust `Duration` an agent reads off a summary is a floor to the second. That includes
every timeout, every elapsed measurement, and every rate.

### Fix or workaround

Expand it and add the two fields yourself, or evaluate the parts:

```bash
lazydap eval "dur.secs" --format json    # 1
lazydap eval "dur.nanos" --format json   # {0:500000000}
```

There is no expression that returns the whole thing as a number — quirk 19 means
`dur.as_millis()` is not available.

### Cross-references

- Quirk 11 — the general rule this is an instance of
- Quirk 19 — why you cannot just call `as_millis()`

---

## 13. `BTreeMap` has no formatter and is uninspectable

### Symptom

A `BTreeMap<i32, i32>` holding `{1: 10, 2: 20}` summarises as its own node internals:

```json
{ "name": "bt",
  "type_name": "alloc::collections::btree::map::BTreeMap<int, int, alloc::alloc::Global>",
  "value": "{root:Some({height:0, node:{pointer:0x00000001006219b0}, _marker:()}), ...}",
  "variables_reference": 1009 }
```

Expanding does not help. Reference 1009:

```json
{ "variables": [
    { "name": "root", "type_name": "core::option::Option<...NodeRef<...>>",
      "value": "Some({height:0, node:{pointer:0x00000001006219b0}, _marker:()})", "variables_reference": 1028 },
    { "name": "length", "type_name": "unsigned long", "value": "2", "variables_reference": 0 },
    { "name": "alloc", "type_name": "core::mem::manually_drop::ManuallyDrop<alloc::alloc::Global>", "value": "{...}", "variables_reference": 1029 },
    { "name": "_marker", "type_name": "core::marker::PhantomData<(i32, i32) *>", "value": "<not available>", "variables_reference": 1030 },
    { "name": "[raw]", "value": "alloc::collections::btree::map::BTreeMap<int, int, alloc::alloc::Global>", "variables_reference": 1031 } ] }
```

`length: 2` is the only true statement about the map's contents anywhere in that response.
There are no keys and no values, at any depth reachable by expanding — only the B-tree's
`root` pointer, which you would have to walk by hand through raw memory.

### Root cause

The Rust formatters codelldb loads (quirk 10) cover the other collections and not this one.
Every neighbour in the same frame formats correctly:

| type | summary | expansion |
|---|---|---|
| `Vec<i32>` | `size=5` | `[0]`…`[4]` with values |
| `HashMap<i32, i32>` | `size=2` | `[0]`, `[1]` as `(key, value)` tuples |
| `HashSet<i32>` | `size=1` | elements |
| `VecDeque<i32>` | `size=1` | elements |
| `BTreeMap<i32, i32>` | node internals | **no keys, no values** |

So this is a gap in coverage rather than a design choice, and the `size=N` convention the
others share is exactly what `BTreeMap` fails to produce.

### Fix or workaround

There is no good one. `BTreeMap` is effectively uninspectable through the debugger.

- `length` is available and correct, so "is it empty" and "how many" are answerable.
- To see contents, print them from the program — a `dbg!` or a log line — or convert to a
  `Vec` at a point you control.
- If the choice of map is yours and you expect to debug it, `HashMap` inspects properly.

Report it upstream rather than working around it silently if this costs you real time; it is a
missing formatter, not a hard problem.

### Cross-references

- Quirk 10 — where the Rust formatters come from, and what happens with none of them
- Quirk 15 — how indexing behaves on the collections that *do* format

---

## 14. Out-of-bounds indexing in `eval` returns `0`, successfully

### Symptom

Indexing a 5-element `Vec<i32>` at 99 succeeds and returns zero:

```console
$ lazydap eval "v_int[99]" --format json
{
  "type_name": "int",
  "value": "0",
  "variables_reference": 0
}
$ echo $?
0
```

For comparison, the in-range read next to it:

```console
$ lazydap eval "v_int[2]" --format json
{ "type_name": "int", "value": "3", "variables_reference": 0 }
```

Nothing distinguishes the two responses in shape, type or exit code. An empty `Vec` indexed at
`0` is different again — it returns a *sentence* in the `value` field, still with exit 0:

```console
$ lazydap eval "v_empty[0]" --format json
{
  "type_name": "int",
  "value": "<read memory from 0x4 failed (0 of 4 bytes read)>",
  "variables_reference": 0
}
```

### Root cause

The expression evaluator does pointer arithmetic on the `Vec`'s data pointer and reads what is
there. No bounds check happens, because the length is not consulted — `v_int[99]` is
`*(data + 99)`, which for a 5-element vector lands in allocator slack or the neighbouring heap.
Zero is simply what was in that memory. `v_empty[0]` dereferences `Vec::new()`'s dangling
alignment-only pointer (`0x4` here), the read fails, and LLDB puts its complaint in the value
slot rather than failing the request.

This is C semantics applied to a Rust type: correct for the machine, and the opposite of what
the language guarantees. `v_int[99]` in the program would panic.

### Fix or workaround

Bound-check before you index, because the adapter will not:

```bash
lazydap eval "/py len(\$v_int)" --format json   # 5 — see quirk 19
```

Treat any `0` from an index expression as suspect until you know the length. And treat a
`value` beginning `<` as an error regardless of the exit code — `<read memory ... failed>` and
`<not available>` both appear in that slot on success.

### Cross-references

- Quirk 15 — the same missing-bounds-check story for maps, where it is worse
- Quirk 19 — `/py` and why `len()` needs it

---

## 15. `map[key]` is a positional index, not a key lookup

### Symptom

A `HashMap<i32, i32>` holding `{1: 100, 2: 200}`. Ask for key 1:

```console
$ lazydap eval "hm_int[1]" --format json
{
  "type_name": "(i32, i32)",
  "value": "(1, 100)",
  "variables_reference": 1042
}
```

Correct — key 1 maps to 100. So the lookup works. Now ask for key 2:

```console
$ lazydap eval "hm_int[2]" --format json
{"details":{"adapter_message":"Index '2' is out of range","command":"evaluate"},
 "error":"DapProtocolError",
 "message":"DapProtocolError: the adapter rejected `evaluate`: Index '2' is out of range"}
```

Out of range — on a two-entry map that contains key 2. And index 0, which is not a key at all:

```console
$ lazydap eval "hm_int[0]" --format json
{ "type_name": "(i32, i32)", "value": "(2, 200)", "variables_reference": 1040 }
```

The subscript is a **child index**, not a key. Expanding the map shows the order it is
indexing:

```json
{ "variables": [
    { "name": "[0]", "type_name": "(i32, i32)", "value": "(2, 200)", "variables_reference": 1032 },
    { "name": "[1]", "type_name": "(i32, i32)", "value": "(1, 100)", "variables_reference": 1033 } ] }
```

`hm_int[1]` returned `(1, 100)` because entry 1 in *this run's* iteration order happens to be
the pair whose key is 1. That is a coincidence of one hash seed. Rust's `RandomState` is seeded
per process, so the same expression on the same program tomorrow returns a different entry —
with no error, and a plausible-looking `(key, value)` tuple either way.

String keys do not even get that far:

```console
$ lazydap eval 'hm_str["k"]' --format json
{"details":{"adapter_message":"'str' object cannot be interpreted as an integer","command":"evaluate"},
 "error":"DapProtocolError",
 "message":"DapProtocolError: the adapter rejected `evaluate`: 'str' object cannot be interpreted as an integer"}
```

That is a Python `TypeError` message escaping from the formatter's implementation, unmapped.

### Root cause

The subscript operator is handled by the same generic "get child N" path that serves `Vec` and
arrays, and the collection formatter presents a `HashMap`'s children as numbered `(key, value)`
tuples. Nothing in the chain knows that for a map the subscript was meant as a key. The Python
`TypeError` is the formatter's `__getitem__` receiving a `str` where it expects an index.

Integer-keyed maps make this dangerous rather than merely wrong: the expression is *type*-valid,
it succeeds, and it returns the right shape. Only the answer is unrelated to the question.

### Fix or workaround

Never subscript a map in `eval`. To find a key, expand the map and match on the tuples:

```bash
lazydap eval "hm_int" --format json          # note variables_reference
lazydap variables --reference 1010 --format json
```

then read the `[N]` children and pick the one whose first element is your key. It is the only
approach that is correct regardless of hash order, and it is honest about the map being
unordered.

`BTreeMap`, which is ordered and would make positional indexing meaningful, cannot be expanded
at all (quirk 13).

### Cross-references

- Quirk 13 — the ordered map, which has the opposite problem
- Quirk 14 — unchecked indexing on sequences

---

## 16. A `&str` containing a NUL is truncated at the NUL

### Symptom

`let nul_str: &str = "before\0after";` — twelve bytes, all of them meaningful — summarises as
six characters and an unbalanced quote:

```json
{ "name": "nul_str",
  "type_name": "&str",
  "value": "\"before",
  "variables_reference": 1016 }
```

That is the JSON encoding of `"before` — an opening double quote, `before`, and no closing
quote. The closing quote is not missing from lazydap's encoding; it is missing from the string
codelldb sent.

The bytes are all present under expansion. Reference 1016:

```json
{ "variables": [
    { "name": "[0]", "type_name": "unsigned char", "value": "98", "variables_reference": 0 },
    { "name": "[1]", "type_name": "unsigned char", "value": "101", "variables_reference": 0 },
    { "name": "[2]", "type_name": "unsigned char", "value": "102", "variables_reference": 0 },
    { "name": "[3]", "type_name": "unsigned char", "value": "111", "variables_reference": 0 },
    { "name": "[4]", "type_name": "unsigned char", "value": "114", "variables_reference": 0 },
    { "name": "[5]", "type_name": "unsigned char", "value": "101", "variables_reference": 0 },
    { "name": "[6]", "type_name": "unsigned char", "value": "0",   "variables_reference": 0 },
    { "name": "[7]", "type_name": "unsigned char", "value": "97",  "variables_reference": 0 },
    { "name": "[8]", "type_name": "unsigned char", "value": "102", "variables_reference": 0 },
    { "name": "[9]", "type_name": "unsigned char", "value": "116", "variables_reference": 0 },
    { "name": "[10]", "type_name": "unsigned char", "value": "101", "variables_reference": 0 },
    { "name": "[11]", "type_name": "unsigned char", "value": "114", "variables_reference": 0 } ] }
```

`98 101 102 111 114 101 0 97 102 116 101 114` is exactly `before\0after`. Nothing was lost on
the wire — only in the summary.

### Root cause

A Rust `&str` is length-delimited: a data pointer plus a byte count, and NUL is an ordinary
byte that `"before\0after"` may legally contain. The summary writer reads the fat pointer's
length correctly enough to expand twelve children, then formats the text with C semantics —
stop at the first NUL — and emits the closing quote only after the terminator it never
reached.

This is the same family as quirk 10, where a `&str` with no formatters at all is read as a
C string and over-runs into neighbouring rodata. Here the formatter is loaded and still applies
one C rule.

### Fix or workaround

An unterminated quote in a `value` is the tell: if a string summary starts with `"` and does
not end with `"`, it was truncated and you are looking at a prefix. Expand the reference and
read the bytes.

Strings from `String`/`&str` that contain no NUL are unaffected, which is nearly all of them —
this bites protocol buffers, framed wire formats, and anything that came from a `read` rather
than a literal.

### Cross-references

- Quirk 10 — the same C-string assumption, applied with no formatters loaded at all
- Quirk 17 — the inverse: bytes that are not text, formatted as text anyway

---

## 17. `[u8; N]` renders as an escaped C string

### Symptom

`let bytes: [u8; 300] = [7u8; 300];` — three hundred bells — produces a 600-character value:

```json
{ "name": "bytes",
  "type_name": "unsigned char[300]",
  "value": "\"\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a\\a…\"",
  "variables_reference": 1017 }
```

(Trimmed here; the real value carries all 300 `\a` escapes.) An `[i32; 4]` in the same frame
renders the way you would expect an array to:

```json
{ "name": "ints", "type_name": "int[4]", "value": "{10, 20, 30, 40}", "variables_reference": 1018 }
```

So the array formatter is fine. It is the element type that changes the rendering.

### Root cause

Rust's `u8` lowers to DWARF as `unsigned char` (quirk 18), and LLDB formats an array of
`unsigned char` as a character string, escaping every non-printable byte. `0x07` is BEL, whose
C escape is `\a`, so each byte costs two characters — and a buffer of genuinely random bytes
costs four each, as `\xNN`.

The practical limit arrives fast. A 4 KiB read buffer is 8–16 KiB of escapes in a single JSON
string field, for a value carrying no more information than `size=4096` would have.

### Fix or workaround

Read byte buffers through `variables --reference`, which gives you numbers:

```bash
lazydap variables --reference 1017 --format json   # [0]=7, [1]=7, ...
```

For a large buffer, prefer a targeted `eval` — `bytes[0]`, or a slice length — over asking for
the whole thing. Note quirk 14: those indexes are unchecked.

Be aware of this before dumping a scope containing a big `[u8; N]` into an agent's context
window; the escaped form is the single largest thing a locals dump can produce.

### Cross-references

- Quirk 18 — why `u8` is `unsigned char` in the first place
- Quirk 14 — indexing into the buffer instead, and its lack of bounds checks

---

## 18. Rust primitives report C type names

### Symptom

Every scalar in a Rust program comes back named as its C equivalent:

| declared | `type_name` | `value` |
|---|---|---|
| `i8` | `char` | `-1` |
| `i16` | `short` | `-2` |
| `i32` | `int` | `-3` |
| `i64` | `long` | `-4` |
| `u64` | `unsigned long` | `4` |
| `usize` | `unsigned long` | `5` |
| `char` | `char32_t` | `U+007A 'z'` |
| `f64` | `double` | `1.5` |
| `i128` | `__int128` | `7` |
| `bool` | `bool` | `true` |

**The values are all correct.** This is a naming quirk, not a data quirk.

Generic types get rewritten in the same direction, inconsistently:

```json
{ "name": "v_int", "type_name": "alloc::vec::Vec<int, alloc::alloc::Global>", "value": "size=5" }
{ "name": "opt",   "type_name": "core::option::Option<i32>",                  "value": "Some(4)" }
```

`Vec<i32>` became `Vec<int, alloc::alloc::Global>` — parameter rewritten to `int`, plus the
allocator parameter Rust source elides. `Option<i32>` kept `i32`. Both are in the same frame,
in the same response.

### Root cause

rustc emits DWARF using the base-type encodings LLDB's C type system already understands, and
LLDB names a type by what its DWARF says rather than by the Rust spelling. The generic
inconsistency follows from *where* each name comes from: a name reconstructed from the DWARF
type graph gets `int` and shows the defaulted allocator, and one carried through as a
template-parameter string keeps `i32`.

### Fix or workaround

Do not match on `type_name` for Rust. Specifically:

- **`i8` reporting `char` is the one that misleads.** An agent that branches on `char` will
  treat a signed byte as text. Nothing else in the table is ambiguous in a harmful way.
- `u64` and `usize` are indistinguishable — both `unsigned long`. If the difference matters,
  it is not recoverable from the response.
- To identify a Rust type, prefer the variable's name and the shape of its `value`
  (`size=N` for a collection, `Some(...)` for an `Option`) over `type_name`.

For code that must work across adapters, note delve sends no `type_name` **at all** — so
`type_name` cannot be a required field in any four-language code path. See the docs-site guide
*Write one script for four languages*.

### Cross-references

- Quirk 17 — `u8` being `unsigned char` is why byte arrays render as text
- [`delve-quirks.md`](delve-quirks.md) quirk 12 — the adapter that omits the field entirely

---

## 19. `eval` cannot call methods

### Symptom

Any method call is a syntax error, at the open parenthesis:

```console
$ lazydap eval "v_int.len()" --format json
{"details":{"adapter_message":"Syntax error: v_int.len()\n                       ^","command":"evaluate"},
 "error":"DapProtocolError","message":"..."}

$ lazydap eval "opt.is_some()" --format json
{"details":{"adapter_message":"Syntax error: opt.is_some()\n                         ^","command":"evaluate"},
 "error":"DapProtocolError","message":"..."}
```

Everything that is not a call works normally:

```console
$ lazydap eval "m.a.x" --format json
{ "type_name": "int", "value": "1", "variables_reference": 0 }

$ lazydap eval "a_i32 + 1" --format json
{ "type_name": "long long", "value": "-2", "variables_reference": 0 }

$ lazydap eval "v_int[2]" --format json
{ "type_name": "int", "value": "3", "variables_reference": 0 }
```

So: field access, nested field access, indexing and arithmetic — yes. Calls — no.

### Root cause

codelldb evaluates Rust expressions with its own small interpreter rather than by compiling
them, and that interpreter has no calling machinery: it can walk a value graph and do
arithmetic on what it finds, and it cannot enter the debuggee to run code.

The `/nat` prefix, which asks for the native (LLDB) evaluator, does **not** rescue it — it
just fails further along, in C++:

```console
$ lazydap eval "/nat v_int.len()" --format json
... "Expression evaluation in Rust not supported. Falling back to default language.
     Ran expression as 'Objective C++'.
     error: <user expression 1>:1:10: called object type 'unsigned long' is not a function or function pointer
         1 | v_int.len()
           | ~~~~~~~~~^"
```

`/py`, which runs a Python expression against LLDB's scripting API, does work. Variables are
spelled `$name`:

```console
$ lazydap eval "/py v_int.len()" --format json
... "name 'v_int' is not defined"

$ lazydap eval "/py len(\$v_int)" --format json
{ "type_name": "long long", "value": "5", "variables_reference": 0 }
```

### Fix or workaround

Use `/py len($var)` for lengths, and otherwise get what you need from field access and
expansion. `Vec`, `HashMap`, `HashSet` and `VecDeque` all report their length in the summary
as `size=N` anyway, which is cheaper than either.

**This is codelldb-specific.** debugpy evaluates calls without complaint:

```console
$ lazydap eval "len([1,2,3])" --format json      # against a Python debuggee
{ "type_name": "int", "value": "3", "variables_reference": 0 }
```

and so does delve (`len(s)` → `3`). An agent that learned "the debugger can call functions"
from a Python session will write expressions that fail only against C, C++ and Rust.

### Cross-references

- Quirk 7 — the other reason an `eval` string might not mean what you think
- Quirk 14 — `/py len($v)` is also the bounds check indexing does not do

---

## 20. Unknown-identifier errors open with an alarming irrelevant banner

### Symptom

Evaluate a name that does not exist and the error begins by announcing that Rust is not
supported and your expression was run as Objective-C++:

```console
$ lazydap eval "no_such_var" --format json
{"details":{"adapter_message":"Expression evaluation in Rust not supported. Falling back to default language. Ran expression as 'Objective C++'.\nerror: <user expression 0>:1:1: use of undeclared identifier 'no_such_var'\n    1 | no_such_var\n      | ^~~~~~~~~~~\n","command":"evaluate"},
 "error":"DapProtocolError","message":"..."}
```

Read as prose, that says Rust debugging is broken. The actual diagnosis — `use of undeclared
identifier 'no_such_var'` — is on the second line, after the part that sounds like a
catastrophe.

### Root cause

The banner is emitted whenever codelldb's Rust expression evaluator declines an expression and
LLDB falls back to its default language. Declining is routine: quirk 19 means every method call
takes this path, and so does every name the Rust evaluator cannot resolve. The fallback then
produces the real error, in the language it actually used.

Rust evaluation is not broken, and the banner is not evidence that it is. Field access,
indexing and arithmetic on Rust values all succeed without it ever appearing.

### Fix or workaround

**Read past the first line.** When parsing adapter errors programmatically, split
`adapter_message` on `\n` and take the line starting `error:` — the banner is line 0 and
carries no information about what went wrong.

Do not report the banner to a user as a finding. "Expression evaluation in Rust not supported"
is a sentence an agent will faithfully relay, and it is false.

### Cross-references

- Quirk 19 — the most common way to provoke the banner
- Quirk 7 — errors that come from LLDB's *command* interpreter instead, which look different again

---

## 21. The adapter's own chatter is emitted as `stderr`

### Symptom

Every `--stop-on-entry` launch of a C, C++ or Rust program produces a line of adapter
commentary tagged as the debuggee's standard error:

```console
$ lazydap launch ./cq --stop-on-entry --format json
{ "raw_reason": "exception", "reason": "entry", "state": "paused", ... }

$ lazydap output --format json
{
  "chunks": [
    { "category": "console", "output": "Console is in 'commands' mode, prefix expressions with '?'.\n", "timestamp_ms": 1785621741833 },
    { "category": "console", "output": "Loading Rust formatters from /Users/you/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/etc\n", "timestamp_ms": 1785621741901 },
    { "category": "console", "output": "Launching: /Users/you/cq\n", "timestamp_ms": 1785621741902 },
    { "category": "console", "output": "Launched process 32172 from '/Users/you/cq'\n", "timestamp_ms": 1785621742369 },
    { "category": "stderr", "output": "Stop reason: signal SIGSTOP\n", "timestamp_ms": 1785621742369 }
  ],
  "dropped": 0
}
```

The program is `int main(void) { int x = 5; printf("hello from cq\n"); return x - 5; }`. It has
not run yet — it is paused at `_dyld_start` — and it never writes to stderr at all. The chunk
is codelldb describing its own entry-stop mechanism.

It survives into `captured_output` on the next `continue --wait`, interleaved with the real
program output:

```json
"captured_output": [
  { "category": "console", "output": "Launched process 32172 from '/Users/you/cq'\n", ... },
  { "category": "stderr", "output": "Stop reason: signal SIGSTOP\n", ... },
  { "category": "stdout", "output": "hello from cq\r\n", ... },
  { "category": "console", "output": "Process exited with code 0.\n", ... }
],
"exit_code": 0,
"state": "exited"
```

`exit_code` is 0. The program succeeded. An agent applying the ordinary shell heuristic —
non-empty stderr means something went wrong — reports a crash that did not happen.

### Root cause

codelldb reports the entry stop by sending the process `SIGSTOP` rather than by setting an
entry breakpoint (quirk 6, which is why `raw_reason` is `exception`). It then narrates the stop
it caused, and sends that narration as a DAP `output` event with `category: "stderr"` instead
of `"console"`, which is the category DAP reserves for adapter-generated text. The three
`console` chunks around it are correctly categorised; this one is not.

lazydap passes categories through as the adapter sets them, so the mislabelling reaches the
client. **Verified against the current build on 2026-08-01: still `stderr`.** Whether lazydap
should re-categorise adapter text it can recognise is being looked at separately; this entry
describes codelldb's behaviour, which is the part that will not change.

### Fix or workaround

Do not treat a non-empty `stderr` category as a failure signal. Branch on `state` and
`exit_code`, which are unambiguous:

```bash
lazydap continue --wait --format json | jq '.state, .exit_code'
```

If you are surfacing captured output to a user, `Stop reason: signal SIGSTOP` on a
`--stop-on-entry` launch can be dropped — but drop it by matching that exact string on a
launch you know used `--stop-on-entry`, not by filtering the category, or you will swallow the
program's real diagnostics.

The chunk does not appear without `--stop-on-entry`.

### Cross-references

- Quirk 6 — the `SIGSTOP` mechanism this is the narration of
- D033 in [`docs/blueprint/15-decision-log.md`](../blueprint/15-decision-log.md) — the `reason`/`raw_reason` normalisation of the same stop
- [`delve-quirks.md`](delve-quirks.md) quirk 3 — delve mixing its own chatter into `stdout`, the same problem in the other category

---

## Adding a new quirk

When you discover a new codelldb behaviour worth documenting:

1. Add a row to the index table at the top.
2. Add a section below following the structure of existing entries: **Symptom**, **Root cause**, **Fix**, **Cross-references**.
3. Cross-link to a `docs/issues/` entry if it represents an upstream/contributor problem worth tracking, and to any chapters or milestones that hit it.
4. Update the "Tested against" version when you verify against a newer release.

The bar for inclusion: any quirk that takes more than 10 minutes to figure out the first time, or has bitten the project more than once. (This mirrors AGENTS.md's general rule: "Add to `docs/reference/` whenever a question takes more than 10 minutes to answer for the second time.")
