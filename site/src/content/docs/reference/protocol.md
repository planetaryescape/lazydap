---
title: Protocol
description: The wire format for writing a lazydap client without going through the CLI.
---

Write a client by connecting to the daemon's Unix socket and exchanging length-delimited JSON
frames. The CLI, the TUI and the agent skill are all clients of this and get no privileges you
do not — that is the point of the socket existing.

You do not need this to use lazydap. You need it to build a frontend: an editor plugin, an
MCP bridge, a web UI, language bindings.

:::note[Every frame on this page is generated]
The JSON below is pasted from `cargo run -p lazydap-protocol --example wire_examples`, which
serialises the real types. Hand-written serde shapes get subtly wrong in ways that are
answered `BadRequest` rather than diagnosed, so none of these were written by hand. Re-run it
after changing `crates/protocol/src/types.rs`.
:::

## Connect

The socket is at `{runtime_dir}/lazydap-{instance}.sock`. Find it without guessing:

```console
$ lazydap logs --limit 1 --format json
{
  "lines": [
    "2026-07-30T20:34:44.459987Z  INFO daemon.ipc: daemon listening instance=lazydap-demo-13cc8efcde46 socket=/tmp/lazydap-501/lazydap-lazydap-demo-13cc8efcde46.sock pid=43070"
  ]
}
```

`LAZYDAP_RUNTIME_DIR` moves the directory the socket lives in; `LAZYDAP_INSTANCE` changes
which socket in it. There is no variable that sets the full socket path. The rest of the
environment lazydap reads is `LAZYDAP_DATA_DIR`, `LAZYDAP_LOG` and `LAZYDAP_TIMEOUT`. See
[the daemon guide](/guides/daemon/).

The daemon may not be running. A client is expected to spawn one, the way the CLI does, or to
require the user to run any lazydap command first.

## Framing

Each frame is a **4-byte big-endian length prefix followed by that many bytes of JSON**.

```text
+----------------+---------------------------+
| u32 big-endian | JSON body, `length` bytes |
+----------------+---------------------------+
```

Frames larger than **16 MiB** are refused. A client claiming a 4 GiB message is broken or
hostile, and the daemon should not allocate for it either way.

A frame the daemon cannot read **ends the connection**. It replies `BadRequest` on id `0` —
id `0` because an unreadable frame has no id to answer in context — and then hangs up, rather
than leaving you waiting for a reply that could never correlate. Reconnect and send a
well-formed frame; there is nothing to recover on the old connection.

## The envelope

```json
{
  "version": 10,
  "id": 1,
  "payload": {
    "Request": "Ping"
  }
}
```

| Field | Notes |
|---|---|
| `version` | `LAZYDAP_PROTOCOL_VERSION`. Read it from `crates/protocol/src/types.rs` — that constant is the source of truth, and this page will drift |
| `id` | Correlates a response with its request. Monotonic per connection |
| `payload` | One of `Request`, `Response`, `Event`, `Error` |

**Events use `id: 0`**, because nobody asked for them. Anything non-zero is answering
something you sent.

:::caution[A request with no fields is a bare string]
`{"Request": "Ping"}`, not `{"Request": {"Ping": null}}`. serde writes unit variants as plain
strings, and the wrapped form is rejected. The same goes for `Status`, `Shutdown`, `Version`,
`Doctor` and `BreakpointList`.
:::

A request that does have fields looks like this:

```json
{
  "version": 10,
  "id": 5,
  "payload": {
    "Request": {
      "Disconnect": {
        "session_id": "1807b262-de22-43bd-9158-f7237f0981c8",
        "terminate": true,
        "dry_run": false
      }
    }
  }
}
```

`Doctor` used to be that example. It carried `check_adapters` and `check_state` until protocol
v10 took them off. Both checks moved into the client in v0.2.3 — it is the process whose `PATH`
and working directory they actually describe — the daemon stopped reading the fields in v0.2.8,
and removing them changes the frame's shape, which is what that bump is for.

## Handshake

