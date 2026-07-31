# delve quirks

Everything delve does that the DAP specification does not require, and that
lazydap therefore had to be told about. Every entry here was found by running
**delve 1.27.0** against **Go 1.26.5** on macOS arm64 and reading the wire — not
by reading delve's documentation, which is unusually good and still does not say
most of this.

The companion files are [`codelldb-quirks.md`](codelldb-quirks.md) and
[`debugpy-quirks.md`](debugpy-quirks.md). Where the three adapters disagree, the
disagreement is noted here, because that is the part an agent gets wrong.

Delve is the best-behaved of the three. It reports a stop-on-entry stop as
`entry`, sends a real `process` event, and names the breakpoint it stopped on.
Most of what follows is about its *launch arguments*, two of which are not
optional in practice.

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

## 3. `mode` is required, and lazydap infers it from the filename

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

## 4. `mode: "debug"` compiles into the *adapter's* working directory

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

## 5. The entry stop has no goroutine, so there is no stack

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

## 6. An unrecovered panic pauses — where debugpy exits

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

## 7. `exited` arrives before `terminated`

DAP does not order these two, and the three adapters do not agree. delve sends
`exited` (with the code) first and `terminated` after it.

That is the favourable order — the exit code is in hand before the session ends —
and it is why lazydap's `POST_TERMINATION_GRACE` drain, which exists for adapters
that send them the other way round, never has anything to do for delve.

## 8. The adapter outlives the debuggee and waits for the client

After the debuggee exits, `dlv dap` stays running. It exits, cleanly, when the
DAP client disconnects.

This is correct behaviour and worth stating because it looks like a leak: a
`pgrep` for `dlv dap` between a program finishing and the session being disposed
finds one. lazydap reaps it when the session goes or the daemon shuts down.
Verified: after `lazydap shutdown`, zero `dlv` processes, zero debuggees, zero
temporary binaries.

## 9. `hitBreakpointIds` is populated — unlike debugpy

delve's `stopped` event names the breakpoint it stopped on, the way codelldb's
does:

```json
{"reason": "breakpoint", "threadId": 1, "allThreadsStopped": true, "hitBreakpointIds": [1]}
```

debugpy sends none (its quirks file, entry 3), so an agent that branches on
*which* breakpoint was hit works under codelldb and delve and must fall back to
the frame under debugpy.

## 10. A dying adapter does not reliably take the debuggee with it

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
