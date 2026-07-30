---
title: Quickstart
description: Break on a line in a C program, read its variables, and run it to exit.
---

Build a small C program, stop it inside a loop, read the loop counter, and let it finish.
Every command and every reply below was run against the current build; home directories are
rewritten to `/Users/you` and nothing else is edited.

## 1. A program to debug

```bash
mkdir -p ~/lazydap-demo && cd ~/lazydap-demo
cat > hello.c <<'EOF'
#include <stdio.h>

int total(int n) {
    int sum = 0;
    for (int i = 1; i <= n; i++) {
        sum += i;
    }
    return sum;
}

int main(void) {
    printf("starting\n");
    fflush(stdout);
    int answer = total(10);
    printf("total=%d\n", answer);
    return 0;
}
EOF
gcc -g -O0 hello.c -o hello
```

`-g` puts the debug info in; without it there are no source lines to break on. `-O0` stops the
optimiser from moving code around, which keeps line numbers meaning what you think they mean.

:::caution[Not in `/tmp`]
On macOS `/tmp` is a symlink to `/private/tmp`, and a breakpoint set on a program living there
silently never binds. Your home directory is fine. [Quirk 8](/reference/codelldb-quirks/#8-breakpoints-never-bind-under-tmp-on-macos)
has the details.
:::

## 2. Set a breakpoint

Line 6 is `sum += i;`, inside the loop.

```console
$ lazydap break hello.c:6 --format json
{
  "action": "added",
  "applied_to_session": false,
  "breakpoints": [
    {
      "enabled": true,
      "id": 1,
      "line": 6,
      "source": "/Users/you/lazydap-demo/hello.c",
      "verified": false
    }
  ],
  "dry_run": false,
  "not_found": []
}
```

No program is running yet, which is what `"applied_to_session": false` means, and why
`verified` is `false` — nothing has checked whether line 6 is a real place to stop. The
breakpoint is recorded in `.lazydap/state.toml` and will be applied to whatever you launch
next, now or tomorrow.

## 3. Launch, stopped before `main`

```console
$ lazydap launch ./hello --stop-on-entry --format json
{
  "breakpoints": [
    {
      "enabled": true,
      "id": 1,
      "line": 6,
      "message": "Resolved locations: 0",
      "source": "/Users/you/lazydap-demo/hello.c",
      "verified": true
    }
  ],
  "capabilities": {
    "supports_conditional_breakpoints": true,
    "supports_configuration_done_request": true,
    "supports_function_breakpoints": true
  },
  "raw_reason": "exception",
  "reason": "entry",
  "session_id": "3a2c4335-0af9-4363-bf2e-2dc866c9045b",
  "state": "paused",
  "thread_id": 27982711
}
```

Two things in that reply are worth a moment.

**`--stop-on-entry` is doing real work.** Without it, the program starts running the instant
`launch` returns. If it reaches your breakpoint before your next command arrives, that
`continue` resumes from the stop you wanted instead of running to it. Stopping at entry means
the program does not move until you say so.

**`reason` and `raw_reason` disagree on purpose.** codelldb implements entry-stop by sending
the process a `SIGSTOP`, which LLDB files under exceptions, so the adapter reports
`exception`. `reason` is lazydap's normalised answer; `raw_reason` is what the adapter
actually said. Branch on `reason`; read `raw_reason` when you need to know what really
happened.

## 4. Run to the breakpoint

```console
$ lazydap continue --wait --format json
{
  "additional_stopped_threads": [],
  "all_threads_stopped": true,
  "breakpoint_updates": [
    {
      "adapter_id": 1,
      "id": 1,
      "line": 6,
      "message": "Resolved locations: 1",
      "verified": true
    }
  ],
  "captured_output": [
    {
      "category": "console",
      "output": "Launching: /Users/you/lazydap-demo/hello\n",
      "timestamp_ms": 1785446339349
    },
    {
      "category": "stdout",
      "output": "starting\r\n",
      "timestamp_ms": 1785446339847
    }
  ],
  "elapsed_ms": 95,
  "exit_code": null,
  "frame": {
    "column": 16,
    "id": 1001,
    "line": 6,
    "name": "total",
    "source": {
      "name": "hello.c",
      "path": "/Users/you/lazydap-demo/hello.c"
    }
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

Three `console` chunks were elided from `captured_output` above; the `stdout` one is the
program's own `printf`.

This one object is the whole trip: where it stopped (`frame`), why (`reason`), which
breakpoint (`hit_breakpoint_ids`), what the program printed on the way (`captured_output`),
and that the breakpoint went from "resolved 0 locations" to "resolved 1" once the binary was
loaded (`breakpoint_updates`).

Without `--wait`, `continue` returns the moment the debugger accepts the request and tells you
none of that. [The `--wait` contract](/guides/wait/) covers the rest of the outcomes.

## 5. Look around

`scopes` tells you what is in view and hands back the reference numbers `variables` needs:

```console
$ lazydap scopes --format json
{
  "scopes": [
    { "expensive": false, "name": "Local", "variables_reference": 1006 },
    { "expensive": false, "name": "Static", "variables_reference": 1007 },
    { "expensive": false, "name": "Global", "variables_reference": 1008 },
    { "expensive": false, "name": "Registers", "variables_reference": 1009 }
  ]
}
```

```console
$ lazydap variables --reference 1006 --format table
NAME  VALUE  TYPE  REFERENCE
n     10     int   0
sum   0      int   0
i     1      int   0
```

First iteration: `i` is 1 and `sum` is still 0.

:::caution[Reference numbers expire the moment the program moves]
`1006` is good for this stop and no other. After any step or `continue`, run `scopes` again
and use the new numbers. A stale one gets you `DapProtocolError: Invalid variabes reference`,
and the typo is codelldb's.

Do not copy `1006` out of this page — yours will differ. Read it from your own `scopes`.
:::

`eval` runs an expression in the program's own language:

```console
$ lazydap eval 'n * 2' --format json
{
  "type_name": "long long",
  "value": "20",
  "variables_reference": 0
}
```

## 6. Step, and finish

```console
$ lazydap step --wait --format json
{
  "elapsed_ms": 56,
  "frame": {
    "column": 5,
    "id": 1005,
    "line": 7,
    "name": "total",
    "source": { "name": "hello.c", "path": "/Users/you/lazydap-demo/hello.c" }
  },
  "hit_breakpoint_ids": [],
  "reason": "step",
  "state": "paused",
  "thread_id": 27982711
}
```

Line 6 to line 7. The breakpoint is still on line 6, so `continue` would come straight back
here on the next iteration — nine more times. Take it out first:

```console
$ lazydap break --remove --all --dry-run --format json
{
  "action": "removed",
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6,
      "message": "Resolved locations: 1",
      "source": "/Users/you/lazydap-demo/hello.c", "verified": true }
  ],
  "dry_run": true,
  "not_found": []
}

