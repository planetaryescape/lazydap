---
title: Breakpoints
description: Set, condition, list and remove breakpoints that outlive the session.
---

Set a breakpoint with `lazydap break file:line`. It is written to `.lazydap/state.toml` in
your project and applied to every session you launch afterwards, whether or not one is
running when you set it.

```bash
lazydap break hello.c:6
```

## They are project state, not session state

This is the part that differs from most debuggers. A breakpoint set today applies to the
program you launch tomorrow, and to the one you launch after restarting the daemon. There is
no separate "save breakpoints" step, and no session to attach them to first.

```console
$ cat .lazydap/state.toml
version = 1
next_breakpoint_id = 3

[[breakpoints]]
id = 2
source = "hello.c"
line = 6
condition = "i == 7"
enabled = true
```

TOML rather than a database, so it diffs, greps, and can be written by anything. Commit it if
your team wants shared breakpoints; the repository's own `.gitignore` leaves that choice to
you.

### Editing it by hand

You can, including while a daemon is running. lazydap compares the file against the version it
last read or wrote, so it can tell what you changed:

- an entry you **add** is adopted, and its id is reserved so nothing else takes it;
- an entry you **delete** stays deleted — it is not written back on the next save;
- an existing id's **fields are lazydap's**, because a live adapter has already been told
  about it. Change one and lazydap's version wins; set it again with `lazydap break` and the
  file is right immediately;
- a file that is **missing, empty, or only whitespace** is not read as "delete everything".
  `rm -rf .lazydap` as a reset, or an editor caught between truncating and writing, leaves
  lazydap holding what it had, and it writes the file back out.

A deletion applies from the next launch. It is not withdrawn from a program that is already
running, so a `--wait` blob can still mention an id you have just removed from the file.
Watch expressions follow all of the same rules.

## Verified means bound

`verified` is the field to read. When you set a breakpoint with nothing running, nothing has
checked whether the line exists:

```console
$ lazydap break hello.c:6 --format json
{
  "action": "added",
  "applied_to_session": false,
  "breakpoints": [
    { "enabled": true, "id": 1, "line": 6,
      "source": "/Users/you/lazydap-demo/hello.c", "verified": false }
  ],
  "dry_run": false,
  "not_found": []
}
```

`launch` applies it and the adapter decides. Watch it change in `breakpoint_updates` on the
next `--wait` reply:

```json
"breakpoint_updates": [
  { "adapter_id": 1, "id": 1, "line": 6,
    "message": "Resolved locations: 1", "verified": true }
]
```

"Resolved locations: 1" is the good case. **A breakpoint that stays `verified: false` will
never stop anything**, and lazydap reports it rather than failing, per DAP. If a program runs
straight past a line you know it reaches, read `verified` first —
[the troubleshooting page](/troubleshooting/#a-breakpoint-never-stops-anything) lists the
three usual causes.

## Conditions

`--condition` takes an expression in the debuggee's language, evaluated at the breakpoint. The
program only stops when it is true.

```console
$ lazydap break hello.c:6 --condition 'i == 7' --format json
{
  "action": "added",
  "breakpoints": [
    { "condition": "i == 7", "enabled": true, "id": 2, "line": 6,
      "source": "/Users/you/lazydap-demo/hello.c", "verified": false }
  ],
  "dry_run": false,
  "not_found": []
}
```

Launching and continuing then stops on the seventh iteration and nowhere else:

```console
$ lazydap variables --reference 1003 --format table
NAME  VALUE  TYPE  REFERENCE
n     10     int   0
sum   21     int   0
i     7      int   0
```

`sum` is 21, which is 1+2+3+4+5+6 — the condition held exactly once, before the seventh
addition.

`--hit-condition` counts hits instead of testing a value: `--hit-condition '>= 10'` stops on
the tenth arrival and after.

`--log` turns a breakpoint into a log point. The program does not stop; the message is emitted
instead, with braces interpolated from the debuggee's scope:

```bash
lazydap break hello.c:6 --log "i = {i}, sum = {sum}"
```

Log-point output arrives in `captured_output` on the next
[`--wait`](/guides/wait/) reply, like anything else the program produced.

Conditions can be set from the CLI but not yet from the TUI.

## Listing and selecting

```console
$ lazydap break --list --format table
ID  LOCATION   ENABLED  VERIFIED  CONDITION
1   hello.c:6  true     true      -
```

`--format ids` prints bare ids for piping:

```console
$ lazydap break --list --format ids
1
```

Selection for `--remove` and `--toggle` is by `--id` (repeatable) or `--all`:

```bash
lazydap break --toggle --id 1              # disable, or re-enable
lazydap break --remove --id 1 --id 2
lazydap break --remove --all
```

## Dry-run first

Every mutation takes `--dry-run` and reports what it would have done:

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
```

The `breakpoints` array is the set that would be removed, chosen by the same selection code
the real removal uses — not a second implementation that could disagree with it. Drop
`--dry-run` and that is what goes.

`not_found` lists ids you asked for that do not exist, so a script can tell "removed nothing
because there was nothing" from "removed nothing because I typo'd the id".

## Paths

Paths are relative to your shell, and lazydap canonicalises them before sending them on. That
is what keeps the daemon and the adapter agreeing about a file regardless of anyone's working
directory, and it is why a typo fails immediately instead of as a silent `verified: false`
later.

The exception is symlinked directories, where canonicalising is the thing that breaks it —
see [quirk 8](/reference/codelldb-quirks/#8-breakpoints-never-bind-under-tmp-on-macos) and
keep debuggees out of `/tmp` on macOS.

## See also

- [`lazydap break`](/reference/cli/break/) — every flag
- [Quickstart](/getting-started/quickstart/) — a breakpoint end to end
- [Troubleshooting](/troubleshooting/) — when one does not bind
