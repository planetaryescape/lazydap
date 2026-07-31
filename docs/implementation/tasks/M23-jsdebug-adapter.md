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
