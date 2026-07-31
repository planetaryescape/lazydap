# M24 — Attach to a running process

## What

`lazydap attach <pid>` (and `attach --port <n>` where an adapter supports gdbserver-style
remote attach): connect the debugger to an already-running process instead of launching one,
then the same loop — break, `continue --wait`, stack, scopes, eval, watches, TUI marker.

## Why

This is the milestone the target segment (see
[`docs/blueprint/00-overview.md`](../../blueprint/00-overview.md) § "The segment that actually
needs this") needs most and no competitor in the 2026 cluster has. A long-running native
process misbehaving *right now* — a daemon eating memory, a server wedged on a lock, a
reproduction that took an hour to set up — is the canonical debugger-or-nothing moment.
Launch-only cannot touch it. Chosen over js-debug (M23) on that basis.

## How

1. **Protocol + core:** attach is a launch-class operation with a different request shape
   (`AttachRequest { pid | port, adapter, ... }`). Decide: new `Request::Attach` variant
   (protocol bump, additive-but-not-decode-compatible per the D043/D056/M22 reasoning — the
   Shutdown escape hatch stays frozen) vs. an attach mode on the existing Launch. Spike which
   the adapters actually want; record as a D-entry.
2. **The trait:** `DebugAdapter` gains an attach path alongside launch. codelldb: DAP
   `attach` request with `pid` (or `program` + `pid`); debugpy: `attach` with
   `connect`/`processId` — debugpy's attach story is the involved one, spike it. delve:
   `dlv attach <pid>` / `dlv dap` attach mode. Each adapter reports which attach modes it
   supports; unsupported → honest error.
3. **D045 interaction is the sharp edge:** a process we ATTACHED to must **never be reaped** —
   we did not launch it, killing it on adapter death would destroy the user's running service.
   D045's launch-only guard already anticipates this ("attach must never record a pid to
   reap") — verify it holds and add attach-specific tests: attach, kill the adapter, assert
   the debuggee SURVIVES. This is the inverse of every reap test so far and the most important
   assertion in the milestone.
4. **Detach semantics:** `disconnect` after an attach must leave the process running by
   default (`terminateDebuggee: false`), with an explicit `--terminate` to kill it. The
   opposite default from launch. Wire it and test both.
5. **CLI + TUI + launch.json:** `lazydap attach <pid> [--adapter] [--format json]`; the TUI
   can attach; `.vscode/launch.json` `"request": "attach"` configs become runnable (they are
   currently listed not-runnable with a reason — flip them, adapter by adapter).
6. **Fixtures:** a long-running C fixture (a loop that sleeps) and a Python one; the test
   attaches to an already-spawned process, breaks it, inspects, detaches leaving it alive,
   then the test kills it.
7. **Platform reality:** attach needs ptrace permissions — macOS may prompt or require the
   debuggee be the user's own; Linux `ptrace_scope`. Document the failure mode (an honest
   "cannot attach: permission denied — see …") rather than a mysterious hang. Spike the
   actual macOS behaviour before claiming it works.

## Success criteria

- `lazydap attach <pid>` on a running C fixture: pauses it, `stack`/`eval` work, `continue
  --wait` resumes, `disconnect` leaves it **running**, `disconnect --terminate` kills it.
- Adapter-death-does-not-reap-an-attached-process test passes (the inverse-of-D045 assertion).
- At least codelldb + one of debugpy/delve; the others report unsupported attach honestly
  rather than misbehaving.
- launch.json attach configs runnable for supported adapters.
- Zero strays from the test's OWN spawned fixtures (which the test, not lazydap, must clean up).

## Depends on

- M22 (delve) merged — the third adapter settles the trait's attach surface across TCP+stdio.
- D045 launch-only reap guard (shipped) — this milestone stress-tests its inverse.

## Notes

- Created 2026-07-31. Chosen over M23 (js-debug) because it serves the target segment and
  closes a gap no competitor fills — user decision, segment-anchored.
- The one place launch's and attach's defaults deliberately diverge: launch kills its child on
  disconnect, attach leaves it alone. Keep that asymmetry loud in code and docs; it is a
  data-loss boundary (kill someone's live service by accident and you have burned trust).
