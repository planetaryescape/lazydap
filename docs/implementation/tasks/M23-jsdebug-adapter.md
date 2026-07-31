# M23 — Fourth adapter: js-debug (Node)

## What

Node.js debugging through vscode-js-debug behind the DebugAdapter trait: the same loop as
the other three adapters, on `.js`/`.ts` programs.

## Why

Same audit and user direction as M22. js-debug is last because it is the hard one: it
orchestrates child sessions through the `startDebugging` reverse request, which lazydap
currently refuses on principle (D053).

## How

1. Spike FIRST, decide scope from evidence: js-debug ships `dapDebugServer.js` (run under
   node, DAP over TCP). Capture a full launch transcript. The known architectural question:
   does a plain `node script.js` launch debug usably in the PARENT session, or does all the
   action happen in a `startDebugging` child? If the child is mandatory, M23's real scope is
   "single-level startDebugging support" — the daemon accepts the reverse request, spawns a
   child session bound to the same lazydap session, and proxies stops/output from it. Record
   the decision as a D-entry BEFORE building; if the scope explodes, stop and report to the
   orchestrator with the transcript.
2. Discovery: js-debug is not a binary on PATH — decide the install story (config pin to a
   dapDebugServer.js path; a managed download is out of scope per D026's unimplemented tier).
   AdapterNotFound message must say exactly how to get it.
3. Fixtures `examples/js-hello/`, serialized `wait_jsdebug.rs` suite, launches import
   (`type: "node"`/`pwa-node"` → runnable), docs + skill + quirks file as in M22.

## Success criteria

Same loop-parity bar as M22 on a Node fixture, or — if the spike shows child-session support
is a larger milestone — a written scope decision with the transcript, approved by the
orchestrator before code. Honesty beats coverage.

## Depends on

- M22 (adapter-addition patterns settled there). Node on PATH.

---

## Spike findings (2026-07-31) — STOPPED AT THE SCOPE GATE, awaiting sign-off

**No adapter code was written.** This file's step 1 says to decide scope from evidence and
stop if it explodes. It does. The evidence is below.

Verified against **js-debug 1.117.0** (`js-debug-dap-v1.117.0.tar.gz`, installed per
CONTRIBUTING to `~/.local/opt/js-debug/src/dapDebugServer.js`), **Node 26.5.0**, macOS arm64.

### The question this file asked, answered

> does a plain `node script.js` launch debug usably in the PARENT session, or does all the
> action happen in a `startDebugging` child?

**All the action happens in the child. The parent session debugs nothing at all.**

Refusing `startDebugging` the way lazydap refuses every reverse request today (D053) gives a
parent session that reaches `launch` success and then does nothing forever:

```text
<<< initialize success
<<< event initialized
>>> setBreakpoints  → {"breakpoints":[{"id":1,"verified":false,"message":"breakpoint.provisionalBreakpoint"}]}
>>> configurationDone → success
<<< event output (console): "/opt/homebrew/bin/node ./main.js\n"
<<< response launch success
<<< REVERSE REQUEST startDebugging {
      "request": "launch",
      "configuration": {"type":"pwa-node","name":"main.js [3633]",
                        "__pendingTargetId":"3dfd67c23218d35ccd4defd3"}
    }
>>> REFUSED
... nothing further, ever ...

VERDICT: stops in the parent session: 0
         debuggee output in the parent session: none
         breakpoint verified: false ("provisional")
```

The program runs to completion unobserved. There is no partial capability to ship: no stop,
no output, no stack, no eval.

### Accepting it makes the whole loop work — with exactly one child

Second spike, answering `startDebugging` with success and opening a **second TCP connection**
to the same server, sending `initialize` + `launch` with the configuration handed back
verbatim (`__pendingTargetId` and all):

```text
[parent] REVERSE startDebugging → ANSWERED success=True
[child0] >>> initialize / launch(configuration verbatim) / setBreakpoints / configurationDone
[child0] === STOP reason='entry' thread=0
[child0] <<< stackTrace: [{"id":0,"name":"global.main","line":5, ...}]
[child0] === OUTPUT [stdout]: 'hello from m23\n'
[child0] === STOP reason='breakpoint' thread=0
[child0] <<< stackTrace: [{"id":18,"name":"global.main","line":8, ...}]
[child0] === OUTPUT [stdout]: 'goodbye y=10\n'
[child0] === TERMINATED
[parent] === TERMINATED {'restart': False}

VERDICT: parent: stops=0 output=[]        children_requested=1
         child0: stops=2 output=2 lines   grandchildren=0
         total DAP connections needed: 2
```

So the boundary is clean: **single-level `startDebugging` is both necessary and sufficient**
for `node script.js`. One child, no recursion. (A program that spawns workers or forks would
request more; that is a larger question and not what this milestone needs.)

### What that costs, concretely

This is the scope proposal, not a decision:

1. **A second DAP connection under one lazydap session.** The child is a separate TCP
   connection with its own `seq` space, its own pump, and its own `AdapterHandle`. D007
   ("one session at a time") is not violated — it is one *lazydap* session — but
   `Session` currently owns exactly one handle, and every inspect/execute path reaches it
   directly.
2. **Routing.** Stops, output, breakpoints, stack, scopes and eval all belong to the child.
   Breakpoints have to be sent to the child (the parent's stay provisional). So the session
   needs a notion of "the handle that is actually debugging", which is the child once one
   exists and the parent before that.
3. **Reversing D053, narrowly.** lazydap refuses reverse requests on principle. This needs
   `startDebugging` to be accepted for js-debug specifically, which is a decision-log entry
   in its own right — it is the first time an adapter asks lazydap for something and gets a
   yes.
4. **Teardown across two connections.** Both sessions get `terminated`; the parent's carries
   `{"restart": false}`. Reaping has to cover both, and D045's reaper needs the child's
   `process` event (D061's `name` handling already covers what it would report).
5. **js-debug listens on IPv6 `::1`, not `127.0.0.1`.** It announces
   `Debug server listening at ::1:57588` on **stdout**. `DapTransport::spawn_tcp` connects to
   a hard-coded `("127.0.0.1", port)` and is refused outright. `TcpSpawn` (D062) already
   carries the marker and the stream; it would need the **host** from the announcement too.
   This part is small and is a genuine prerequisite.

### Recommendation

Treat single-level child-session support as **its own milestone** rather than smuggling it
into an adapter milestone. Adding a fourth adapter is the small half; the multi-connection
session model is the real work, it changes a decision-log entry, and it is the foundation
anything multi-session later stands on. M23 then becomes "js-debug on top of it" and is
comparable in size to M18 and M22.

Nothing here blocks that judgement being made differently — the transcripts above are the
whole basis for it, and they are reproducible with the two scripts described in the M23
report.

### Toolchain, recorded

| thing | version |
|---|---|
| js-debug | 1.117.0 |
| Node | 26.5.0 |
| install | `js-debug-dap-v1.117.0.tar.gz` → `~/.local/opt/js-debug/`, per CONTRIBUTING |

`dapDebugServer.js` takes the port **positionally** (`node dapDebugServer.js 0`), has no
`--version` flag, and silently treats an unrecognised argument as the address to bind — so
`--version` makes it listen on a socket named `--version` rather than printing anything.
