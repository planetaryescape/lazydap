---
title: "delve quirks"
description: "17 delve behaviours lazydap had to be told about, found by reading the wire rather than the documentation."
---

:::note[Generated page]
From [`docs/reference/delve-quirks.md`](https://github.com/planetaryescape/lazydap/blob/main/docs/reference/delve-quirks.md) in the repository, which is where these are written as they are found. To change this page, change that file, then run `npm run generate` in `site/` and commit the result. CI fails if the two disagree.
:::

Everything delve does that the DAP specification does not require, and that
lazydap therefore had to be told about. Every entry here was found by running
**delve 1.27.0** against **Go 1.26.5** on macOS arm64 and reading the wire — not
by reading delve's documentation, which is unusually good and still does not say
most of this.

The companion files are [`codelldb-quirks.md`](/reference/codelldb-quirks/) and
[`debugpy-quirks.md`](/reference/debugpy-quirks/). Where the three adapters disagree, the
disagreement is noted here, because that is the part an agent gets wrong.

Delve is the best-behaved of the three. It reports a stop-on-entry stop as
`entry`, sends a real `process` event, and names the breakpoint it stopped on.
Most of what follows is about its *launch arguments*, two of which are not
optional in practice.

Entries 12 to 15 were added on 2026-08-01 from the cross-adapter dogfooding
campaign. They are about *reading results* rather than launching, and entry 12 is
the one most likely to break a script written for another language. The
cross-adapter summary lives in the docs-site guide *Write one script for four
languages*.

---

## 1. The port is announced on stdout, under different words

codelldb prints `Listening on 127.0.0.1:1234` to **stderr**, and only when
`RUST_LOG=debug` is set. delve prints

```text
DAP server listening at: 127.0.0.1:54421
```

to **stdout**, with no environment needed.

Nothing about the two startups is shared, which is why `Spawn::Tcp` carries a
whole `TcpSpawn` recipe — program, arguments, environment, which stream, and the
marker text — rather than just a path.

lazydap starts it as `dlv dap --listen=127.0.0.1:0`. Port zero for the reason
codelldb gets it (quirk 3 there): letting the operating system choose is what
keeps two projects on one machine from fighting over a fixed port. The explicit
`127.0.0.1` matters — a debug adapter listening on every interface is a remote
code execution service.

## 2. Without `outputMode: "remote"`, the debuggee's output is lost

**The most consequential entry in this file.** By default, delve writes the
debuggee's stdout and stderr to **its own stdout** — the adapter process's —
rather than sending them as DAP `output` events.

lazydap does not read the adapter's stdout (it is a DAP channel for stdio
adapters and a log for TCP ones), so the symptom is that every
`continue --wait` blob comes back with

```json
"captured_output": []
```

while the program is visibly printing to a terminal nobody is attached to. There
is no error and nothing in the log.

The fix is one launch argument, `outputMode: "remote"`, and lazydap always sends
it. Every `captured_output` assertion in `crates/daemon/tests/wait_delve.rs` is a
regression test for that one line.

## 3. delve's own chatter arrives tagged as the debuggee's stdout

With `outputMode: "remote"` on, the compile step reports itself through the same
channel the program's output uses, and with the same category:

```json
{"category": "stdout", "output": "Building /path/to/main.go\n"}
{"category": "console", "output": "Type 'dlv help' for list of commands.\n"}
{"category": "stdout", "output": "hello from m22\n"}
```

The first line is delve talking; the third is the program. Both say `stdout`.

lazydap passes this through rather than filtering it. Dropping a line because it
starts with `Building ` would mean a program that legitimately prints that loses
it, and a debugger that silently edits a program's output is worse than one that
includes a line of build noise. It only appears for `mode: "debug"` — an `exec`
launch compiles nothing and its `captured_output` is the program's alone.

Worth knowing if you are diffing `captured_output` against expected program
output: strip the build line, or launch a prebuilt binary.

## 4. `mode` is required, and lazydap infers it from the filename

delve's `launch` needs to know whether `program` is source to compile or a binary
to run:

| mode | what `program` is | what delve does |
|---|---|---|
| `debug` | a `.go` file or package directory | compiles it, then runs the result |
| `exec` | an already-built binary | runs it |
| `test` | a package | compiles and runs its tests |

lazydap reads the extension: `.go` means `debug`, anything else means `exec`.
This is the same rule `AdapterKind::for_program` uses to pick delve in the first
place.

`test` is **not** sent. Supporting it needs a way to say "this is a test binary"
that the CLI does not have — the filename cannot carry it, because
`foo_test.go` is a perfectly ordinary program to `debug`. It is a feature, not a
quirk, and it needs its own milestone.

