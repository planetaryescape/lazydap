# Output schemas

What each command returns under `--format json`. Field names are stable; a
breaking change to any of them requires a decision-log entry in lazydap.

Two conventions, and it matters which one a field follows:

- **Everything a response is made of** — the `--wait` blob and its `frame`,
  `locals` and `user_frame`; `status`'s `session` and its `exit_code`; `stack`'s
  `total` — is **always present**, and `null` when there is nothing. Test the
  value.
- **A breakpoint, a stack frame, a variable, a watch** — the objects a response
  carries rather than defines — **omit** their optional fields. So a frame with
  no file has no `source.path` key at all, and a breakpoint with no condition
  has no `condition`. Test for presence.

## The `--wait` blob

Returned by `continue`, `step`, `step-in`, `step-out` and `pause` when given
`--wait`. The single most important shape here.

```json
{
  "state": "paused",
  "reason": "breakpoint",
  "raw_reason": null,
  "thread_id": 26836542,
  "adapter_thread_id": null,
  "all_threads_stopped": true,
  "additional_stopped_threads": [],
  "hit_breakpoint_ids": [1],
  "exit_code": null,
  "frame": {
    "id": 1,
    "name": "main",
    "line": 19,
    "column": 30,
    "source": { "name": "main.c", "path": "/abs/path/main.c" }
  },
  "user_frame": null,
  "locals": {
    "frame_id": 1,
    "variables_reference": 2,
    "variables": [
      { "name": "x", "value": "10", "type_name": "int",
        "evaluate_name": "x", "variables_reference": 0 }
    ],
    "truncated": false
  },
  "captured_output": [
    { "category": "stdout", "output": "hello\r\n", "timestamp_ms": 1785433977464 }
  ],
  "output_truncated": false,
  "dropped_events": 0,
  "breakpoint_updates": [],
  "thread_updates": [],
  "elapsed_ms": 95
}
```

| Field | Notes |
|---|---|
| `state` | `paused`, `exited`, `terminated`, `timeout`, `adapter_died`. Always present. Branch on this first. |
| `reason` | Why it stopped: `breakpoint`, `step`, `entry`, `exception`, `pause`, or whatever the debugger called it. `null` unless `paused`. |
| `raw_reason` | `null` unless lazydap renamed the reason. See "Normalised reasons" below. |
| `thread_id` | The thread that stopped, or — after a `step --thread` — the thread you asked to step. `null` unless `paused`. |
| `adapter_thread_id` | `null` unless the debugger named a *different* thread than the one you asked to step. codelldb does this: it answers a step aimed at one thread by naming whichever it had selected before. `thread_id` is the thread that moved; this is the debugger's own answer, kept so nothing is hidden. |
| `all_threads_stopped` | Whether the whole program stopped, not just this thread. |
| `additional_stopped_threads` | Other threads that stopped in the same instant. **Always empty against codelldb**, which reports a multi-threaded stop as a single event. Read `all_threads_stopped` instead. |
| `hit_breakpoint_ids` | Your breakpoint ids — the same numbers `lazydap break` returned. Empty unless a breakpoint caused the stop. |
| `exit_code` | The program's status. Present when it finished. `null` otherwise. |
| `frame` | Where it stopped. `null` when the program is no longer there. |
| `user_frame` | The nearest frame **below** `frame` that has a source path, present only when `frame` has none. A crash inside a library stops in something like `_platform_strcmp$VARIANT$Base`, which has no file you can open; this is the frame in the code you are debugging. It is never a correction of `frame` — that is genuinely where the program is. **Read `user_frame` first and fall back to `frame`.** `null` when `frame` already has a path, or when no frame in the stack does. |
| `locals` | The locals of whichever of those two a person would look at, so reading one is not a second command. `frame_id` says which frame they belong to. `null` when the program is not paused or the debugger would not answer — never an empty list standing in for "could not find out". Capped at 100, with `truncated` saying so; page the rest with `variables --reference` and `--start`. |
| `captured_output` | Everything printed during this call, in order. See below. |
| `output_truncated` | `true` when you are **not** seeing all of it — either the run outran the 1 MB output cap (what you keep is then a *prefix*; nothing after the cap is spliced on), or events were lost before this call could read them (what you keep is then a *suffix*). |
| `dropped_events` | How many events were lost before this call could carry them. `0` when nothing was lost that way — including when `output_truncated` was set by the output cap, which drops bytes rather than events. |
| `breakpoint_updates` | Breakpoints the debugger changed its mind about mid-run — verified late, or moved to the nearest line with code. |
| `thread_updates` | Threads that started or ended during the run. **Always empty against codelldb**, which sends no per-thread events. |
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
`null` when the two agree. Read `reason`.

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
    "supports_conditional_breakpoints": true,
    "supports_variable_paging": false
  },
  "breakpoints": [ { "id": 1, "source": "/abs/path/main.c", "line": 19,
                     "enabled": true, "verified": true } ]
}
```

`breakpoints` is what your saved breakpoints did on this launch — check
`verified` here rather than assuming they took. A `message` appears only on one
that did **not** verify, where it is the reason; a verified breakpoint carries
none, because the debugger's commentary on one it accepted contradicted itself
more often than it informed.

## `continue`, `step` and friends *without* `--wait`

```json
{ "session_id": "971baa06-…", "state": "running", "thread_id": 26836542,
  "already_running": false }
