---
title: Protocol
description: The wire format for writing a lazydap client without going through the CLI.
---

Write a client by connecting to the daemon's Unix socket and exchanging length-delimited JSON
frames. The CLI, the TUI and the agent skill are all clients of this and get no privileges you
do not — that is the point of the socket existing.

You do not need this to use lazydap. You need it to build a frontend: an editor plugin, an
MCP bridge, a web UI, language bindings.

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

`$LAZYDAP_SOCKET_PATH` overrides it. `runtime_dir` and how the instance name is derived are
in [the daemon guide](/guides/daemon/).

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

Malformed JSON inside a well-formed frame does not tear the connection down. The daemon
answers `BadRequest` and stays connected, so a client bug is recoverable.

## The envelope

```json
{
  "version": 2,
  "id": 1,
  "payload": { "Request": { "Ping": null } }
}
```

| Field | Notes |
|---|---|
| `version` | `LAZYDAP_PROTOCOL_VERSION`, currently **2** |
| `id` | Correlates a response with its request. Monotonic per connection |
| `payload` | One of `Request`, `Response`, `Event`, `Error` |

**Events use `id: 0`**, because nobody asked for them. Anything non-zero is answering
something you sent.

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

## Requests come in four kinds

Every request classifies into one bucket. Adding a fifth needs a decision-log entry.

1. **Session** — needs a live session: `Launch`, `Continue`, `Step`, `StepIn`, `StepOut`,
   `Pause`, `SetBreakpoints`, `Eval`, `StackTrace`, `Scopes`, `Variables`, `Disconnect`.
2. **Project** — reads or writes `.lazydap/state.toml`, and works with nothing running:
   breakpoint add, list, remove, toggle.
3. **Diagnostics** — `Ping`, `Status`, `Logs`, `Doctor`, `Version`.
4. **ClientSpecific** — pane state, scroll offsets. Never sent; the daemon never sees them.
   There are none today.

## Responses

Named variants rather than a generic envelope: `Pong`, `Status`, `Version`, `Doctor`,
`Launched`, `Disconnected`, `ShuttingDown`, `Continued`, `Stepped`, `Threads`, `StackTrace`,
`Scopes`, `Variables`.

The one worth knowing about: a stepping request **without** `--wait` is answered `Continued`
(the request was accepted), and **with** `--wait` is answered `Stepped`, carrying the full
stable-state object. Same request, different reply, chosen by the wait mode. The shape of that
object is on the [JSON output](/reference/json-output/) page — the CLI prints it through
without reshaping it.

On the wire the wait mode is `WaitMode::Wait { timeout_ms }`, where absent means the daemon
default of 30 seconds and `0` means no timeout at all.

## Events

Subscribe by sending `Subscribe { channels: [...] }`. Kinds: `SessionStarted`, `SessionEnded`,
`Stopped`, `Continued`, `Output`, `BreakpointUpdated`, `ThreadChanged`.

Three things to know:

**It is answered with a `Status` response**, not a variant of its own. The snapshot is taken
at the instant the subscription starts, so there is no gap between "what is the state now" and
"tell me when it changes" for an event to fall into.

**Nothing buffered is replayed.** You get what happens from now on.

**Subscribing again replaces the set**, rather than adding to it.

Events then arrive as ordinary frames with `id: 0`, interleaved with replies to your own
requests. A client must be able to read a frame it did not ask for at any point.

This is exactly what [the TUI](/getting-started/tui/) does: `Subscribe` to the session events,
then send the same `Continue` and `Step` requests the CLI sends.

## Failure modes to handle

| What happens | What you see |
|---|---|
| The adapter dies | A synthetic `SessionEnded`; any in-flight wait resolves `adapter_died` |
| The daemon dies | Socket EOF. Retry `Ping`, then spawn a daemon |
| You send garbage | `BadRequest`, connection stays up |
| The adapter is missing | `AdapterNotFound`, with the searched paths in `details` |

## One execution request at a time

At most one of `Continue`, `Step`, `StepIn`, `StepOut` or `Pause` is in flight per session;
the daemon queues the rest. Inspection requests run in parallel.

Do not work around this by opening a second connection. Pipelining execution requests to one
adapter deadlocks when the request you are waiting on is blocked on an event that a queued
request would have caused.

## See also

- [Architecture](/guides/architecture/) — where this sits in the stack
- [The daemon](/guides/daemon/) — lifecycle and paths
- [JSON output](/reference/json-output/) — the payload shapes
