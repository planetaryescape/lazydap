---
title: Solve common debugging problems
description: Ten worked recipes, from finding a segfault to asserting runtime state in CI.
---

{/* Watch expressions (M16) and the REPL pane (M17) landed in Wave 7, after this
    page was written, and have no recipe yet. Add one here rather than expanding
    the breakpoints guide — and verify it against the binary first, as every
    other recipe on this page was. */}

Each recipe is a goal, the shortest path to it, and what to look at when it does not work.
Every command was run against the current build before it went on the page; output is real,
with home directories rewritten to `/Users/you` and long blobs trimmed where marked.

For what a flag means rather than how to reach a goal, go to the
[CLI reference](/reference/cli/). These pages do not repeat each other.

## Find out why a program segfaults

**You have a binary that crashes and no idea which line does it.**

Launch it and let it run. A crash is a stop like any other, so `--wait` returns when it
happens.

```bash
gcc -g -O0 crashes.c -o crashes
lazydap launch ./crashes --format json
lazydap continue --wait --format json
```

The reply says where and what, trimmed here to the fields that matter:

```json
{
  "captured_output": [
    { "category": "stdout", "output": "about to crash\r\n", "timestamp_ms": 1785472019334 },
    { "category": "stderr", "output": "Stop reason: EXC_BAD_ACCESS (code=1, address=0x0)\n",
      "timestamp_ms": 1785472019336 }
  ],
  "frame": {
    "column": 14, "id": 1001, "line": 8, "name": "main",
    "source": { "name": "crashes.c", "path": "/Users/you/crashes.c" }
  },
  "reason": "exception",
  "state": "paused",
  "thread_id": 30361859
}
```

`"reason": "exception"` with `address=0x0` is a null dereference, and `frame.line` is the
line that did it. The program is still paused there, so ask what the pointer held:

```console
$ lazydap eval 'nowhere' --format json
{
  "type_name": "int *",
  "value": "<null>",
  "variables_reference": 1010
}
```

**If it fails.** `state: "terminated"` with no frame means the process died outright rather
than stopping — usually a crash the adapter could not catch, and `captured_output` is then
all the evidence there is. If `eval` says *use of undeclared identifier*, you are not in the
frame you think: run [`lazydap stack`](/reference/cli/stack/) and check which function is on
top.

## Stop the fiftieth time a line runs

**A loop is fine for the first few hundred iterations and wrong later.** Stopping by hand is
not an option.

Give the breakpoint a hit condition:

```bash
lazydap break chatty.c:6 --hit-condition '>= 50'
lazydap launch ./chatty --stop-on-entry
lazydap continue --wait
```

It stops on the fiftieth arrival and not before:

```console
$ lazydap variables --reference 1003 --format table
NAME  VALUE  TYPE  REFERENCE
done  1      int   0
i     49     int   0
```

`i` is 49 because the loop counts from zero — the fiftieth hit is `i == 49`, and exactly 49
lines had been printed by then.

Use `--condition` instead when you know the value rather than the count:

```bash
lazydap break chatty.c:6 --condition 'i == 120'
```

`--condition` takes an expression in the program's own language. `--hit-condition` counts
arrivals and takes `>= n`, `== n` or `% n`.

**If it fails.** A condition that cannot compile is reported by the adapter, not by lazydap —
check `message` on the breakpoint after launching. A condition referring to a variable that
is not in scope at that line never fires and never errors, which looks identical to a
condition that is never true. Set a plain breakpoint on the same line first and confirm it
stops at all.

## Capture everything a program prints while still stopping at a breakpoint

**You want the program's output and a breakpoint, and running it twice would change what it
prints.**

You do not have to choose. Every `--wait` reply carries the output produced during that
wait, and [`lazydap output`](/reference/cli/output/) carries the whole session's.

```bash
lazydap break chatty.c:6 --hit-condition '>= 50'
lazydap launch ./chatty --stop-on-entry
lazydap continue --wait --format json    # stops at hit 50
lazydap break --remove --all
lazydap continue --wait --format json    # runs to exit
```

Read the program's own lines out of either one, filtering the adapter's chatter:

```bash
lazydap output --format json \
  | jq -r '.chunks[] | select(.category=="stdout") | .output'
```

```console
$ lazydap output --format json | jq '{chunks: (.chunks|length), dropped}'
{
  "chunks": 66,
  "dropped": 0
}
```

**A chunk is not a line.** Those 66 chunks hold 200 printed lines: the adapter batches
whatever arrived together, so one chunk routinely contains dozens of newline-separated
lines. Join the `output` strings and split on newlines rather than treating each chunk as a
line.