Getting the mode wrong is not subtle: delve rejects a `.go` file in `exec` mode
and a binary in `debug` mode, both with a message naming the file.

## 5. `mode: "debug"` compiles into the *adapter's* working directory

Left alone, delve writes the binary it compiles to `__debug_bin<random>` in its
own working directory — which is the **daemon's**, so somebody's repository. It
shows up in `git status` at about 2 MB.

delve removes it when it handles `disconnect`, and lazydap's teardown sends one
before killing the adapter, so an ordinary session leaves nothing behind
(verified: `disconnect`, await the response, `SIGKILL`, no file). An adapter that
dies *without* a disconnect leaves it there for good.

So lazydap sends `output`, pointing at a unique path under the system temporary
directory. The leak still happens on a hard adapter death; it just lands
somewhere the operating system sweeps rather than in a working tree. The path is
unique per launch because delve deletes it on disconnect, and a shared name would
let one session's cleanup remove the file another was about to run.

This also makes leaked Go debuggees findable: they run under a
`lazydap-delve-` prefix, which is what the suite's stray check greps for.

## 6. The entry stop has no goroutine, so there is no stack

A `--stop-on-entry` launch stops before the Go runtime has scheduled anything.
`threads` answers with a single placeholder:

```json
{"threads": [{"id": 1, "name": "Dummy"}]}
```

and `stackTrace` on that thread **fails**:

```json
{"error": {"id": 2004, "format": "Unable to produce stack trace: unknown goroutine 1"}}
```

so `lazydap stack` immediately after a Go `launch --stop-on-entry` is an error,
not an empty stack.

lazydap does not paper over this. An empty stack would say "no frames" where the
truth is "not yet", and skipping the entry stop for Go would take a working
`--stop-on-entry` away. Continue to a breakpoint and everything works normally.

Not mode-specific: `exec` on a prebuilt binary behaves the same way, so it is
delve's entry point rather than the compile step.

## 7. An unrecovered panic pauses — where debugpy exits

The three adapters do genuinely different things with a program that kills
itself, and this is the one most likely to surprise an agent:

| adapter | failure | outcome |
|---|---|---|
| codelldb | segfault | **pauses** — a signal the debugger sees whether or not anybody asked |
| debugpy | uncaught exception | **exits**, code 1, no pause (a stop needs `setExceptionBreakpoints`, which lazydap does not send) |
| delve | unrecovered panic | **pauses**, `reason: "exception"` |

lazydap sends no exception filters to any of them. delve pauses anyway: its
`initialize` response advertises `unrecovered-panic` and `runtime-fatal-throw`
with `"default": true`, and it applies those defaults server-side rather than
waiting to be asked.

An agent that learned "a crash means `state: exited`" from a Python session will
be wrong here. The program is still there, and its stack is still readable, which
is better — just different.

## 8. `exited` arrives before `terminated`

DAP does not order these two, and the three adapters do not agree. delve sends
`exited` (with the code) first and `terminated` after it.

That is the favourable order — the exit code is in hand before the session ends —
and it is why lazydap's `POST_TERMINATION_GRACE` drain, which exists for adapters
that send them the other way round, never has anything to do for delve.

## 9. The adapter outlives the debuggee and waits for the client

After the debuggee exits, `dlv dap` stays running. It exits, cleanly, when the
DAP client disconnects.

This is correct behaviour and worth stating because it looks like a leak: a
`pgrep` for `dlv dap` between a program finishing and the session being disposed
finds one. lazydap reaps it when the session goes or the daemon shuts down.
Verified: after `lazydap shutdown`, zero `dlv` processes, zero debuggees, zero
temporary binaries.

## 10. `hitBreakpointIds` is populated — unlike debugpy

delve's `stopped` event names the breakpoint it stopped on, the way codelldb's
does:

```json
{"reason": "breakpoint", "threadId": 1, "allThreadsStopped": true, "hitBreakpointIds": [1]}
```

debugpy sends none ([`debugpy-quirks.md`](/reference/debugpy-quirks/), entry 5), so an
agent that branches on *which* breakpoint was hit works under codelldb and delve
and must fall back to the frame under debugpy.

## 11. A dying adapter does not reliably take the debuggee with it

Delve sits between the other two here, and it depends on what the program was
doing:

- **Paused at a breakpoint**, `SIGKILL` the adapter → the debuggee dies with it.
- **Running**, `SIGKILL` the adapter → the debuggee **survives**, reparented to
  init, still running.