```

`already_running: true` means **nothing was resumed, because nothing was
stopped** — the request was a no-op and `thread_id` is `null`. It is not a
failure and not a success at moving the program; it is the honest answer to
`continue` on a program that is already going. Use `--wait` if what you wanted
was the next stop.

## `break` — every mode

`break`, `break --list`, `break --remove` and `break --toggle` all return the
same shape, so you parse it once.

Setting a location is idempotent and last-write-wins: `break x.c:10 --condition
'i == 3'` on a line you already broke on updates that breakpoint rather than
adding a second one, and the modifiers you *omit* are cleared. That includes
`enabled`: a re-set re-enables a breakpoint somebody had disabled with
`--toggle`, so pass `--disabled` when you mean to keep it off. Read `action` to
tell the three cases apart.

If a mutation succeeds in the store but the debugger will not take it — an
adapter that has just died — the command **fails** (exit 1) and the error says
so: `details.recorded_breakpoint_ids` names what the project kept, and
`details.applied_to_session` is `false`. The change is real and applies at the
next `launch`; only the running session missed it.

```json
{
  "action": "added",
  "dry_run": false,
  "breakpoints": [
    { "id": 1, "source": "/abs/path/main.c", "line": 19,
      "enabled": true, "verified": true }
  ],
  "not_found": [],
  "applied_to_session": true
}
```

| Field | Notes |
|---|---|
| `action` | `listed`, `added`, `updated`, `unchanged`, `removed`, `toggled`. Setting a location that already has a breakpoint **edits** it, keeping its id: `updated` when the modifiers now differ, `unchanged` when you asked for what was already there. |
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

`id` is a handle for `scopes --frame` and `eval --frame`. It is **not** a
position in the stack — `--frame 0` is not "the top frame", it is a number
nobody handed out, and lazydap refuses it saying so.

**A handle stops being valid the moment the program moves,** and again when the
session ends. Fetch a new stack after every step. Using an old one is refused
with `StaleHandle` and exit 1 rather than answered: handles are numbered by the
daemon and never reused, so lazydap can always tell an old one from a current
one, and you can never be given another frame's — or another *session's* — data
by accident. Both `frame_id` and `variables_reference` work this way.

## `scopes`

```json
{ "scopes": [
    { "name": "Local", "variables_reference": 5, "expensive": false },
    { "name": "Static", "variables_reference": 6, "expensive": false },
    { "name": "Global", "variables_reference": 7, "expensive": false },
    { "name": "Registers", "variables_reference": 8, "expensive": false } ] }