**If it fails.** `dropped` above zero, or `output_truncated: true` on a `--wait` reply, means
the buffer filled before anything read it — the program printed faster than it was drained,
and what you have is incomplete. Read output at each stop instead of only at the end. Missing
the last few lines usually means the program did not flush; that is the program's behaviour,
not lazydap's.

## Debug a Python script

**The program is Python, not a compiled binary.** Same loop, different adapter.

Point `--adapter` at debugpy. Everything downstream is identical, because the adapter seam is
what varies and the commands are not.

```bash
lazydap break total.py:4
lazydap launch ./total.py --adapter debugpy --stop-on-entry
lazydap continue --wait
```

```console
$ lazydap scopes --format table
NAME     REFERENCE  EXPENSIVE
Locals   3          false
Globals  4          false

$ lazydap variables --reference 3 --format table
NAME  VALUE  TYPE  REFERENCE
i     1      int   0
n     10     int   0
sum   0      int   0

$ lazydap eval 'sum * 10' --format json
{
  "type_name": "int",
  "value": "0",
  "variables_reference": 0
}
```

`eval` runs the expression in Python, so Python syntax is what it wants — the same command
that took a C expression a section ago.

Two differences worth expecting rather than debugging:

- **Scope names differ.** debugpy offers `Locals` and `Globals` where codelldb offers `Local`,
  `Static`, `Global` and `Registers`. Read the names from `scopes` rather than assuming them.
- **`hit_breakpoint_ids` can be empty.** The stop reports `"reason": "breakpoint"` at the right
  line, but debugpy does not always say which breakpoint caused it. Branch on `reason` and
  `frame.line`, not on the id list being non-empty.

**If it fails.** `--adapter` only accepts adapters this build ships; an unknown name is a usage
error with exit `2` that names what it does support. debugpy has to be importable by the
interpreter that runs your script (`python3 -c 'import debugpy'`), and it is looked up
independently of codelldb — `lazydap doctor` reports each adapter separately.

## Run a configuration from your team's `.vscode/launch.json`

**Your repository already describes how to launch the thing.** You would rather not retype it
as flags.

lazydap reads `.vscode/launch.json` and never writes it.

```console
$ lazydap launches list --format table
NAME                 SOURCE       ADAPTER  REQUEST  PROGRAM                    RUNNABLE
Debug crashes        launch.json  lldb     launch   /Users/you/crashes         yes
Attach to something  launch.json  lldb     attach   -                          no (it attaches to a running process, which lazydap cannot do yet)
```

Then run one by name:

```bash
lazydap launches run "Debug crashes"
```

`${workspaceFolder}` and friends are resolved before the program path is used, which is why
the table shows an absolute path rather than the string in the file.

**If it fails.** The `RUNNABLE` column is the first thing to read, and it explains itself.
Running a config that says no gets you the same sentence as an error, with exit code `1`:

```console
$ lazydap launches run "Attach to something" --format json
{"details":{"adapter_type":"lldb","name":"Attach to something","source":"launch.json","unresolved":[]},"error":"InvalidLaunchConfig","message":"InvalidLaunchConfig: cannot run `Attach to something`: it attaches to a running process, which lazydap cannot do yet"}
```

In `--format json`, each config also carries `unresolved` — variables lazydap could not
substitute — and the top level carries `warnings` for configs it skipped entirely.

## Assert runtime state from a CI job

**You want a test that fails when a variable is wrong at a particular line**, which no unit
test can see.

Drive lazydap the way you would any other CLI: `--format json`, `jq`, and exit codes.

```bash
#!/usr/bin/env bash
# Fails unless ./crashes stops at crashes.c:8 with a null `nowhere`.
set -euo pipefail

lazydap break crashes.c:8 --format json >/dev/null
trap 'lazydap break --remove --all --format json >/dev/null 2>&1 || true' EXIT

lazydap launch ./crashes --stop-on-entry --format json >/dev/null

result=$(lazydap continue --wait --timeout 30 --format json)
state=$(printf '%s' "$result" | jq -r .state)

if [ "$state" != "paused" ]; then
  echo "expected to stop, got state=$state" >&2
  exit 1
fi

line=$(printf '%s' "$result" | jq -r .frame.line)
value=$(lazydap eval 'nowhere' --format json | jq -r .value)

echo "stopped at line $line with nowhere=$value"
[ "$line" = "8" ] && [ "$value" = "<null>" ]
```

```console
$ bash assert-line.sh
stopped at line 8 with nowhere=<null>
$ echo $?
0
```

Four things make this survive CI rather than only working on your laptop:

- **`--stop-on-entry`, always.** Without it the program starts the moment `launch` returns and
  can finish before your `continue` arrives. You then wait on a program that is already gone.
