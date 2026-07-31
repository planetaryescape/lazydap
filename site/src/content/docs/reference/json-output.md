---
title: JSON output
description: The shape each command returns under --format json, field by field.
---

`--format json` returns one object or array per command, on a stable schema. Breaking that
schema costs an entry in the project's decision log, which is the difference between this and
the `table` output — that one gets reflowed whenever it reads badly.

Every shape below was captured from a real run. Home directories are rewritten to
`/Users/you`; nothing else is edited.

## Stepping and continuing

`continue`, `step`, `step-in`, `step-out` and `pause`, **with `--wait`**, all return the same
object. `launch` does not take `--wait` and has [its own shape](#launch).

```json
{
  "additional_stopped_threads": [],
  "all_threads_stopped": true,
  "breakpoint_updates": [
    { "adapter_id": 1, "id": 1, "line": 6, "message": "Resolved locations: 1", "verified": true }
  ],
  "captured_output": [
    { "category": "stdout", "output": "starting\r\n", "timestamp_ms": 1785446339847 }
  ],
  "elapsed_ms": 95,
  "exit_code": null,
  "frame": {
    "column": 16, "id": 1001, "line": 6, "name": "total",
    "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" }
  },
  "hit_breakpoint_ids": [1],
  "output_truncated": false,
  "raw_reason": null,
  "reason": "breakpoint",
  "state": "paused",
  "thread_id": 27982711,
  "thread_updates": []
}
```

| Field | Type | Notes |
|---|---|---|
| `state` | string | `paused`, `exited`, `terminated`, `timeout`, `adapter_died`. Always lower-case |
| `reason` | string or null | Why it paused: `breakpoint`, `step`, `entry`, `exception`, `pause`. Null unless paused |
| `raw_reason` | string or null | The adapter's own word, when lazydap normalised `reason`. Null when nothing was normalised |
| `thread_id` | integer or null | The thread that stopped |
| `all_threads_stopped` | boolean | Whether the whole process stopped |
| `additional_stopped_threads` | array of integer | Other threads that stopped inside the 50 ms coalescing window |
| `frame` | object or null | Top frame. Null when the program is gone |
| `hit_breakpoint_ids` | array of integer | lazydap breakpoint ids, matching `lazydap break --list` |
| `exit_code` | integer or null | The debuggee's, set on `exited`. Null on `adapter_died`: that ending carries a `detail` string rather than the adapter's process status |
| `captured_output` | array | Everything the program printed during the wait |
| `output_truncated` | boolean | True when output was dropped rather than buffered |
| `breakpoint_updates` | array | Breakpoints whose state changed during the run |
| `thread_updates` | array | Threads that started or exited |
| `elapsed_ms` | integer | How long the wait took |

**Without `--wait`**, those commands return an acknowledgement instead — the request was
accepted, nothing more. [The `--wait` contract](/guides/wait/) explains when each is right.

### `frame`

```json
{
  "column": 16,
  "id": 1001,
  "line": 6,
  "name": "total",
  "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" }
}
```

`source.path` is absent for frames with no source file on disk; those carry
`source.source_reference` instead, as system frames like `@start` do.

### `captured_output` chunks

```json
{ "category": "stdout", "output": "starting\r\n", "timestamp_ms": 1785446339847 }
```

`category` is `stdout`, `stderr` or `console`. `console` is the adapter talking about itself
("Launching: ...", "Process exited with code 0."), not your program. Filter on
`category == "stdout"` when you want only what the program wrote.

## `launch`

Its own shape, not a stable-state blob. The session identity and what the adapter can do:

```json
{
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6, "message": "Resolved locations: 0",
      "source": "/Users/you/lazydap-demo/hello.c", "verified": true }
  ],
  "capabilities": {
    "supports_conditional_breakpoints": true,
    "supports_configuration_done_request": true,
    "supports_function_breakpoints": true
  },
  "raw_reason": "exception",
  "reason": "entry",
  "session_id": "3a2c4335-0af9-4363-bf2e-2dc866c9045b",
  "state": "paused",
  "thread_id": 27982711
}
```

`breakpoints` is every project breakpoint as the adapter now sees it. Read `verified` here:
that is where a breakpoint that will never bind says so.

## `stack`

```json
{
  "frames": [
    { "column": 16, "id": 1002, "line": 6, "name": "total",
      "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" } },
    { "column": 18, "id": 1003, "line": 14, "name": "main",
      "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" } },
    { "column": 0, "id": 1004, "line": 1751, "name": "start",
      "source": { "name": "@start", "source_reference": 1000 } }
  ],
  "total": null
}
```

Innermost frame first. `total` is the adapter's count of all frames when it supplies one, and
null when it does not.

## `scopes`

```json
{
  "scopes": [
    { "expensive": false, "name": "Local", "variables_reference": 1006 },
    { "expensive": false, "name": "Static", "variables_reference": 1007 },
    { "expensive": false, "name": "Global", "variables_reference": 1008 },
    { "expensive": false, "name": "Registers", "variables_reference": 1009 }
  ]
}
```

`expensive` warns that expanding the scope is slow — `Registers` and globals in a large
binary. `variables_reference` is what `lazydap variables --reference N` takes.

:::caution[References expire when the program moves]
These numbers are valid for this stop only. After any step or continue, call `scopes` again.
A stale one returns `DapProtocolError: Invalid variabes reference` — the typo is codelldb's.
:::

## `variables`

```json
{
  "variables": [
    { "name": "n",   "type_name": "int", "value": "10", "variables_reference": 0 },
    { "name": "sum", "type_name": "int", "value": "21", "variables_reference": 0 },
    { "name": "i",   "type_name": "int", "value": "7",  "variables_reference": 0 }
  ]
}
```

`value` is always a string, because it is the debugger's rendering rather than a typed value.
A non-zero `variables_reference` means the variable has children: pass it back to `variables`
to expand a struct, an array or a pointer target.

## `eval`

```json
{ "type_name": "long long", "value": "20", "variables_reference": 0 }
```

Same shape as one variable, minus the name.

## `break`

```json
{
  "action": "added",
  "applied_to_session": false,
  "breakpoints": [
    { "condition": "i == 7", "enabled": true, "id": 2, "line": 6,
      "source": "/Users/you/lazydap-demo/hello.c", "verified": false }
  ],
  "dry_run": false,
  "not_found": []
}
```

| Field | Notes |
|---|---|
| `action` | `added`, `removed`, `toggled`, or `listed` |
| `applied_to_session` | Whether a live session was updated as well as the state file |
| `breakpoints` | The set acted on. Under `--dry-run`, the set that *would* be |
| `not_found` | Ids you selected that do not exist |
| `dry_run` | Echoes the flag, so a log tells you which run this was |

## `threads`

```json
{ "threads": [ { "id": 27982711, "name": "1: tid=27982711" } ] }
```

## `output`

```json
{
  "chunks": [
    { "category": "stdout", "output": "starting\r\n", "timestamp_ms": 1785446339847 },
    { "category": "stdout", "output": "total=55\r\n",  "timestamp_ms": 1785446390991 }
  ],
  "dropped": 0
}
```

Everything the program has produced this session, not only the last wait window. `dropped` is
non-zero when the buffer overflowed.

## `status`

```json
{
  "daemon_pid": 2452,
  "instance": "lazydap-demo-13cc8efcde46",
  "lazydap_version": "0.1.0",
  "protocol_version": 5,
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

`session` is null when the daemon is up with nothing being debugged.

## `doctor` and `version`

```json
{
  "checks": [
    { "detail": "/Users/you/.local/bin/codelldb", "name": "adapter.codelldb", "ok": true }
  ],
  "ok": true
}
```

```json
{ "lazydap": "0.1.0", "protocol": 5 }
```

The two versions move independently. The protocol one is what a daemon from an older build
mismatches on.

## Errors

Not on stdout. See [errors and exit codes](/reference/errors/).

## See also

- [Output formats and piping](/guides/output-formats/) — `jsonl`, `csv`, `ids`
- [CLI reference](/reference/cli/) — the flags that produce these
