# debugpy quirks

The counterpart to [`codelldb-quirks.md`](codelldb-quirks.md), and much shorter
on purpose: debugpy follows the DAP specification closely, so most of this file
records places where it does something *differently from codelldb* rather than
something wrong.

Everything below was observed against **debugpy 1.8.21** on **CPython 3.14.6**,
macOS 15 (arm64), by driving a real adapter and reading the wire. Where a claim
came from a captured message, the message is quoted.

---

## 1. It is a module, not a binary

There is no adapter executable to find on `PATH`. The adapter is started as:

```
python3 -m debugpy.adapter
```

debugpy does install a `debugpy-adapter` shim, but a user-site install puts it
somewhere that is usually *not* on `PATH` — `~/Library/Python/3.14/bin` on
macOS — while an interpreter that can import the module is on `PATH` by
definition of having been found there.

**What lazydap does:** discovery for `AdapterKind::Debugpy` resolves a Python
*interpreter* (`python3`, then `python`), and `[adapter.debugpy] command` in
the config file means an interpreter too. Whichever is found is then asked to
prove itself:

```
python3 -c "import debugpy; print('lazydap-debugpy-ok')"
```

Exit status alone is not enough — a pinned command that is not an interpreter
at all succeeds at almost any argument list. `/bin/echo` is the honest
accident; a shell wrapper is the interesting one. Only something that really
ran the program prints the sentinel back.

An interpreter that is found and cannot import debugpy does not end the search:
a machine can have several interpreters and only one with debugpy in it. If
none can, the error names the one that was found and how to fix it, which is a
different problem from "no Python at all" and has a different fix.

## 2. It speaks DAP over stdio, not TCP

codelldb listens on a port and announces it on stderr (its quirk 3). debugpy
uses its own stdin and stdout. The framing is identical, so this is a transport
difference and nothing more — see D053 for how `DapTransport` covers both.

A consequence worth stating: there is no port to scrape, so there is none of
the `RUST_LOG=debug` fragility that codelldb's quirk 2 describes. stderr is
still drained into the log, because a child whose stderr pipe fills up blocks
writing to it, and an adapter blocked in a log call answers no requests.

## 3. It sends the `process` event — so no pid scraping

The thing codelldb does not do (its quirk 9). Captured verbatim:

```json
{"seq":15,"type":"event","event":"process","body":{
  "startMethod":"launch","isLocalProcess":true,
  "systemProcessId":93720,"name":"/tmp/main.py","pointerSize":64}}
```

So D045's reaping gets the debuggee's pid as data rather than out of a
human-readable console line. The shared handshake reads this event for any
adapter that sends it; `DebugAdapter::debuggee_pid_in` is the fallback for
adapters that do not, and codelldb is the only implementor.

Two details:

- `isLocalProcess: false` is ignored. A remote pid names an unrelated *local*
  process, and the reaper would be entitled to kill it.
- The event is not ordered against the `launch` response. A launch without
  `--stop-on-entry` can settle before it arrives, so the pump watches for it as
  well as the handshake. `Session::set_debuggee` keeps the first answer, so both
  recording it is not two records.

## 4. A stop-on-entry stop really is called `entry`

codelldb implements entry-stop with `SIGSTOP` and LLDB reports the result as an
exception, which is why D033 renames it. debugpy just says:

```json
{"reason":"entry","threadId":1,"preserveFocusHint":false,"allThreadsStopped":true}
```

So the normalisation has nothing to do, and `raw_reason` stays `null` — which
is the point of D033 keeping it visible: an empty `raw_reason` means nothing
was renamed.

## 5. `stopped` carries no `hitBreakpointIds`

The one place debugpy gives an agent *less* than codelldb. A breakpoint stop
looks like this, complete:

```json
{"reason":"breakpoint","threadId":1,"preserveFocusHint":false,"allThreadsStopped":true}
```

There are no adapter breakpoint ids in it, so lazydap has none to map back to
its own and reports `hit_breakpoint_ids: []`. The stop still says `breakpoint`
and still says where, so an agent can identify the breakpoint from
`frame.source` and `frame.line`; what it cannot do is branch on *which* of two
breakpoints on the same line was hit. `wait_debugpy.rs` asserts the empty array
rather than skipping the check, so if a future debugpy fills it in, the test
says so.

## 6. An uncaught exception is not a pause

lazydap sends no `setExceptionBreakpoints` filters, and debugpy stops on an
exception only if asked. So a Python program that raises dies the way it would
have died unattended: exit code 1, traceback on stderr, no pause to inspect.

