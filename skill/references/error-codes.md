# Errors and exit codes

Two signals, and they answer different questions.

**The exit code** says whether the *command* worked. **The `state` field** says
what the *program* did. `lazydap continue --wait` exiting `0` with
`"state": "exited"` means the debugger did its job and the program finished —
that is a success, not a failure.

## Exit codes

| Code | Meaning | What to do |
|---|---|---|
| `0` | The command worked. | Read the JSON. |
| `1` | The command failed. | Read `error` on stderr and act on it. |
| `2` | The command line was wrong. | Fix the arguments; check [`commands.md`](commands.md). |
| `3` | The daemon could not be started or reached. | Usually transient. Retry once; then report it. |
| `4` | The debug adapter is missing. | codelldb is not installed. Tell the user; you cannot fix this. |

## The error shape

Failures print one JSON object on **stderr** when you asked for a machine
format, and stdout stays empty:

```json
{
  "error": "SessionNotPaused",
  "message": "SessionNotPaused: session 971baa06-... is running; pause it first (`lazydap pause --wait`) or wait for a breakpoint",
  "details": { "session_id": "971baa06-...", "state": "running" }
}
```

Branch on `error`. The `message` is for a human and may be reworded; the name
is the contract.

The `error` name and the exit code are consistent with each other: every
`UsageError` exits `2` and every `DaemonUnreachable` exits `3`, whether the
mistake was caught by the argument parser or by lazydap itself.

**`lazydap doctor` is the exception, on purpose.** It is the command you run to
find out *why* nothing works, so it reports an unreachable daemon as a failed
`daemon` check on stdout and exits `1` — a failed check — rather than exiting
`3` with nothing printed. The checks above it have usually already named the
reason, and `doctor --check-state` starts no daemon at all.

Read the report on stdout, not the error on stderr: a failed run summarises
itself as `{"error":"DaemonInternalError","message":"N check(s) failed"}`, and
the `checks` array is where the reason is.

## The errors you will actually hit

| `error` | Means | Do this |
|---|---|---|
| `SessionNotPaused` | You asked about the stack, scopes, variables or an expression while the program is running. | `lazydap pause --wait`, or `continue --wait` to a breakpoint, then retry. |
| `SessionNotFound` | No session — you have not launched, or you already disconnected. | `lazydap launch <program>` first. |
| `SessionAlreadyActive` | A program is already being debugged. Only one at a time. | `lazydap disconnect`, then launch. `details` names the session in the way. |
| `StaleHandle` | You passed a `--frame` or `--reference` from a stop the program has left, or from a session that has ended. `message` says which. | Ask again *now* — `lazydap stack` for a frame id, `lazydap scopes` for a reference — and retry with the new one. Handles are never reused, so an old one is always detected rather than silently answered with somebody else's data. |
| `BadRequest` | The request made no sense here — a handle nobody handed out (`--frame 0` is not "the top frame"), stepping a program that has already exited, pausing one that is already stopped, or asking for an answer too large to fit in one 16 MiB frame. | Read `message`; it says what to do and where a valid value comes from. For an answer that did not fit, narrow it: `--max`, `--start`/`--count`, `--since`. |
| `AdapterNotFound` | No codelldb on `PATH`. | Nothing you can do in-session. Report it; `details.searched` lists where it looked. |
| `DapProtocolError` | The debugger refused. Usually your expression, not lazydap. | Read `details.adapter_message` — it is the debugger's own words, e.g. *use of undeclared identifier 'y'*. |
| `AdapterCrashed` | The debugger process died. | The session is unrecoverable. `disconnect`, then launch again. |
| `AdapterTimeout` | The debugger did not answer in time. | Retry once; if it repeats, `disconnect` and start over. |
| `InvalidLaunchConfig` | The program or working directory could not be resolved. | Check the path exists and is executable. |
| `Unsupported` | This build does not implement that. | Do not retry. |
| `VersionMismatch` | A daemon from a different build is running. | `lazydap shutdown`, then retry. |
| `UsageError` | The command line was wrong — a flag that does not exist, two that contradict, a `--format` this command cannot print, a `LAZYDAP_TIMEOUT` that is not a number. Always exit `2`. | Fix the arguments; check [`commands.md`](commands.md). Do not retry unchanged. |
| `DaemonUnreachable` | No daemon could be started or contacted. Also covers the directories lazydap keeps its socket, lock, pid and log in — a socket path over the length limit, a runtime directory owned by somebody else. Always exit `3`. | Retry once. If it repeats, `message` names the directory or the socket. |

## Failures that are not errors

These exit `0`. They are outcomes, and you read them from `state`:

| `state` | Means |
|---|---|
| `paused` | Stopped and waiting. `reason` says why. |
| `exited` | The program finished. `exit_code` is its status. |
| `terminated` | The session ended without an exit status — usually a disconnect. |
| `timeout` | Nothing settled within `--timeout`. **The program is still running.** Nothing was paused; call `pause --wait` if you want it stopped. |
| `adapter_died` | The debugger vanished mid-run. `disconnect` and start over. |

## A worked failure

Asking about a variable that is not in scope yet — the single most common
mistake, because `--stop-on-entry` stops before `main`:

```bash
$ lazydap eval "y" --format json
$ echo $?
1
```

stderr:

```json
{
  "error": "DapProtocolError",
  "message": "DapProtocolError: the adapter rejected `evaluate`: ... use of undeclared identifier 'y' ...",
  "details": {
    "command": "evaluate",
    "adapter_message": "... use of undeclared identifier 'y' ..."
  }
}
```

The fix is not a different expression. It is to get the program to a line
where `y` exists: set a breakpoint past its declaration and `continue --wait`.
