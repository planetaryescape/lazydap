---
title: The daemon
description: How the per-project daemon starts itself, where it keeps its files, and how to read its log.
---

You never start the daemon. The first command that needs one spawns it, and it stays up
between commands so the next one finds the program where the last one left it.

```console
$ lazydap status --format json
{
  "daemon_pid": 2452,
  "instance": "lazydap-demo-13cc8efcde46",
  "lazydap_version": "0.1.0",
  "protocol_version": 2,
  "session": {
    "adapter": "codelldb",
    "buffered_events": 11,
    "captured_output_chunks": 5,
    "dropped_events": 0,
    "exit_code": null,
    "program": "/Users/you/lazydap-demo/hello",
    "session_id": "3a2c4335-0af9-4363-bf2e-2dc866c9045b",
    "state": "paused",
    "uptime_ms": 41508
  },
  "uptime_ms": 45819
}
```

## Why there is one at all

A debug session is a live process with live state. If each `lazydap` invocation owned it, the
session would die with the command and `lazydap stack` would have nothing to look at.

The daemon holds the adapter process and the session, so every subcommand can be a separate,
short-lived process and still talk about the same paused program. That is what makes the CLI
scriptable at all.

## One per project

The instance is derived from your project root, found by walking up from the working directory
for the first of `.lazydap/`, then `.git/`, then a language manifest. Two projects get two
daemons and cannot see each other's sessions.

The name is a readable slug plus a hash — `lazydap-demo-13cc8efcde46`. The slug is for
recognising it in `ps`; the hash is what actually keeps two directories apart.

Override it with `--instance NAME` or `LAZYDAP_INSTANCE`. Useful for running two sessions in
one project, given the one-session-per-daemon limit:

```bash
LAZYDAP_INSTANCE=api    lazydap launch ./api
LAZYDAP_INSTANCE=worker lazydap launch ./worker
```

## Where the files are

| What | Where |
|---|---|
| Socket | `{runtime_dir}/lazydap-{instance}.sock` |
| Lock | `{runtime_dir}/lazydap-{instance}.lock` |
| PID | `{data_dir}/lazydap-{instance}.pid` |
| Log | `{data_dir}/lazydap-{instance}.log` |
| Breakpoints | `.lazydap/state.toml`, in the project |

`runtime_dir` is `$LAZYDAP_RUNTIME_DIR`, else the platform runtime directory, else
`/tmp/lazydap-{uid}`. macOS has no `XDG_RUNTIME_DIR`, and its per-user temp directory eats
most of the 104-byte limit a Unix socket path gets, so `/tmp` is deliberate rather than lazy.

`data_dir` is `$LAZYDAP_DATA_DIR`, else the platform data directory.

Both are created `0700` and their ownership is checked before use. That check is not
ceremony: anything that can bind that socket can accept a `launch`, and a `launch` runs a
program.

## Reading the log

```console
$ lazydap logs --format json
{
  "lines": [
    "2026-07-30T20:34:44.459987Z  INFO daemon.ipc: daemon listening instance=lazydap-demo-13cc8efcde46 socket=/tmp/lazydap-501/lazydap-lazydap-demo-13cc8efcde46.sock pid=43070",
    "2026-07-30T20:34:44.516231Z  INFO daemon.session: launching session_id=8a2b018f-ecbf-4602-aaf3-17a022dc1220 program=/Users/you/lazydap-demo/hello stop_on_entry=false",
    "2026-07-30T20:35:03.129498Z  WARN daemon.ipc: request failed request_id=3 error=DapProtocolError: the adapter rejected `variables`: Internal debugger error: Invalid variabes reference"
  ]
}
```

Lines elided. That last one is a real stale-reference mistake, recorded at the moment it
happened — the log is the first place to look when a command failed and the message was too
short to explain why.

```bash
lazydap logs --level warn        # warnings and louder
lazydap logs --limit 20          # last 20 lines, default 200
lazydap logs --follow            # keep printing
lazydap logs --purge             # delete the file
```

## Stopping and restarting it

```bash
lazydap disconnect     # end the session, leave the daemon up
lazydap shutdown       # stop the daemon and every session it owns
```

Nothing needs starting afterwards. The next command spawns a fresh one.

`lazydap daemon --foreground` runs it in the foreground instead, which is what you want under
a process manager or when watching it work.

## Version mismatches

The protocol is versioned separately from the binary and is at **v2**. A daemon left running
from an older build refuses new clients with `VersionMismatch` rather than half-speaking an
older dialect:

```bash
lazydap shutdown       # clears it; the next command starts a current daemon
```

This is the usual symptom right after a rebuild.

## When it dies

Clients notice the socket has gone, retry once after 100 ms, and spawn a new daemon. What
survives is `.lazydap/state.toml` — your breakpoints. What does not is the live session: the
program it was debugging is gone and you launch again.

[The TUI](/getting-started/tui/) does not yet reconnect after this; restart it.

## See also

- [`lazydap status`](/reference/cli/status/), [`lazydap logs`](/reference/cli/logs/), [`lazydap shutdown`](/reference/cli/shutdown/)
- [Architecture](/guides/architecture/) — what the daemon owns and what it refuses to
- [Protocol](/reference/protocol/) — talking to it without the CLI
