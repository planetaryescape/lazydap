# DAP protocol cheatsheet

> **New to debuggers?** Read [`how-debuggers-actually-work.md`](how-debuggers-actually-work.md) first. It explains where DAP sits in the stack and what's actually happening below it (ptrace, DWARF, INT3, etc.). This doc assumes you already know that DAP is just the IDE-to-adapter protocol.



Quick reference for the DAP messages lazydap actually uses. Full spec at [microsoft.github.io/debug-adapter-protocol/specification.html](https://microsoft.github.io/debug-adapter-protocol/specification.html).

## Wire format

```
Content-Length: 119\r\n\r\n
{"seq":1,"type":"request","command":"initialize","arguments":{...}}
```

Header(s) followed by `\r\n\r\n` then exactly `Content-Length` bytes of UTF-8 JSON. Same as LSP.

## Message types

### Request

```json
{
  "seq": 1,
  "type": "request",
  "command": "initialize",
  "arguments": { ... }
}
```

`seq` is monotonic per session. Lazydap tracks via `AtomicI64`.

### Response

```json
{
  "seq": 2,
  "type": "response",
  "request_seq": 1,
  "command": "initialize",
  "success": true,
  "body": { ... },
  "message": null
}
```

`success: false` means error; `message` carries it.

### Event

```json
{
  "seq": 3,
  "type": "event",
  "event": "stopped",
  "body": { "threadId": 1, "reason": "breakpoint", ... }
}
```

No `request_seq`. Push-based.

## The 15 messages lazydap uses (v0.1)

### Lifecycle

#### `initialize` (request → response)

```json
// args
{
  "clientID": "lazydap",
  "clientName": "lazydap",
  "adapterID": "lldb",
  "linesStartAt1": true,
  "columnsStartAt1": true,
  "pathFormat": "path",
  "supportsVariableType": true,
  "supportsVariablePaging": true,
  "supportsRunInTerminalRequest": false,
  "locale": "en-US"
}

// response body: Capabilities { ...flags... }
```

#### `launch` (request → response)

```json
// args (codelldb-flavoured)
{
  "type": "lldb",
  "request": "launch",
  "program": "/path/to/binary",
  "args": [],
  "cwd": "/path",
  "stopOnEntry": false,
  "console": "internalConsole"
}
```

After `launch` request, the adapter sends the `initialized` event when it's ready to receive `setBreakpoints` and `configurationDone`.

#### `attach` (request → response)

```json
// args
{ "type": "lldb", "request": "attach", "pid": 12345 }
```

#### `disconnect` (request → response)

```json
{ "terminateDebuggee": true }
```

#### `configurationDone` (request → response)

No args. Sent after `setBreakpoints` to signal "I'm done configuring, run the program."

### Breakpoints

#### `setBreakpoints` (request → response)

```json
// args
{
  "source": { "path": "/path/to/main.c", "name": "main.c" },
  "breakpoints": [
    { "line": 42, "condition": "x > 5", "logMessage": null }
  ],
  "sourceModified": false
}

// response
{ "breakpoints": [{"verified": true, "line": 42, "id": 1}] }
```

This **replaces** all breakpoints in this source file. To add one, send the full list.

### Threads / frames / scopes / variables

#### `threads` (request → response)

```json
// no args
// response: { "threads": [{ "id": 1, "name": "main" }] }
```

#### `stackTrace` (request → response)

```json
// args
{ "threadId": 1, "startFrame": 0, "levels": 20 }

// response
{
  "stackFrames": [
    {
      "id": 42,
      "name": "main",
      "source": { "path": "/path/main.c", "name": "main.c" },
      "line": 42,
      "column": 1
    }
  ],
  "totalFrames": 1
}
```

#### `scopes` (request → response)

```json
// args
{ "frameId": 42 }

// response
{
  "scopes": [
    { "name": "Locals", "variablesReference": 100, "expensive": false }
  ]
}
```

#### `variables` (request → response)

```json
// args
{ "variablesReference": 100, "filter": null, "start": 0, "count": null }

// response
{
  "variables": [
    {
      "name": "x",
      "value": "5",
      "type": "int",
      "variablesReference": 0
    },
    {
      "name": "buf",
      "value": "char[256]",
      "type": "char[]",
      "variablesReference": 101    // expand for children
    }
  ]
}
```

`variablesReference: 0` = leaf, no children. Non-zero = expandable, send another `variables` request with that reference.

#### `evaluate` (request → response)

```json
// args
{ "expression": "x + y", "frameId": 42, "context": "watch" }

// response
{ "result": "15", "type": "int", "variablesReference": 0 }
```

`context`: `"watch"` | `"repl"` | `"hover"` — affects formatting.

### Stepping

#### `continue` (request → response)

```json
// args
{ "threadId": 1, "singleThread": false }

// response
{ "allThreadsContinued": true }
```

The response is acknowledgement only. The program is now running. Wait for a `stopped` event for the next pause.

#### `next` (request → response) — step over

```json
{ "threadId": 1, "granularity": "line" }
```

#### `stepIn` (request → response)

```json
{ "threadId": 1, "targetId": null, "granularity": "line" }
```

#### `stepOut` (request → response)

```json
{ "threadId": 1, "granularity": "line" }
```

#### `pause` (request → response)

```json
{ "threadId": 1 }
```

### Events

#### `initialized` (event)

No body. Means: adapter is ready for `setBreakpoints` + `configurationDone`.

#### `stopped` (event)

```json
{
  "reason": "breakpoint",       // "step" | "exception" | "pause" | "entry" | "goto" | ...
  "description": "Breakpoint hit",
  "threadId": 1,
  "preserveFocusHint": false,
  "text": null,
  "allThreadsStopped": true,
  "hitBreakpointIds": [1]
}
```

Set on first stopped only when multiple threads pause simultaneously.

#### `continued` (event)

```json
{ "threadId": 1, "allThreadsContinued": true }
```

#### `exited` (event)

```json
{ "exitCode": 0 }
```

#### `terminated` (event)

```json
// body usually empty
{}
```

May arrive without `exited` (e.g. user disconnected). Always means session ends.

#### `output` (event)

```json
{
  "category": "stdout",         // | "stderr" | "console" | "telemetry" | "important"
  "output": "hello\n",
  "source": { "path": "...", "name": "..." },
  "line": null,
  "column": null
}
```

#### `breakpoint` (event)

```json
{
  "reason": "changed",          // | "new" | "removed"
  "breakpoint": { "id": 1, "verified": true, "line": 42 }
}
```

Adapter telling client about breakpoint state changes (e.g. resolved after module load).

#### `thread` (event)

```json
{ "reason": "started", "threadId": 1 }    // | "exited"
```

## The handshake sequence

```
1. Spawn adapter, connect transport.
2. → request: initialize
3. ← response: initialize (with Capabilities)
4. → request: launch     (don't await response yet)
5. ← event: initialized
6. → request: setBreakpoints (per source file)
7. ← response: setBreakpoints (with verification info)
8. → request: configurationDone
9. ← response: configurationDone
10. ← response: launch (NOW, often deferred until configurationDone)
11. ← events: thread, output, ...
12. ← event: stopped (program ran into a breakpoint)
   ... user inspects, calls stackTrace / scopes / variables / evaluate
13. → request: continue
14. ← response: continue
15. ← events: output, ...
16. ← event: terminated (program done)
17. → request: disconnect
18. ← response: disconnect
```

## Capabilities flags

Returned from `initialize`. Defaults to `false` if absent. Lazydap checks before using features.

Most-used:

- `supportsConfigurationDoneRequest` — does the adapter need configurationDone?
- `supportsConditionalBreakpoints` — `condition: "x > 5"` allowed?
- `supportsHitConditionalBreakpoints` — hit count breakpoints?
- `supportsLogPoints` — `logMessage: "x = {x}"` instead of pause?
- `supportsEvaluateForHovers` — eval in hover context allowed?
- `supportsValueFormattingOptions` — eval format options?
- `supportsCancelRequest` — can we cancel a pending request?
- `supportsRestartRequest` — restart vs disconnect-relaunch?

## What the spec doesn't tell you

- **codelldb requires TCP, not stdio.** Use `--port 0`, parse "Listening on port N" from stderr.
- **Adapters may not respond to `launch` until after `configurationDone`.** Don't block on the launch response.
- **`output` events can arrive at any time**, not just after stepping. Keep the read pump running.
- **Breakpoint resolution may move the line.** The response's `breakpoint.line` may differ from the request.
- **`threads` request is expensive on some adapters** (debugpy walks all Python frames). Cache; refresh on `thread` events.
- **`stopped` events for the same thread can arrive multiple times** in pathological adapter behaviour. Don't trust naively; check thread state.

See [`/docs/blueprint/10-async-to-sync.md`](../blueprint/10-async-to-sync.md) §race conditions for the full list.

## Useful real DAP traces

Run with `LAZYDAP_LOG_DAP=1 RUST_LOG=lazydap_dap=trace lazydap launch ./bin` to see every message.
