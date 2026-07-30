---
title: Errors and exit codes
description: Exit codes, the error object shape, and what to do about each error name.
---

Two signals answer two different questions. **The exit code** says whether the command
worked. **The `state` field** says what your program did. `lazydap continue --wait` exiting
`0` with `"state": "exited"` means the debugger did its job and the program finished — a
success, not a failure.

## Exit codes

| Code | Means | Do |
|---|---|---|
| `0` | The command worked | Read the JSON on stdout |
| `1` | The command failed | Read `error` on stderr and act on it |
| `2` | The command line was wrong | Fix the arguments |
| `3` | The daemon could not be started or reached | Usually transient; retry once |
| `4` | The debug adapter is missing | Install codelldb |

## The error object

Failures print one JSON object on **stderr**, and stdout stays empty — so a pipeline reading
stdout never receives an error where it expected data.

```console
$ lazydap stack --format json
{"details":{"session_id":"74dc1a31-6a61-48c0-8e2a-772de75ef957","state":"running"},"error":"SessionNotPaused","message":"SessionNotPaused: session 74dc1a31-6a61-48c0-8e2a-772de75ef957 is running; pause it first (`lazydap pause --wait`) or wait for a breakpoint"}
$ echo $?
1
```

Branch on `error`. That name is the contract; `message` is written for a human and gets
reworded. `details` varies by error and is where the useful specifics live.

## Error names

The complete set, from `ErrorCode` in `crates/protocol/src/types.rs` — check there first when
this table looks short.

| `error` | Means | Do |
|---|---|---|
| `SessionNotPaused` | You asked for the stack, scopes, variables or an expression while the program is running | `lazydap pause --wait`, or continue to a breakpoint, then retry |
| `SessionNotFound` | No session: nothing launched, or already disconnected | `lazydap launch <program>` |
| `SessionAlreadyActive` | One program is already being debugged | `lazydap disconnect`, then launch |
| `BadRequest` | The request made no sense here, commonly stepping a program that already exited. Also what an unreadable protocol frame gets | Read `message`; for a finished program, launch again |
| `AdapterNotFound` | No codelldb on `PATH` | [Install it](/getting-started/install/); `details.searched` lists where it looked |
| `DapProtocolError` | The debugger refused. Usually the expression, not lazydap | Read `details.adapter_message` — the debugger's own words |
| `AdapterCrashed` | The debugger process died | Unrecoverable session; `disconnect` and launch again |
| `AdapterTimeout` | The debugger did not answer in time | Retry once; if it repeats, `disconnect` |
| `InvalidLaunchConfig` | The program or working directory could not be resolved | Check the path exists and is executable |
| `InvalidProjectRoot` | The project root could not be determined, so there is nowhere to put `.lazydap/` | Run from inside the project, or set `--instance` |
| `DaemonInternalError` | The daemon hit a bug | Read `lazydap logs`, then report it |
| `Timeout` | An operation exceeded its deadline. Distinct from a `--wait` returning `"state": "timeout"`, which is not an error | Retry, or raise `--timeout` |
| `Cancelled` | The operation was abandoned before it finished, usually because the client went away | Retry if you still want it |
| `Unsupported` | This build does not implement it | Do not retry |
| `VersionMismatch` | A daemon from a different build is running | `lazydap shutdown`, then retry |

Two names come from the CLI rather than the protocol, because they happen before or outside a
daemon round trip:

| `error` | Means | Do |
|---|---|---|
| `UsageError` | Bad arguments. Exits `2`, not `1` | `details.kind` classifies it; `message` carries clap's own suggestion |
| `DaemonUnreachable` | No daemon could be started or contacted. Exits `3` | Usually transient. Retry once; then check `lazydap logs` |

## Outcomes that are not errors

These exit `0`. Read them from `state`:

| `state` | Means |
|---|---|
| `paused` | Stopped and waiting. `reason` says why |
| `exited` | The program finished. `exit_code` is its status |
| `terminated` | The session ended without an exit status, usually a disconnect |
| `timeout` | Nothing settled within `--timeout`. **The program is still running** |
| `adapter_died` | The debugger vanished mid-run |

[The `--wait` contract](/guides/wait/) covers what each carries.

## Worked failures

### A typo in a flag

```console
$ lazydap variables --ref 1006 --format json
{"details":{"kind":"UnknownArgument"},"error":"UsageError","message":"error: unexpected argument '--ref' found\n\n  tip: a similar argument exists: '--reference'\n\nUsage: lazydap variables --reference <REFERENCE>\n\nFor more information, try '--help'."}
$ echo $?
2
```

Exit `2`, and the suggestion comes from the argument parser rather than a hand-maintained
list.

### An expression the debugger will not take

```console
$ lazydap eval 'nope + 1' --format json
{"details":{"adapter_message":"Expression evaluation in pure C not supported. Ran expression as 'ISO C++'.\nerror: <user expression 0>:1:1: use of undeclared identifier 'nope'\n    1 | nope\n      | ^~~~\n","command":"evaluate"},"error":"DapProtocolError","message":"DapProtocolError: the adapter rejected `evaluate`: ..."}
$ echo $?
1
```

`details.adapter_message` is LLDB's own diagnostic, carried through unedited. When an
identifier is undeclared, the fix is usually not a different expression — it is getting the
program to a line where the variable exists. `--stop-on-entry` stops before `main`, where
none of your variables do.

### No session

```console
$ lazydap stack --format json
{"details":{},"error":"SessionNotFound","message":"SessionNotFound: no active session; run `lazydap launch <program>` first"}
$ echo $?
1
```

## See also

- [The `--wait` contract](/guides/wait/) — the five outcomes
- [JSON output](/reference/json-output/) — success shapes
- [Troubleshooting](/troubleshooting/) — symptoms rather than error names