Send `Ping` first. The daemon answers `Pong` with its version, instance and uptime. A
mismatch means you are talking to a daemon from a different build, and it says
`VersionMismatch` rather than half-speaking an older dialect.

```bash
lazydap shutdown    # what a user does about a mismatch
```

:::caution[`Shutdown` must never gain fields]
`Request::Shutdown` is frozen. It is the escape hatch a new client uses to stop an old daemon,
and it is version-exempt on the server. Adding a field turns the wire form from `"Shutdown"`
into `{"Shutdown":{...}}`, which an older daemon rejects at parse time — before the exemption
can apply. This has broken twice.

`lazydap shutdown --dry-run` is therefore built out of a `Status` call rather than a flag on
`Shutdown`.
:::

## Every request

Twenty-one variants, grouped by the four buckets. Adding a fifth bucket needs a decision-log
entry.

| Bucket | Variants |
|---|---|
| **Diagnostics** | `Ping`, `Status`, `Shutdown`, `Version`, `Doctor` |
| **Session lifecycle** | `Launch`, `Disconnect` |
| **Stepping** | `Continue`, `Step`, `Pause` |
| **Inspection** | `Threads`, `StackTrace`, `Scopes`, `Variables`, `Eval`, `Output` |
| **Breakpoints** (project state; no session needed) | `BreakpointList`, `BreakpointAdd`, `BreakpointRemove`, `BreakpointToggle` |
| **Subscription** | `Subscribe` |

Three things a reader of the CLI might expect here and not find:

- **No `StepIn` or `StepOut`.** One `Step` request carries `kind`, which serialises as
  `"over"`, `"in"` or `"out"`.
- **No `SetBreakpoints`.** Breakpoints are project state, edited through the four
  `Breakpoint*` requests.
- **No `Logs`.** `lazydap logs` reads the daemon's log file directly rather than asking the
  daemon for it.

`ClientSpecific` is a design category for state that never leaves a client — pane focus,
scroll offsets — not a request variant. Nothing sends it and the daemon has no case for it.

## Launching and stepping

```json
{
  "version": 10,
  "id": 6,
  "payload": {
    "Request": {
      "Launch": {
        "adapter": "codelldb",
        "program": "/Users/you/lazydap-demo/hello",
        "args": [],
        "cwd": "/Users/you/lazydap-demo",
        "env": {},
        "stop_on_entry": true,
        "adapter_command": null
      }
    }
  }
}
```

`Launch` has no wait mode. It answers `Response::Launched` once the configuration phase is
done, and that reply has its own shape — session id, state, capabilities, and the persisted
breakpoints with whatever the adapter made of them. It is not a `StableState`.

Stepping requests do have a wait mode:

```json
{
  "version": 10,
  "id": 7,
  "payload": {
    "Request": {
      "Continue": {
        "session_id": "1012b692-01c4-48e0-b8e1-4124163bafe3",
        "thread_id": null,
        "wait": {
          "Wait": {
            "timeout_ms": null
          }
        },
        "all_threads": false
      }
    }
  }
}
```

`"wait": {"Wait": {"timeout_ms": null}}` blocks with the daemon's 30-second default;
`timeout_ms: 0` means no timeout at all. Not waiting is the bare string `"NoWait"`:

```json
{
  "version": 10,
  "id": 8,
  "payload": {
    "Request": {
      "Step": {
        "session_id": "99ede8a7-3727-4b36-8da9-a84bda1da86c",
        "thread_id": null,
        "kind": "over",
        "wait": "NoWait"
      }
    }
  }
}
```

## Responses

Named variants rather than a generic envelope: `Pong`, `Status`, `Version`, `Doctor`,
`Launched`, `Disconnected`, `ShuttingDown`, `Continued`, `Stepped`, `Threads`, `StackTrace`,
`Scopes`, `Variables`, `Evaluated`, `Output`.

The one worth knowing about: a stepping request **without** a wait is answered `Continued`
(the request was accepted), and **with** one is answered `Stepped`, carrying the full
stable-state object. Same request, different reply, chosen by the wait mode. The shape of
that object is on the [JSON output](/reference/json-output/) page — the CLI prints it through
without reshaping it.

