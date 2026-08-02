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

## The errors you will actually hit

| `error` | Means | Do this |
|---|---|---|
| `SessionNotPaused` | You asked about the stack, scopes, variables or an expression while the program is running. | `lazydap pause --wait`, or `continue --wait` to a breakpoint, then retry. |
| `SessionNotFound` | No session — you have not launched, or you already disconnected. | `lazydap launch <program>` first. |
| `SessionAlreadyActive` | A program is already being debugged. Only one at a time. | `lazydap disconnect`, then launch. `details` names the session in the way. |
| `StaleHandle` | You passed a `--frame` or `--reference` from a stop the program has since left. | Ask again *at this stop* — `lazydap stack` for a frame id, `lazydap scopes` for a reference — and retry with the new one. Never a reason to re-launch. |
| `BadRequest` | The request made no sense here — a handle nobody handed out (`--frame 0` is not "the top frame"), stepping a program that has already exited, or pausing one that is already stopped. | Read `message`; it says what to do and where a valid value comes from. |
| `AdapterNotFound` | No codelldb on `PATH`. | Nothing you can do in-session. Report it; `details.searched` lists where it looked. |
| `DapProtocolError` | The debugger refused. Usually your expression, not lazydap. | Read `details.adapter_message` — it is the debugger's own words, e.g. *use of undeclared identifier 'y'*. |
| `AdapterCrashed` | The debugger process died. | The session is unrecoverable. `disconnect`, then launch again. |
| `AdapterTimeout` | The debugger did not answer in time. | Retry once; if it repeats, `disconnect` and start over. |
| `InvalidLaunchConfig` | The program or working directory could not be resolved. | Check the path exists and is executable. |
| `Unsupported` | This build does not implement that. | Do not retry. |
| `VersionMismatch` | A daemon from a different build is running. | `lazydap shutdown`, then retry. |

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