So delve needs D045's reaper, and the reaper needed fixing to handle it (D061).
It identifies a debuggee by matching its command line against what was launched;
under `mode: "debug"` the process in the table is the *compiled binary*, not the
`.go` file, so the match failed and lazydap declined to kill its own debuggee.
It now takes the adapter's word — the `process` event's `name` — for what was
actually started.

Found by `wait_delve.rs`'s adapter-kill test leaking two Go debuggees onto the
development machine, which is the same way D045 itself was found.

## 12. `type_name` is never sent — the type is inside `value`

delve omits DAP's optional `type` field entirely, on every variable and every `evaluate`
result, and encodes the type into the value string instead:

```console
$ lazydap eval "p" --format json
{
  "value": "main.Pt {X: 1, Y: 2}",
  "variables_reference": 1001
}

$ lazydap eval "s" --format json
{
  "value": "[]int len: 3, cap: 3, [1,2,3]",
  "variables_reference": 1002
}
```

The locals scope is the same:

```console
$ lazydap variables --reference 1000 --format json
{ "variables": [
    { "name": "p", "value": "main.Pt {X: 1, Y: 2}", "variables_reference": 1003 },
    { "name": "s", "indexed_variables": 3, "value": "[]int len: 3, cap: 3, [1,2,3]", "variables_reference": 1004 },
    { "name": "n", "value": "42", "variables_reference": 0 },
    { "name": "name", "value": "\"hi\"", "variables_reference": 0 } ] }
```

Because lazydap omits absent optional fields rather than writing `null`, there is **no
`type_name` key at all** in a Go variable — where codelldb and debugpy both put a string. Code
doing `v["type_name"]` gets a missing-key error rather than a null, and code doing
`v.get("type_name").startswith(...)` gets `None`.

It is not a loss of information — `main.Pt` and `[]int` are right there, and arguably more
useful than codelldb's C-flavoured names (its quirk 18). It is a *shape* difference, and it is
the single most likely thing to break a script written against Python or C and then pointed at
Go. `type_name` cannot be a required field in any code path meant to cover four languages.

## 13. `frame.column` is always `0`

```console
$ lazydap continue --wait --format json
{ "frame": { "column": 0, "id": 1000, "line": 15, "name": "main.main",
             "source": { "name": "goq.go", "path": "/Users/you/goq.go" } },
  "hit_breakpoint_ids": [ 8 ],
  "reason": "breakpoint",
  "state": "paused" }
```

DAP columns are 1-based, so `0` is not merely a placeholder — it is outside the legal range,
and it is delve's way of saying it has no column to report.

All three adapters differ here, and two of the three are lying:

| adapter | `frame.column` |
|---|---|
| codelldb | real (`5` at a statement indented four spaces) |
| debugpy | always `1` — the smallest legal value (its quirk 15) |
| delve | always `0` — not a legal value at all |

Anything that renders a caret under a source line, or slices a line at the column, needs to
treat this as advisory. delve's `0` at least fails loudly if you use it as an index into a
1-based string; debugpy's `1` quietly points at the first character forever.

## 14. `eval` errors do not say what went wrong

Every failed evaluation gives the same seven words:

```console
$ lazydap eval "no_such_var" --format json
{"details":{"adapter_message":"Unable to evaluate expression","command":"evaluate"},
 "error":"DapProtocolError",
 "message":"DapProtocolError: the adapter rejected `evaluate`: Unable to evaluate expression"}

$ lazydap eval "1/0" --format json
{"details":{"adapter_message":"Unable to evaluate expression","command":"evaluate"},
 "error":"DapProtocolError",
 "message":"DapProtocolError: the adapter rejected `evaluate`: Unable to evaluate expression"}
```

An undefined identifier and a division by zero are indistinguishable. The message names
neither the identifier nor the cause, so there is nothing to act on: an agent cannot tell "you
typed the name wrong" from "the expression is arithmetically invalid" from "that variable is
not in scope at this frame".

This is the mirror image of codelldb's quirk 20, which buries a genuinely precise diagnosis
(`use of undeclared identifier 'no_such_var'`) under an alarming irrelevant banner. Given the
two, codelldb's is the better failure — the information is there once you skip a line.

What delve *can* do is worth stating alongside, because it is more than codelldb: calls work.

```console
$ lazydap eval "len(s)" --format json
{ "value": "3", "variables_reference": 0 }
```

codelldb rejects `v.len()` outright (its quirk 19). So an expression that fails against Go may
have failed for a reason as ordinary as a typo — do not conclude the expression form is
unsupported.

