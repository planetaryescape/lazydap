---
title: Architecture
description: The three protocols lazydap stacks, and the crate graph that keeps clients honest.
---

lazydap is three protocols stacked. Knowing which layer a failure is on tells you which log to
read, which is most of what this page is for.

```text
 you / a script / an agent / the TUI
            │
            │  lazydap protocol  (length-delimited JSON over a Unix socket)
            ▼
        lazydap daemon
            │
            │  DAP  (Content-Length-framed JSON)
            ▼
      codelldb  (the adapter)
            │
            │  ptrace / native debug APIs
            ▼
        your program
```

You only ever see the top arrow. The other two are the daemon's problem.

## Who owns what

**The daemon** owns the adapter process, the session, the DAP request queue, the event pump,
and the per-project state file. It is the only thing that speaks DAP, the only thing that
touches the adapter, and the only thing that writes `.lazydap/state.toml`.

**Clients** — the CLI, the TUI, an agent, anything you write — connect to a socket and send
requests. They hold no session state that the daemon does not also hold, so a client can be
killed and restarted without the debugger noticing.

**The adapter** owns the program being debugged. lazydap does not parse DWARF or call ptrace;
codelldb and LLDB do that.

## The crate graph is the architecture

Seven crates, with the dependency arrows chosen so that the rules cannot be broken by someone
who disagrees with them:

Straight from `cargo metadata`:

| Crate | Depends on | Is |
|---|---|---|
| `lazydap-core` | nothing internal | Domain types and errors. Zero I/O. |
| `lazydap-dap` | nothing internal | Raw DAP framing. Knows nothing about lazydap. |
| `lazydap-config` | nothing internal | Paths, project-root detection. |
| `lazydap-protocol` | `core` | The IPC contract, so a client can speak it without the daemon. |
| `lazydap-store` | `core` | `.lazydap/state.toml`. No sockets, no adapters. |
| `lazydap-tui` | `core`, `protocol` | A client. Nothing more. |
| `lazydap-daemon` | all six | The `lazydap` binary, the daemon, the CLI. |

The load-bearing row is `lazydap-tui`. It cannot depend on the daemon, the store, or DAP —
so a TUI feature that reaches into daemon-private state is not something a developer can
write here even if they want to. It has to become a protocol request first, at which point
the CLI can send it too.

That is why "every TUI action has a CLI equivalent" is a property of the build rather than a
promise in a contributing guide. `scripts/check_architecture_boundaries.sh` runs in CI and
fails on a manifest that adds a forbidden arrow.

The daemon crate does depend on the TUI, because the daemon crate is also the `lazydap`
binary and therefore the thing that launches the TUI. The arrow only points that way; the
reverse is the bypass the table forbids.

:::note
Older copies of `ARCHITECTURE.md` and the blueprint show eleven crates, including a separate
one per adapter. Those were planned and not built that way: codelldb lives inside the daemon
at `crates/daemon/src/adapter/`. The seven above are what exists.
:::

## Requests come in four kinds

Every protocol request classifies into one bucket, and adding a fifth needs a decision-log
entry rather than a pull request:

1. **Session** — things that need a live session: `Launch`, `Continue`, `Step`,
   `SetBreakpoints`, `Eval`, `StackTrace`, `Scopes`, `Variables`, `Disconnect`.
2. **Project** — things that read or write `.lazydap/state.toml`, and work with no session
   running: breakpoint add, list, remove, toggle.
3. **Diagnostics** — `Ping`, `Status`, `Logs`, `Doctor`, `Version`.
4. **ClientSpecific** — pane state, scroll offsets. Never leaves the client; the daemon never
   sees them. There are none today.

The split is what makes "set a breakpoint before launching anything" work: it is a bucket-2
request and has no session to need.

## Three failure surfaces

| Symptom | Layer | Where to look |
|---|---|---|
| A lazydap command errors with a JSON error name | lazydap protocol | `lazydap logs` |
| The adapter rejected something, or a breakpoint will not bind | DAP | `lazydap logs`, then [codelldb quirks](/reference/codelldb-quirks/) |
| The program will not start, or symbols are missing | native | The adapter's own output in `captured_output` |

Most confusing failures are the middle row, and most of those are already written down.

## What is deliberately not supported

- **Cross-machine sessions.** Tunnel codelldb's own `--port` mode and run lazydap next to it.
- **Two adapters in one daemon.** Run two daemons with different `--instance` names.
- **State surviving a daemon restart**, beyond `.lazydap/state.toml`. A session is live
  process state and does not serialise honestly.

## See also

- [The daemon](/guides/daemon/) — lifecycle, paths, logs
- [Protocol](/reference/protocol/) — the wire format, for writing a client
- [Why lazydap](/guides/why-lazydap/) — why the CLI rather than the TUI is the product