- **An explicit `--timeout`.** A hung debuggee should fail the job, not occupy the runner.
- **`trap` for breakpoint cleanup.** Breakpoints are project state and outlive the process
  that set them, so a failed run otherwise leaves them behind for the next one.
- **Branch on `state`, not on the exit code.** `lazydap continue --wait` exits `0` when it
  successfully reports that your program crashed. The exit code tells you the command worked;
  `state` tells you what the program did.

**If it fails.** `set -e` will abort on the first lazydap command that exits non-zero, which
is what you want everywhere except a check you expect to fail. Exit `3` means no daemon could
be started — on a fresh runner that usually means the working directory is not what you think,
since the daemon is per project root. Exit `4` means codelldb is not installed on the runner.
Both are listed on [errors and exit codes](/reference/errors/).

## Work out why a breakpoint never stops anything

**The program runs to completion and `hit_breakpoint_ids` is empty.**

Read `verified`, which `launch` reports for every breakpoint it applied:

```bash
lazydap break --list --format table
lazydap launch ./program --stop-on-entry --format json | jq '.breakpoints'
```

Three causes, in the order worth checking.

**The binary has no debug info.** The clearest signal there is:

```console
$ lazydap launch ./chatty_nodebug --format json | jq '.breakpoints[0]'
{
  "verified": false,
  "message": "Resolved locations: 0",
  ...
}
```

"Resolved locations: 0" means the adapter found nowhere to put it. Rebuild with `-g`, and
`-O0` while you are there, since an optimiser can move or delete the line you asked about.

**The line has no code.** A blank line, a comment, or a declaration that emitted nothing.
Move to the next line that does something.

**The breakpoint bound but the program outran you.** This one is deceptive, because
`verified` is `true` and `message` says "Resolved locations: 1" — and the program still
exits without stopping:

```console
$ lazydap launch ./chatty --format json | jq '.breakpoints[0].verified'
true
$ lazydap continue --wait --format json | jq '{state, hit_breakpoint_ids}'
{
  "state": "exited",
  "hit_breakpoint_ids": []
}
```

Nothing is broken. `launch` without `--stop-on-entry` starts the program immediately, it ran
past the breakpoint and finished before `continue` was sent, and `continue` then resumed a
program that had already exited. Add `--stop-on-entry` and it stops properly.

**If none of those.** Working under a symlinked directory used to break this on macOS and no
longer does — `/tmp` resolves and binds correctly now. The forensic write-ups for the
failures that remain are in [codelldb quirks](/reference/codelldb-quirks/).

## Pin a specific codelldb build

**Two machines behave differently, or you need a codelldb that is not on `PATH`.**

Adapter discovery checks lazydap's config first. Create `~/.config/lazydap/config.toml`:

```toml
[adapter.codelldb]
command = "/Users/you/.local/opt/codelldb-1.12.2/extension/adapter/codelldb"

[general]
wait_timeout_seconds = 60
```

`lazydap doctor` tells you whether it was read and which binary won:

```console
$ lazydap doctor --format json
{
  "checks": [
    { "detail": "/Users/you/project/lazydap-config.toml (read)", "name": "config.file", "ok": true },
    { "detail": "/Users/you/.local/opt/codelldb/extension/adapter/codelldb", "name": "adapter.codelldb", "ok": true },
    { "detail": "/opt/homebrew/bin/python3", "name": "adapter.debugpy", "ok": true },
    { "detail": "/Users/you/go/bin/dlv", "name": "adapter.delve", "ok": true },
    { "detail": "/Users/you/project/.lazydap/state.toml (1 breakpoints)", "name": "state.file", "ok": true },
    { "detail": "instance cookbook-pin2, pid 54121, protocol v7", "name": "daemon", "ok": true }
  ],
  "ok": true
}
```

`adapter.codelldb` reports the pinned path rather than the one on `PATH`, which is how you know
the pin took. Each adapter is checked separately, so a broken codelldb pin does not hide a
working debugpy or delve.

`LAZYDAP_CONFIG_PATH` points at a different file, which is the tidy way to try a pin without
editing your real config.

**If it fails.** A pin at a path that is not executable is named directly rather than falling
back to `PATH`, so a typo cannot silently give you a different debugger:

```console
$ lazydap doctor --format json
{"details":{},"error":"DaemonInternalError","message":"1 check(s) failed"}
```

The same run in `table` form says which check failed and why:

