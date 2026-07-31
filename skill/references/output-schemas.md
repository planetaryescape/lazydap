# Output schemas

What each command returns under `--format json`. Field names are stable; a
breaking change to any of them requires a decision-log entry in lazydap.

Optional fields are **omitted** rather than set to `null`, except where noted
below — so test for presence, not for `null`.

## The `--wait` blob

Returned by `continue`, `step`, `step-in`, `step-out` and `pause` when given
`--wait`. The single most important shape here.

```json
{
  "state": "paused",
  "reason": "breakpoint",
  "raw_reason": null,
  "thread_id": 26836542,
  "all_threads_stopped": true,
  "additional_stopped_threads": [],
  "hit_breakpoint_ids": [1],
  "exit_code": null,
  "frame": {
    "id": 1002,
    "name": "main",
    "line": 19,
    "column": 30,
    "source": { "name": "main.c", "path": "/abs/path/main.c" }
  },
  "captured_output": [
    { "category": "stdout", "output": "hello\r\n", "timestamp_ms": 1785433977464 }
  ],
  "output_truncated": false,
  "breakpoint_updates": [],
  "thread_updates": [],
  "elapsed_ms": 95
}
```

| Field | Notes |
|---|---|
| `state` | `paused`, `exited`, `terminated`, `timeout`, `adapter_died`. Always present. Branch on this first. |
| `reason` | Why it stopped: `breakpoint`, `step`, `entry`, `exception`, `pause`, or whatever the debugger called it. `null` unless `paused`. |
| `raw_reason` | Present only when lazydap renamed the reason. See "Normalised reasons" below. |
| `thread_id` | The thread that stopped. `null` unless `paused`. |
| `all_threads_stopped` | Whether the whole program stopped, not just this thread. |
| `additional_stopped_threads` | Other threads that stopped in the same instant. Usually empty. |
| `hit_breakpoint_ids` | Your breakpoint ids — the same numbers `lazydap break` returned. Empty unless a breakpoint caused the stop. |
| `exit_code` | The program's status. Present when it finished. `null` otherwise. |
| `frame` | Where it stopped. `null` when the program is no longer there. |
| `captured_output` | Everything printed during this call, in order. See below. |
| `output_truncated` | `true` if the program outran the buffer and some output was dropped. |
| `breakpoint_updates` | Breakpoints the debugger changed its mind about mid-run — verified late, or moved to the nearest line with code. |
| `thread_updates` | Threads that started or ended during the run. |
| `elapsed_ms` | How long the wait took. |

### `captured_output`

```json
{ "category": "stdout", "output": "hello\r\n", "timestamp_ms": 1785433977464 }
```

`category` is `stdout` or `stderr` for the program's own output, and `console`
for the debugger's commentary (*"Launched process 77265"*). When quoting the
program's output back to a user, filter to `stdout`/`stderr` — the `console`
lines are noise to them.

`timestamp_ms` is Unix-epoch milliseconds.

### Normalised reasons

lazydap reports the reason DAP defines, not the one the adapter happened to
use, and shows its working. codelldb implements stop-on-entry by signalling
the process, which it then reports as an exception:

```json
{ "state": "paused", "reason": "entry", "raw_reason": "exception" }
```

`reason` is what happened. `raw_reason` is what the adapter called it, and is
absent when the two agree. Read `reason`.

## `launch`

```json
{
  "session_id": "971baa06-e3bc-4e20-87cd-326edd9ea046",
  "state": "paused",
  "reason": "entry",
  "raw_reason": "exception",
  "thread_id": 26836542,
  "capabilities": {
    "supports_configuration_done_request": true,
    "supports_function_breakpoints": true,
    "supports_conditional_breakpoints": true
  },
  "breakpoints": [ { "id": 1, "source": "/abs/path/main.c", "line": 19,
                     "enabled": true, "verified": true } ]
}
```

`breakpoints` is what your saved breakpoints did on this launch — check
`verified` here rather than assuming they took.

## `break` — every mode

`break`, `break --list`, `break --remove` and `break --toggle` all return the
same shape, so you parse it once.

```json
{
  "action": "added",
  "dry_run": false,
  "breakpoints": [
    { "id": 1, "source": "/abs/path/main.c", "line": 19,
      "enabled": true, "verified": true,
      "message": "Resolved locations: 1" }
  ],
  "not_found": [],
  "applied_to_session": true
}
```