## 15. A breakpoint on a line with no statement is refused, helpfully

Where codelldb slides forward and debugpy slides backward, delve declines and says why. A
breakpoint on line 2 of a Go file — the blank line after `package main`:

```console
$ lazydap break /Users/you/goq.go:2 --format json
{ "breakpoints": [ { "enabled": true, "id": 7, "line": 2,
                     "source": "/Users/you/goq.go", "verified": false } ] }

$ lazydap launch /Users/you/goq.go --format json
{ "breakpoints": [
    { "enabled": true, "id": 7, "line": 2,
      "message": "could not find statement at /Users/you/goq.go:2, please use a line with a statement",
      "source": "/Users/you/goq.go", "verified": false },
    { "enabled": true, "id": 8, "line": 15,
      "source": "/Users/you/goq.go", "verified": true } ],
  "state": "running" }
```

`verified: false` plus a `message` that names the file, the line and the fix. This is the
behaviour the other two should have: nothing silently moves, and the caller is told in terms
they can act on.

It does mean a Go breakpoint is more likely to be refused than a C or Python one, and a script
that ignores `verified` will wait at a breakpoint that was never set. Check `verified` and read
`message` when it is false — under delve the message is worth reading, which is not true
everywhere.

---

## 16. It waits to be disconnected from after `terminated` — and that is when it deletes the binary

delve holds its socket open after reporting the program terminated, waiting for a
`disconnect`, exactly as codelldb (quirk 25) and debugpy (quirk 17) do. No EOF
arrives; a client reading until the connection closes reads forever.

For delve there is a second thing riding on that `disconnect`: the binary
`mode: "debug"` compiled is deleted when delve handles it. So an exited session
that was never disconnected from leaked both a `dlv` process and a
`lazydap-delve-<pid>-<nanos>` file in the temp directory — the file leak quirk 5
describes, reached by a different route. Since D094 the daemon disconnects an
adapter as soon as its session ends, which is what gives delve the chance to
clean up after itself; lazydap's own removal of the file is now the backstop it
was meant to be rather than the only thing doing it.

Two smaller notes from the same session (delve 1.27.0, 2026-08-18):

- **`setBreakpoints` for a file that does not exist is answered, not rejected** —
  `{"verified": false, "message": "could not find file /tmp/ghost.go"}`. Same as
  the other two adapters.
- **`mode: "debug"` compiles before it says anything.** delve emits one
  `Building...` output event and is then silent for as long as `go build` takes.
  lazydap used to bound every message of a launch at 15 s each, which failed a
  launch whose build took longer even though the launch itself had 30 s; after
  `launch` is sent the only deadline is now the launch's own.

---

## 17. It cannot leave a debuggee running, and does not claim it can

delve's `initialize` answer contains no `supportTerminateDebuggee` — DAP's
capability for "you may ask me to leave the debuggee running", spelled without
the `s` on `support`. That is honest of it. Measured on delve 1.27.0
(2026-08-18): a `disconnect` carrying `terminateDebuggee: false` against a
running Go program is answered promptly, and the debuggee is dead **0.08 s
later**. Under `mode: "debug"` the compiled binary runs as delve's own child, and
delve takes it with it however the request was phrased.

**What lazydap does:** treats delve as an adapter that cannot detach, so
`--no-terminate` is carried out as a terminate and answered
`terminated_debuggee: true`, with a warning on stderr (D095). The alternative
was reporting `false` about a process that had been dead for eighty milliseconds.

---

## Versions this was verified against

| thing | version |
|---|---|
| delve | 1.27.0 (`$Id: 0782d3511ee64ac561a207d35b3403f49d3744a6`) |
| Go | 1.26.5 darwin/arm64 |
| lazydap | M22 |

Delve requires a Go toolchain for `mode: "debug"` — it shells out to `go build`.
`mode: "exec"` needs only the binary, which is worth knowing for a container that
ships a built program and no compiler.

Install it with:

```bash
go install github.com/go-delve/delve/cmd/dlv@latest
```

and make sure `$(go env GOPATH)/bin` is on `PATH` — `go install` puts it there,
and a shell that has never been told about that directory will not find it. This
is the single most likely reason `lazydap doctor --check-adapters` reports delve
missing on a machine that has it.

## See also

- [Write one script for four languages](/guides/adapters/) — what differs between the three adapters, side by side
- [codelldb quirks](/reference/codelldb-quirks/) — the same treatment for codelldb
- [debugpy quirks](/reference/debugpy-quirks/) — the same treatment for debugpy
- [Troubleshooting](/troubleshooting/) — the same ground, organised by symptom