$ lazydap break --remove --all --format json
{
  "action": "removed",
  "applied_to_session": true,
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6,
      "source": "/Users/you/lazydap-demo/hello.c", "verified": false }
  ],
  "dry_run": false,
  "not_found": []
}
```

`--dry-run` reported exactly what the real run then removed. Every lazydap mutation takes it,
and it uses the same selection logic as the real thing rather than a second implementation
that might disagree.

```console
$ lazydap continue --wait --format json
{
  "captured_output": [
    { "category": "stdout", "output": "total=55\r\n", "timestamp_ms": 1785446390991 },
    { "category": "console", "output": "Process exited with code 0.\n", "timestamp_ms": 1785446390992 }
  ],
  "elapsed_ms": 1,
  "exit_code": 0,
  "frame": null,
  "reason": null,
  "state": "exited",
  "thread_id": null
}
```

`"state": "exited"` with `"exit_code": 0`. There is no frame, because there is no longer a
program to have one. The answer the program computed is in `captured_output`.

Read `state` to find out what happened to the program. The command's own exit code tells you
whether the *command* worked, which is a different question — `lazydap continue` exits `0`
here even though it is reporting the program's death.

## 7. Tidy up

```bash
lazydap disconnect          # end the session, leave the daemon running
```

## Next

- [The `--wait` contract](/guides/wait/) — every outcome, and what times out
- [Breakpoints](/guides/breakpoints/) — conditions, hit counts, log points, persistence
- [The TUI](/getting-started/tui/) — the same session, on a screen
- [Debug with an agent](/guides/agents/) — hand this loop to a model