This differs from the C case, where a segfault pauses — but the difference is
in the languages, not in lazydap: a signal is something the debugger sees
whether or not anybody asked for it. Changing this means choosing exception
filters on everyone's behalf; see D054's closing note.

## 7. `console` must be `internalConsole`, and `python` pins the interpreter

Any other value makes debugpy ask for a terminal with a `runInTerminal`
reverse request that lazydap does not advertise, and sends the debuggee's
stdout somewhere lazydap would never read it. codelldb's equivalent is
`terminal: "console"`.

Likewise `subProcess: false`: following a subprocess means debugpy asking us to
open a second debug session with `startDebugging`, and lazydap runs one session
at a time (D007). Reverse requests are refused rather than ignored either way
(D053) — this just avoids provoking them.

A `launch.json` debugpy configuration also routinely names its interpreter, as
`"python"` (a string, or a list whose head is the interpreter) or the older
`"pythonPath"`. lazydap honours it: that pin is the entire point of a
per-project virtualenv, since the named interpreter has the project's
dependencies and the first one on `PATH` does not. It replaces discovery rather
than seeding it, and is checked before the launch — an interpreter that is
missing, or that cannot import debugpy, is an `AdapterNotFound` naming the
configured path and the file that named it, not an adapter that mysteriously
crashed on startup.

## 8. It will not send `initialized` until it has seen `launch`

The strictest sequencer of the two adapters. A client that waits for
`initialized` before sending `launch` deadlocks forever.

lazydap was already safe here: the handshake writes `launch` without awaiting
its response, because codelldb holds *that response* until after
`configurationDone`. The two adapters impose the same ordering for opposite
reasons, and it is the only ordering that satisfies both.

Verified separately that `initialize` **is** answered without a `launch`, so
awaiting the initialize response first — which the handshake does — is safe:

```
>>> initialize seq=1
<<< initialize answered WITHOUT launch: success=True
```

## 9. `continue` on a running program is never answered

Not documented anywhere; found by running the agent loop. If no thread is
paused, debugpy does not answer a `continue` at all — no success, no error.
codelldb acknowledges it and nothing happens.

The sequence that reaches this is ordinary: launch without `--stop-on-entry`,
then `continue --wait` to reach the first breakpoint. The unacknowledged
request then trips the execution timeout, which lazydap correctly treats as a
wedged adapter and kills the session over (D021/D022) — so the symptom is a
dead session, ten seconds after a command that should have worked.

**What lazydap does:** does not send it, deciding that atomically with the
state transition so the pump cannot slip a stop between the two. See D055.

`step` on a running program has the same shape and is *not* handled, because
"step" has no reading that means "wait for whatever happens next". It remains a
way to reach an adapter timeout on both adapters.

## 10. It cleans up its own debuggee when the adapter dies

Kill `debugpy.adapter` mid-session and the debuggee goes with it: the launcher
notices the socket drop and stops the program. By the time lazydap's own reaper
(D045) looks at the pid it recorded, the process is already gone.

Worth knowing for two reasons. The reap is *belt-and-braces* for Python rather
than the only thing between a user and an orphaned process, as it is for
codelldb — and, less comfortably, an integration test that kills the adapter
and then checks for survivors passes whether or not lazydap's reaping works at
all. That check lives in `debuggee.rs`'s unit tests, against a real `python3
script.py` process and a real `ps` line, where it can actually fail.

The identity check itself needed fixing for this adapter: it compared what was
launched against `ps` output as a *prefix*, which is true of codelldb (it execs
the binary) and false of every Python debuggee (`python3 /path/to/main.py`).
The path is now also matched as a whole argument anywhere in the command.

## 11. Noisy events lazydap ignores

`debugpySockets` (repeatedly, as internal ports open and close), `module` for
every import, and `output` events with `category: "telemetry"` carrying
`{"output":"ptvsd"}` / `{"output":"debugpy"}`.

None are modelled. The telemetry ones matter slightly more than the others:
they are `output` events, and `OutputCategory::Telemetry` is already excluded
from `is_debuggee()`, so they do not reach `captured_output`. If that
classification ever changed, every Python session would start with two lines of
adapter branding in the program's output.

## 12. `justMyCode` and the frames it hides

Not a quirk so much as a default lazydap deliberately overrides — debugpy
defaults it to `true`. See D054. The visible cost: the stack at a stop-on-entry
pause includes `runpy` frames, because that is genuinely where the interpreter
is:

```
main    app.py:20
<module>  app.py:26
_run_code  runpy.py:88
_run_module_as_main  runpy.py:203
```