```text
CHECK             STATUS  DETAIL
config.file       ok      /Users/you/project/bad-config.toml (read)
adapter.codelldb  FAILED  codelldb is pinned to /nope/codelldb by lazydap's config, and that is not an executable
adapter.debugpy   ok      /opt/homebrew/bin/python3
adapter.delve     ok      /Users/you/go/bin/dlv
state.file        ok      /Users/you/project/.lazydap/state.toml (1 breakpoints)
daemon            ok      instance cookbook-pin2, pid 53986, protocol v7
error: 1 check(s) failed
```

The config file is per user, never per project — project state lives in `.lazydap/state.toml`.
Unknown keys are accepted and skipped, so only `[adapter.<name>] command` and
`[general] wait_timeout_seconds` do anything in this build.

## Trace a variable over time and find its peak

**You want to know how a value evolves — where it peaks, when it drops — but the program never
prints it.** Stopping at a breakpoint on every iteration would work and would be slow. A log
point samples the value on every pass without pausing, so one `continue --wait` brings back
the whole series.

Given a loop whose body updates `v` on line 8:

```console
$ lazydap break signal.c:8 --log "i={i} v={v}" --format json
$ lazydap launch ./signal --stop-on-entry --format json
$ lazydap continue --wait --format json > run.json
```

The program runs to completion at full speed. Every sample is in the blob's
`captured_output`, one interpolated line per pass:

```json
{ "category": "console", "output": "i=29 v=97.533029379599526\n", ... }
{ "category": "console", "output": "i=30 v=100.90572645692235\n", ... }
{ "category": "console", "output": "i=31 v=97.601712495361525\n", ... }
```

Extract and analyse with anything that reads JSON — here, the peak:

```console
$ jq -r '.captured_output[].output' run.json \
    | grep -o 'i=[0-9]* v=[0-9.-]*' \
    | sort -t= -k3 -g | tail -1
i=30 v=100.90572645692235
```

Feed the same series to gnuplot, matplotlib, or a spreadsheet via `--format csv` thinking:
the data is already structured, so charting is whatever tool you have. An agent can render
the chart itself.

A chunk of `captured_output` is not one line — fast loops batch many samples into one chunk,
so split on newlines, not on chunks. Braces interpolate variables in the paused frame's
scope; for expressions log points cannot express (struct fields through pointers, function
calls), fall back to a breakpoint plus a `continue --wait` loop with `eval` at each stop —
slower, but the expression runs with full debugger power.

**If it fails:** an unverified log point (`"verified": false` before launch) binds during
launch like any breakpoint — check `breakpoint_updates` in the first blob. No samples at all
usually means the line is never reached: confirm with a plain breakpoint first. And if the
series is truncated, check `output_truncated` in the blob — a chatty enough loop can overflow
the buffer, in which case sample less often with `--condition "i % 10 == 0"`.

## Clean up after yourself

**Breakpoints outlive sessions and daemons outlive commands.** Leaving both behind means the
next run stops in places nobody asked for.

Preview before removing. Every mutation takes `--dry-run`, and it uses the same selection
logic the real removal uses:

```console
$ lazydap break --list --format table
ID  LOCATION     ENABLED  VERIFIED  CONDITION
4   chatty.c:6   true     false     -
5   crashes.c:5  true     true      -
6   chatty.c:10  true     false     -

$ lazydap break --remove --all --dry-run --format json | jq '{dry_run, ids: [.breakpoints[].id]}'
{
  "dry_run": true,
  "ids": [4, 5, 6]
}
```

Then remove them, all at once or by id:

```bash
lazydap break --remove --all
lazydap break --list --format ids | xargs -I{} lazydap break --remove --id {}
```

The second form is worth knowing because `--format ids` and `--id` are built to feed each
other, and it generalises to any subset you can filter.

End the session, the daemon, or both:

```bash
lazydap disconnect            # end the session, leave the daemon up
lazydap shutdown --dry-run    # what would this stop?
lazydap shutdown              # stop the daemon and every session it owns
```

```console
$ lazydap shutdown --dry-run --format json | jq '{dry_run, sessions: (.sessions|length)}'
{
  "dry_run": true,
  "sessions": 1
}
```

Nothing needs starting again afterwards; the next command spawns a daemon.

**If it fails.** `xargs -r` is GNU-only and does nothing on macOS, so guard an empty list
yourself rather than relying on it. Breakpoints removed while a session is live also apply to
that session — `applied_to_session` in the reply says whether that happened.

## See also

- [Breakpoints](/guides/breakpoints/) — conditions, log points and persistence in full
- [The `--wait` contract](/guides/wait/) — every outcome a stepping command can return
- [Output formats and piping](/guides/output-formats/) — the five formats and how they compose
- [Errors and exit codes](/reference/errors/) — what each failure means
- [Troubleshooting](/troubleshooting/) — symptoms rather than goals