| Field | Notes |
|---|---|
| `action` | `listed`, `added`, `removed`, `toggled`. |
| `dry_run` | `true` when `--dry-run` was given: nothing changed, and `breakpoints` is what *would* change. |
| `breakpoints` | For `list`, all of them. Otherwise the ones affected. |
| `not_found` | Ids you named that no longer exist — a stale id from an earlier listing. Empty on success. |
| `applied_to_session` | Whether a running program was told. `false` means recorded and waiting for the next launch. |

A breakpoint:

| Field | Notes |
|---|---|
| `id` | Small integer. Stable across restarts; safe to store. |
| `source`, `line` | Absolute path, and the line you asked for. |
| `enabled` | `false` after `--toggle`; disabled breakpoints are not sent to the debugger. |
| `verified` | Whether the debugger found code there. `false` before anything has been launched. |
| `adapter_line` | Present only when the debugger moved it — that is where it will actually stop. |
| `condition`, `hit_condition`, `log_message` | Omitted unless set. |

## `stack`

```json
{
  "frames": [
    { "id": 1002, "name": "main", "line": 19, "column": 30,
      "source": { "name": "main.c", "path": "/abs/path/main.c" } },
    { "id": 1003, "name": "start", "line": 1751, "column": 0,
      "source": { "name": "@start", "source_reference": 1000 } }
  ],
  "total": null
}
```

Innermost frame first. A frame with a `source.path` is a file you can read; one
with only a `source_reference` is code the debugger holds itself (a system
stub, disassembly) and has no file on disk.

`id` is a handle for `scopes --frame` and `eval --frame`. **It stops being
valid the moment the program moves** — fetch a new stack after every step.

## `scopes`

```json
{ "scopes": [
    { "name": "Local", "variables_reference": 1005, "expensive": false },
    { "name": "Static", "variables_reference": 1006, "expensive": false },
    { "name": "Global", "variables_reference": 1007, "expensive": false },
    { "name": "Registers", "variables_reference": 1008, "expensive": false } ] }
```

`Local` is almost always the one you want. Pass its `variables_reference` to
`variables`. `expensive: true` warns that expanding it is slow.

## `variables`

```json
{ "variables": [
    { "name": "x", "value": "5", "type_name": "int", "variables_reference": 0 },
    { "name": "buf", "value": "char [64]", "type_name": "char [64]",
      "variables_reference": 1012 } ] }
```

`value` is always a string — the debugger's own rendering, which is what you
want for pointers and structs.

`variables_reference` is `0` for a scalar and non-zero for anything with
children; pass a non-zero one back to `variables` to expand it.

## `eval`

```json
{ "value": "10", "type_name": "int", "variables_reference": 0 }
```

Same fields as a variable, minus the name. A non-zero `variables_reference`
means the result has children you can expand.

## `threads`

```json
{ "threads": [ { "id": 26836542, "name": "1: tid=26836542" } ] }
```

## `output`

```json
{ "chunks": [ { "category": "stdout", "output": "hello\r\n",
                "timestamp_ms": 1785433977464 } ],
  "dropped": 0 }
```

The whole session's buffered output, and how much was lost to the buffer.
Reading it does not consume it.

## `status`

```json
{
  "instance": "lazydap-myproject",
  "daemon_pid": 77256,
  "uptime_ms": 776,
  "protocol_version": 5,
  "lazydap_version": "0.1.0",
  "session": {
    "session_id": "971baa06-...",
    "adapter": "codelldb",
    "program": "/abs/path/hello",
    "state": "paused",
    "exit_code": null,
    "buffered_events": 5,
    "captured_output_chunks": 4,
    "dropped_events": 0,
    "uptime_ms": 700
  }
}
```

`session` is `null` when nothing has been launched.

## `disconnect`, `shutdown`, `version`, `doctor`

```json
{ "session_id": "971baa06-...", "disconnected": true,
  "dry_run": false, "terminated_debuggee": true }
```

```json
{ "instance": "cap", "shutting_down": true, "dry_run": false, "sessions": [] }
```

```json
{ "lazydap": "0.1.0", "protocol": 2 }
```

```json
{ "ok": true,
  "checks": [ { "name": "adapter.codelldb", "ok": true,
                "detail": "/Users/you/.local/bin/codelldb" } ] }
```

`doctor` exits `1` when any check fails, so you can branch on the exit code
rather than reading `ok`.
