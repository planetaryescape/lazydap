# M22 — Third adapter: delve (Go)

## What

Go debugging through Delve's DAP mode, behind the `DebugAdapter` trait (D052), reaching
parity with the codelldb/debugpy workflows: break, launch, `continue --wait`, stack, scopes,
eval, watches, TUI marker.

## Why

The competitive audit (docs/research/2026-07-31) found the nearest rival ships dlv and
js-debug; the user's call: "let's learn from debug-skill and create the adapters they have
that we don't." Delve first — it speaks DAP natively and its headless mode is the
best-documented of any debugger.

## How

1. `dlv dap --listen=127.0.0.1:0` serves DAP over TCP, one session per process —
   `Spawn::Tcp` like codelldb, but the port comes from dlv's stdout announcement (verify the
   exact line live before coding; do not trust this file). Discovery: config pin > PATH `dlv`
   + a liveness probe (`dlv version`).
2. Adapter selection: `.go` extension → delve compiles-and-debugs via its `debug` launch
   mode; a compiled Go binary stays codelldb-able but `--adapter delve` must win. Check what
   launch request shape dlv dap wants (`mode: debug|exec|test`) — support `debug` and `exec`
   minimum, `test` if cheap.
3. Entry stop, `process` event, exited/terminated ordering, reverse requests: spike first
   with a raw transcript (the debugpy spike pattern) and record every deviation as
   quirks/D-entries. Known from research: dlv dap is single-use per connection — the daemon
   must spawn per session, not reuse.
4. `examples/go-hello/` fixtures mirroring c-hello/py-hello; a serialized `wait_delve.rs`
   suite with the static-mutex + stable-fixture-path discipline.
5. launches import: `type: "go"` configs map to delve (runnable), and the not-runnable
   reason for Go configs is deleted.
6. Docs: adapter page updates (README scope line, why-lazydap when-to-use-else, install page
   gets a dlv prerequisite note), skill regeneration, quirks file `docs/reference/delve-quirks.md`.

## Success criteria

The full agent loop on a Go fixture, live: break, launch, continue --wait breakpoint blob
with captured stdout, eval, exit blob; watches evaluate across a stop; TUI marker lands;
AdapterNotFound (exit 4) with an install hint when dlv is missing; zero strays including Go
debuggees (D045 reap verified for dlv's process model — spike whether dlv kills the debuggee
on adapter death like debugpy or orphans it like codelldb).

## Depends on

- D052 trait (shipped, M18). Go toolchain + dlv on this machine (install user-local if
  absent; record versions).
