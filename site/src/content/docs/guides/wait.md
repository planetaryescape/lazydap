---
title: The --wait contract
description: What --wait blocks on, the five outcomes it can return, and how timeouts behave.
---

Put `--wait` on every command that moves the program, then branch on the `state` field of what
comes back. Without it, a stepping command returns the moment the debugger accepts the
request — before the program has gone anywhere — and anything you ask next reads a stale
stack.

```bash
lazydap continue --wait --format json
```

`--wait` is available on `continue`, `step`, `step-in`, `step-out` and `pause`.

**Not on `launch`.** `lazydap launch --wait` is a usage error (exit `2`). Launching answers
with its own shape — session id, capabilities, and the persisted breakpoints with whatever
the adapter made of them — rather than a stable-state blob. Use `--stop-on-entry` to hold the
program still, then `continue --wait` to move it.

## What it blocks on

Until the program reaches a **stable state**: paused, exited, or terminated. Those are the
states in which asking for a stack or a variable has a defined answer. While the program is
running, `stack`, `scopes`, `variables` and `eval` are refused rather than answered with
something unreliable:

```console
$ lazydap stack --format json
{"details":{"session_id":"74dc1a31-6a61-48c0-8e2a-772de75ef957","state":"running"},"error":"SessionNotPaused","message":"SessionNotPaused: session 74dc1a31-6a61-48c0-8e2a-772de75ef957 is running; pause it first (`lazydap pause --wait`) or wait for a breakpoint"}
```

Two further outcomes are lazydap's rather than the program's: the wait can time out, or the
adapter can die.

## The five outcomes

Read `state`. It is always one of these, always lower-case:

| `state` | Means | What is populated |
|---|---|---|
| `paused` | Stopped somewhere | `reason`, `thread_id`, `frame`, and `hit_breakpoint_ids` when it was a breakpoint |
| `exited` | The program finished | `exit_code`; `frame` is `null` |
| `terminated` | The session is over | Later commands on that session get `SessionNotFound` |
| `timeout` | Nothing settled in time | The program **is still running** |
| `adapter_died` | codelldb went away | The session is unrecoverable. `exit_code` is `null` — the ending carries a `detail` string, not the adapter's process status |

An adapter that dies never gets to stop the program it was debugging, so lazydap kills the
debuggee itself before announcing the ending, and says so in that `detail` — "the debuggee
(pid 4821) was left running and has been killed". You are not left with an orphaned process
that nothing knows is being debugged. The one case it declines is a reused pid, where the
number now belongs to something else and killing it would be worse than leaking.

The command's own exit code is a different question. `lazydap continue --wait` exits `0` when
it successfully reports that your program crashed. Read `state` for the program; read the exit
code for the command.

## What comes back

One object per command, covering the whole trip. From a `continue` that ran to a breakpoint:

```console
$ lazydap continue --wait --format json
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

`captured_output` is the field that usually justifies the flag. Everything the program printed
while it ran is in there, categorised `stdout`, `stderr` or `console`, in order, with
timestamps. You do not have to arrange to capture it separately, and it is not discarded when
the program stops for some other reason.

Every field is filled in on a best-effort basis, including on the bad outcomes. A crash report
that dropped the program's output because the program also crashed would make the crash
harder to diagnose.

## Timeouts

Default 30 seconds. `--timeout N` sets it in seconds; `--timeout 0` waits forever and makes
the wait your problem. `LAZYDAP_TIMEOUT` changes the default.

On timeout, lazydap does not pause the program for you. It says so and leaves it running:

```console
$ lazydap continue --wait --timeout 3 --format json
{
  "captured_output": [
    { "category": "stdout", "output": "tick 0\r\n", "timestamp_ms": 1785446686324 },
    { "category": "stdout", "output": "tick 1\r\n", "timestamp_ms": 1785446687325 },
    { "category": "stdout", "output": "tick 2\r\n", "timestamp_ms": 1785446688325 }
  ],
  "elapsed_ms": 3001,
  "exit_code": null,
  "frame": null,
  "hit_breakpoint_ids": [],
  "reason": null,
  "state": "timeout",
  "thread_id": null
}
```

Three seconds of a program that prints once a second, and all three lines are in
`captured_output`. Auto-pausing here would be surprising, and would hide the difference
between "slow" and "stuck".

To take control after a timeout, pause explicitly:

```bash
lazydap pause --wait --format json
```

That returns a normal `paused` blob, carrying the output produced since the timeout.

## Waiting on more than one thread

By default `--wait` returns on the first `stopped` event, then keeps collecting for 50
milliseconds. Any other thread that stops inside that window lands in
`additional_stopped_threads` rather than being lost.

`--all-threads` waits for every thread to stop instead, with no coalescing window, which is
what you want for a consistent cross-thread snapshot. It is slower and it is possible for a
thread never to stop.

The window exists because DAP sets `allThreadsStopped: true` only on the first stopped event,
so a naive reader concludes one thread stopped when the whole process did.

## One at a time

At most one of `continue`, `step`, `step-in`, `step-out` or `pause` is in flight per session;
further ones queue. Inspection requests — `eval`, `scopes`, `variables`, `stack` — run in
parallel with each other, because in a stable state they cannot interfere.

This is not caution for its own sake. Pipelining execution requests to one adapter deadlocks
when the request you are waiting on is itself blocked on an event that the queued request
would have triggered.

## Using it from a script

```bash
result=$(lazydap continue --wait --format json)
case "$(printf '%s' "$result" | jq -r .state)" in
  paused)       printf '%s' "$result" | jq -r '.frame.source.name + ":" + (.frame.line|tostring)' ;;
  exited)       printf '%s' "$result" | jq -r '"exit " + (.exit_code|tostring)' ;;
  timeout)      echo "still running after the timeout"; lazydap pause --wait --format json ;;
  adapter_died) echo "adapter gone; disconnect and relaunch" >&2; exit 1 ;;
esac
```

Do not poll `lazydap status` in a loop instead. `--wait` exists so that you do not have to,
and polling races with the thing you are polling for.

## See also

- [Debug with an agent](/guides/agents/) — where `--wait` matters most
- [`lazydap continue`](/reference/cli/continue/) — the flags in full
- [JSON output](/reference/json-output/) — the field-by-field schema