```

Usually you do not need this call at all: the `--wait` blob already carries the
stopped frame's locals. Reach for it when you want another scope, or the locals
of a frame further down.

`Local` is almost always the one you want. Pass its `variables_reference` to
`variables`. `expensive: true` warns that expanding it is slow.

## `variables`

```json
{ "variables": [
    { "name": "x", "value": "5", "type_name": "int",
      "evaluate_name": "x", "variables_reference": 0 },
    { "name": "[100]", "value": "100", "type_name": "int",
      "evaluate_name": "big[100]", "variables_reference": 0 },
    { "name": "buf", "value": "char [64]", "type_name": "char [64]",
      "evaluate_name": "buf", "variables_reference": 12 } ],
  "truncated": false }
```

`value` is always a string — the debugger's own rendering, which is what you
want for pointers and structs.

`variables_reference` is `0` for a scalar and non-zero for anything with
children; pass a non-zero one back to `variables` to expand it.

`evaluate_name` is the expression that names this row, in the debugger's own
words — **use it rather than `name` when building an `eval` argument.** A row
called `[100]` or `label` means nothing to `eval` on its own; `evaluate_name`
is what you can actually pass. Absent when the debugger did not supply one.

`--start` and `--count` window the list and are honoured against every adapter,
including ones that ignore them on the wire. `--filter` is passed straight to
the debugger, and a debugger that does not implement it returns everything —
lazydap does not second-guess which children are indexed.

`truncated` means **there is more than you are seeing** — whatever narrowed the
list. It is `true` when the default cap bit *and* when your own `--count` left
rows behind, so you can page on it without tracking which limit applied. A
window that happens to reach the end of the list reports `false`.

**At most 200 rows come back by default**, so a `Vec` of two thousand does not
silently become two thousand and one rows of your context. Page on with
`--start`, or raise the cap with `--max N` — `--max 0` lifts it entirely. When
both are given, the narrower wins. Values themselves are never shortened: a
truncated *list* is recoverable, a truncated *value* would be a claim about the
data.

`--count 0` and `--max 0` both mean "no limit", the way `--timeout 0` does.

## `eval`

```json
{ "value": "10", "type_name": "int", "variables_reference": 0 }
```

Same fields as a variable, minus the name. A non-zero `variables_reference`
means the result has children you can expand.

An expression the debugger could not evaluate **fails the command** — exit 1
with an `error` on stderr — rather than returning the error text as a `value`.
One known gap: codelldb reports an unreadable address as a value that reads
`<read memory from 0x4 failed (0 of 4 bytes read)>` and exits 0. A `value`
wrapped in angle brackets is worth a second look.

## `threads`

```json
{ "threads": [ { "id": 26836542, "name": "1: tid=26836542" } ] }
```

`name` is **absent** when the debugger did not name the thread — lazydap does
not invent one. Asking while the program is *running* is allowed but the answer
is debugger-dependent: codelldb replies with a single nameless thread `0`, which
is a placeholder rather than a thread. Ask again once it is paused.

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
  "protocol_version": 9,
  "lazydap_version": "0.2.8",
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
{ "lazydap": "0.2.8", "protocol": 9 }
```

```json
{ "ok": true,
  "checks": [ { "name": "adapter.codelldb", "ok": true,
                "detail": "/Users/you/.local/bin/codelldb" } ] }
```

`ok` — and the exit code, which is `1` when it is `false` — means **lazydap can
debug something here**, not that this machine has every adapter lazydap ships.
Every check that is about lazydap itself has to pass (`config.file`,
`state.file`, `daemon`), and at least one `adapter.*` check has to pass. A
missing adapter is reported with `"ok": false` and a `detail` saying how to
install it, and does not fail the run on its own; losing the last one does.

In `--format table` a failed adapter check reads `missing` rather than
`FAILED`, so the column agrees with the verdict. The JSON has no such value —
it is `"ok": false` either way.

`doctor --check-state` reads `.lazydap/state.toml` in this process and starts
no daemon, so it can name the line in a state file that stops one from
starting.
