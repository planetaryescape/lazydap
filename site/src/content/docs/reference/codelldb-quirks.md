---
title: codelldb quirks
description: Eight codelldb behaviours that surprise people, with the cause and the fix for each.
---

lazydap wraps codelldb, so codelldb's surprises become yours. These eight have each cost real
time at least once. Tested against codelldb 1.12.2 on Darwin 25.5.0.

If you are here because something is broken rather than because you are reading reference
material, [troubleshooting](/troubleshooting/) is organised by symptom instead.

| # | Quirk | Bites you when |
|---|---|---|
| 1 | [A symlink on `PATH` breaks liblldb resolution](#1-a-symlink-on-path-breaks-liblldb-resolution) | Installing |
| 2 | [Silent on stderr without `RUST_LOG`](#2-silent-on-stderr-without-rust_log) | Writing an adapter |
| 3 | [DAP over TCP, not stdio](#3-dap-over-tcp-not-stdio) | Writing an adapter |
| 4 | [`--version` is not a flag](#4---version-is-not-a-flag) | Scripting an install check |
| 5 | [Every invocation hangs after a macOS update](#5-every-invocation-hangs-after-a-macos-update) | The morning after an OS update |
| 6 | [`--stop-on-entry` reports `exception`](#6---stop-on-entry-reports-exception-not-entry) | Reading a stop reason |
| 7 | [`eval` with context `repl` runs an LLDB command](#7-eval-with-context-repl-runs-an-lldb-command) | Evaluating an expression |
| 8 | [Breakpoints never bind under `/tmp` on macOS](#8-breakpoints-never-bind-under-tmp-on-macos) | Writing a scratch program |

## 1. A symlink on `PATH` breaks liblldb resolution

**Symptom.** Any codelldb invocation panics on startup:

```text
thread 'main' panicked at src/codelldb/bin/main.rs:56:49:
called Result::unwrap() on an Err value:
"dlopen(/Users/you/.local/lldb/lib/liblldb.dylib, ...) (no such file)"
```

The path it names does not exist. The real `liblldb.dylib` is at
`~/.local/opt/codelldb/extension/lldb/lib/`.

**Cause.** codelldb finds `liblldb` at runtime by taking `argv[0]`, stripping the basename,
and appending `../lldb/lib/liblldb.dylib`. Invoked through a symlink, `argv[0]` is the symlink
path, so the walk starts one directory too high. There is no `@rpath` involved to save it —
the lookup is a hand-rolled `dlopen`.

**Fix.** A wrapper script, not a symlink:

```bash
cat > ~/.local/bin/codelldb <<'EOF'
#!/usr/bin/env bash
exec "$HOME/.local/opt/codelldb/extension/adapter/codelldb" "$@"
EOF
chmod +x ~/.local/bin/codelldb
```

`exec` with an absolute path sets `argv[0]` to the real binary, and the walk lands correctly.
This is the same thing Mason's codelldb install does.

## 2. Silent on stderr without `RUST_LOG`

**Symptom.** Spawn `codelldb --port 0` and read its stderr to learn the port. Nothing ever
arrives, though the process is running and has opened a listener.

**Cause.** codelldb logs through `tracing`, which emits nothing unless `RUST_LOG` sets a
level. "Loaded liblldb" is `INFO`; "Listening on" is `DEBUG`.

**Fix.** Set `RUST_LOG=debug` when spawning it. lazydap does this for you; it matters if you
are writing your own adapter.

## 3. DAP over TCP, not stdio

**Symptom.** You expect a DAP adapter to speak the protocol over stdin and stdout, the way a
language server does. codelldb does not.

**Cause.** `codelldb --port N` opens a TCP listener and waits for one connection. `--port 0`
picks a free port and announces it on stderr, gated by quirk 2. The child's stdio carries logs
only.

**Fix.** Nothing to fix — it is the design. It does mean an adapter implementation has to
spawn, scrape the port out of stderr, then connect. Other adapters differ: debugpy speaks DAP
over stdio directly, which is why the adapter seam exists.

## 4. `--version` is not a flag

```console
$ codelldb --version
error: unexpected argument '--version' found
```

Use `--help` to check it runs. For the actual version, read `package.json` in the extension
directory, or the `lldb version` string in the `RUST_LOG=info` load line.

## 5. Every invocation hangs after a macOS update

**Symptom.** Even `codelldb --help` hangs forever with no output. `--port 0` never listens.
`sample <pid>` shows every sample at `_dyld_start` — the process never reaches `main`.
`codesign -v` passes and there is no quarantine attribute.

To lazydap this looks like `NoPortFromAdapter`, which is indistinguishable from quirk 1 until
you run `codelldb --help` by hand.

**Cause.** macOS keeps a per-inode security evaluation record for launched binaries. An OS
update can leave it wedged, after which dyld blocks forever for that one file. The proof:
copying the binary to a new path makes the copy run instantly while the original still hangs
— same bytes, same signature.

**Fix.** Re-copy the install so every file gets a fresh inode:

```bash
rm -rf ~/.local/opt/codelldb
cp -R <source>/extension ~/.local/opt/codelldb/extension
timeout 8 codelldb --help    # must print usage and exit 0
```

## 6. `--stop-on-entry` reports `exception`, not `entry`

**Symptom.** Launching with `--stop-on-entry` does stop the program before `main`, but the
adapter says the reason was an exception, with `"description": "signal SIGSTOP"`.

**Cause.** codelldb implements entry-stop by starting the process and immediately sending it
`SIGSTOP`. LLDB files signal stops under exceptions, and codelldb reports what LLDB tells it.
DAP's `entry` reason is not used by this adapter on macOS at all.

**Fix.** lazydap normalises it and shows you both:

```json
{ "raw_reason": "exception", "reason": "entry", "state": "paused" }
```

Branch on `reason`. Read `raw_reason` when you need to know what the adapter really said.
Reporting only the normalised value would be a tidy lie; reporting only the raw one would make
every caller learn this quirk.

## 7. `eval` with context `repl` runs an LLDB command

**Symptom.** Evaluating `x`, where `x` is an `int` holding 5, fails with a *memory read* error.
Evaluating `x + 2` fails with *invalid start address expression*.

**Cause.** DAP's `evaluate` takes a `context` field. codelldb reads `repl` literally — "a line
the user typed at the debug console" — and hands it to LLDB's *command* interpreter rather
than its expression evaluator. `x` is LLDB's built-in alias for `memory read`, so asking for a
variable called `x` reads memory and reads it badly. codelldb announces this at launch, in an
output event nobody reads: `Console is in 'commands' mode, prefix expressions with '?'.`

Measured against the same paused program:

| `--context` | `eval "x"` | `eval "x + 2"` |
|---|---|---|
| `repl` | error (memory read) | error (address expression) |
| `watch` | `5` (`int`) | `7` (`long long`) |
| `hover` | `5` (`int`) | `7` (`long long`) |

**Fix.** `lazydap eval` defaults to `--context watch`, so you get expression evaluation
without asking. `--context repl` is still there for running an adapter command deliberately,
which with codelldb is a real feature.

Prefixing with `?` as the console suggests was rejected: it is codelldb-specific syntax
leaking into what a caller types, and it would make `lazydap eval` mean different things
depending on the adapter.

## 8. Breakpoints never bind under `/tmp` on macOS

**Symptom.** A scratch program in `/tmp`, a breakpoint on a line you know is reached, and the
program runs straight to completion. `break` looks like it worked. `launch` says so in a
`message` that reads like success:

```json
{
  "message": "Breakpoint at /private/tmp/demo/hello.c:6 could not be resolved, but a valid location was found at /tmp/demo/hello.c:6",
  "verified": false
}
```

Then `continue --wait` returns `"state": "exited"` with `"hit_breakpoint_ids": []`.

The two paths differ only by `/private`, which is easy to read past. **`verified: false` is
the real signal.**

**Cause.** On macOS `/tmp` is a symlink to `/private/tmp`, and three components disagree about
the file's name. lazydap canonicalises, so it sends `/private/tmp/...`. The compiler records
the path as typed on its command line, so DWARF says `/tmp/...`. codelldb compares the two as
strings without resolving either. Neither canonicalising nor not canonicalising is wrong;
disagreeing is.

Not specific to `/tmp` — any symlinked directory on the way to a source file does it. `/var`
and `/etc` are symlinks on macOS too.

**Fix.** Keep debuggees out of `/tmp`. Anywhere under `$HOME` has no symlink on the path and
the problem does not arise.

If you must work under a symlinked path, compile with the resolved path so the debug info
matches what lazydap will send:

```bash
gcc -g -O0 /private/tmp/scratch/hello.c -o /private/tmp/scratch/hello
```

And read `verified` rather than assuming. A script that stops on `verified: false` is spared
the confusion; one that ignores it debugs a program that never pauses.

## See also

- [Troubleshooting](/troubleshooting/) — the same material, by symptom
- [Install](/getting-started/install/) — the wrapper-script install that avoids quirk 1
- The repository's `docs/reference/codelldb-quirks.md` for the forensic write-ups