## Events

Subscribe by sending `Subscribe { channels: [...] }`. **Channel names are snake_case on the
wire**, even though the Rust variants are not:

```json
{
  "version": 10,
  "id": 9,
  "payload": {
    "Request": {
      "Subscribe": {
        "channels": [
          "stopped",
          "output",
          "session_ended"
        ]
      }
    }
  }
}
```

The seven kinds, exactly as they serialise: `session_started`, `session_ended`, `stopped`,
`continued`, `output`, `breakpoint_updated`, `thread_changed`.

Three things to know:

**It is answered with a `Status` response**, not a variant of its own. The snapshot is taken
at the instant the subscription starts, so there is no gap between "what is the state now" and
"tell me when it changes" for an event to fall into.

**Nothing buffered is replayed.** You get what happens from now on. `Request::Output` reads
the debuggee's earlier output without draining it.

**Subscribing again replaces the set**, rather than adding to it.

Events then arrive as ordinary frames with `id: 0`, interleaved with replies to your own
requests. A client must be able to read a frame it did not ask for at any point.

```json
{
  "version": 10,
  "id": 0,
  "payload": {
    "Event": {
      "Continued": {
        "session_id": "99ede8a7-3727-4b36-8da9-a84bda1da86c",
        "thread_id": 27982711,
        "all_threads_continued": true
      }
    }
  }
}
```

`BreakpointUpdated` is the one whose shape changed in v3, and it is why the version went up.
Its `session_id` is optional: `Some` is an adapter's opinion about a live session, `None` is a
change to the project's list, which `lazydap break` can make with nothing running.

```json
{
  "version": 10,
  "id": 0,
  "payload": {
    "Event": {
      "BreakpointUpdated": {
        "session_id": "4234e458-358b-4ba4-bf10-719a8653b0ea",
        "breakpoint": {
          "id": 1,
          "adapter_id": 1,
          "verified": true,
          "line": 6,
          "message": "Resolved locations: 1"
        }
      }
    }
  }
}
```

```json
{
  "version": 10,
  "id": 0,
  "payload": {
    "Event": {
      "BreakpointUpdated": {
        "session_id": null,
        "breakpoint": {
          "id": 2,
          "verified": false,
          "line": 6
        }
      }
    }
  }
}
```

Note what the second frame leaves out. `adapter_id` and `message` are skipped when absent
rather than sent as `null`, so a client must treat every field but `verified` as optional.

Both shapes would have claimed to be v2 and failed to decode each other, which is the hazard
the version number exists to turn into a clean `VersionMismatch` and a restart.

This is exactly what [the TUI](/getting-started/tui/) does: `Subscribe` to the session events,
then send the same `Continue`, `Step` and breakpoint requests the CLI sends.

## Failure modes to handle

| What happens | What you see |
|---|---|
| The adapter dies | The debuggee is killed first, then a synthetic `SessionEnded` carrying a `detail` string that says so; any in-flight wait resolves `adapter_died` |
| The daemon dies | Socket EOF. Retry `Ping`, then spawn a daemon |
| You send an unreadable frame | `BadRequest` on id `0`, then the connection closes |
| The adapter is missing | `AdapterNotFound`, with the searched paths in `details` |

Error payloads carry a `code` from a fixed set, a human `message`, and free-form `details`
(`{}` when there is nothing to add). The codes are listed on
[errors and exit codes](/reference/errors/).

## One execution request at a time

At most one of `Continue`, `Step` or `Pause` is in flight per session; the daemon queues the
rest. Inspection requests run in parallel.

Do not work around this by opening a second connection. Pipelining execution requests to one
adapter deadlocks when the request you are waiting on is blocked on an event that a queued
request would have caused.

## See also

- [Architecture](/guides/architecture/) — where this sits in the stack
- [The daemon](/guides/daemon/) — lifecycle and paths
- [JSON output](/reference/json-output/) — the payload shapes
