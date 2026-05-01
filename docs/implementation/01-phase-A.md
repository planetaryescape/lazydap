# 01 — Phase A: see the protocol

**Goal:** understand DAP by talking to it. No daemon, no UI, no abstraction. Each milestone is one small binary.

Phase A is the learning phase. By the end, you've seen real DAP traffic, set a breakpoint, hit it, observed events. You understand what the daemon needs to do because you've done it manually.

## Milestones

- **[M0 — Hello, adapter](tasks/M00-hello-adapter.md)** — spawn codelldb, read raw bytes from stdout, print, exit. ~30 lines.
- **[M1 — Read one message](tasks/M01-read-one-message.md)** — parse `Content-Length` framing, decode one JSON message, pretty-print.
- **[M2 — Initialize handshake](tasks/M02-initialize-handshake.md)** — send `initialize`, parse response, print capabilities.
- **[M3 — Launch and observe](tasks/M03-launch-and-observe.md)** — send `launch` for a hello-world C binary, stream events for 5s, disconnect.
- **[M4 — Pause on breakpoint](tasks/M04-pause-on-breakpoint.md)** — set breakpoint via `setBreakpoints`, observe `stopped` event.

## What you'll have at the end

- A small example binary in `examples/m0_hello_adapter.rs` etc., one per milestone.
- A `crates/dap` crate with the basic transport (Content-Length framing) and request/response types.
- A `examples/c-hello/` C source you can compile with `gcc -g` for use as a debuggee.
- Direct understanding of every DAP message lazydap will use.

## What you won't have yet

- A daemon. (M5.)
- A CLI subcommand surface. (M5/M6.)
- A TUI. (M8+.)
- Robust error handling. (Phase A is exploratory; tighten in Phase B.)

## Phase-level concepts

### The Content-Length framing

Every DAP message is `Content-Length: N\r\n\r\n` followed by N bytes of UTF-8 JSON. Same as LSP. M1 handles this.

### The `seq` field

DAP messages have a monotonic `seq` field. Requests have one. Responses reference the request's seq via `request_seq`. Events have `seq` but no request_seq. The daemon will use this for correlation; in Phase A you can ignore it (we only do one request at a time).

### Capabilities negotiation

`initialize` returns a `Capabilities` object listing what the adapter supports. We don't enforce capabilities checking in Phase A — assume codelldb supports everything we need.

## Risks specific to Phase A

- **codelldb requires TCP, not stdio.** Surprising. M0 handles via `--port 0` parsing. (See [`/docs/blueprint/03-adapters.md`](../blueprint/03-adapters.md).)
- **Cold start ~2–4 seconds.** codelldb loads its Python runtime. Don't conclude something's broken.
- **Tools you may not have.** `gcc -g` for the example binary. `lldb` to verify the binary debugs at all outside lazydap.

## Phase A is done when

- All of M0–M4 boxes ticked.
- You can run `cargo run --example m4_pause_on_breakpoint` and see it pause on a known line.
- You understand every DAP message in the trace and could explain why each is sent in what order.

Then move to Phase B.
