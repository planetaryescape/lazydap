---
title: Troubleshooting
description: Symptoms, causes and fixes for the ways lazydap and codelldb go wrong.
---

Find your symptom, apply the fix. Start with `lazydap doctor --format json` — it checks the
three things that are usually wrong.

```console
$ lazydap doctor --format json
{
  "checks": [
    { "detail": "/Users/you/.local/bin/codelldb", "name": "adapter.codelldb", "ok": true },
    { "detail": "/Users/you/lazydap-demo/.lazydap/state.toml (not created yet)", "name": "state.file", "ok": true },
    { "detail": "instance lazydap-demo-13cc8efcde46, pid 12102, protocol v4", "name": "daemon", "ok": true }
  ],
  "ok": true
}
```

## A breakpoint never stops anything

The program runs to completion and `hit_breakpoint_ids` is empty.

**Check `verified` first.** It is in `launch`'s reply and in `breakpoint_updates` on any
`--wait` reply:

```bash
lazydap break --list --format table
```

`verified: false` after a launch means the breakpoint is not attached to anything. Three
causes, in order of likelihood:

**1. The program is under `/tmp` on macOS.** `/tmp` is a symlink to `/private/tmp`, lazydap
sends the resolved path, the compiler recorded the unresolved one, and codelldb compares them
as strings. Move the program under `$HOME` and rebuild. Full explanation:
[quirk 8](/reference/codelldb-quirks/#8-breakpoints-never-bind-under-tmp-on-macos).

**2. The binary has no debug info.** Rebuild with `-g`, and `-O0` while you are there — an
optimiser can move or delete the line you asked about.

```bash
gcc -g -O0 hello.c -o hello
```

**3. The line has no code.** A blank line, a comment, or a declaration the compiler emitted
nothing for. Pick the next line that does something.

## `SessionNotPaused`

```json
{"error":"SessionNotPaused","message":"... is running; pause it first (`lazydap pause --wait`) or wait for a breakpoint"}
```

`stack`, `scopes`, `variables` and `eval` need a stopped program. Either pause it:

```bash
lazydap pause --wait --format json
```

or set a breakpoint and `continue --wait` to it.

This most often follows a `--wait` that returned `"state": "timeout"`. A timeout does not
pause anything — the program is still running, and that is deliberate.

## `Invalid variabes reference`

```json
{"error":"DapProtocolError","message":"... the adapter rejected `variables`: Internal debugger error: Invalid variabes reference"}
```

The typo is codelldb's. The cause is a stale `variables_reference`: those numbers are valid
only for the stop they came from, and any step or continue invalidates them.

Call `scopes` again after every stop and use the new numbers.

## `SessionAlreadyActive`

One session per daemon. End the current one:

```bash
lazydap disconnect
```

Or run the second program under a different daemon:

```bash
LAZYDAP_INSTANCE=other lazydap launch ./other-program
```

A session whose program already finished is cleared automatically, so this usually means
something is still paused somewhere.

## `VersionMismatch`

A daemon from an earlier build is still running, typically right after a rebuild.

```bash
lazydap shutdown
```

The next command starts a current one. Nothing is lost except the live session; breakpoints
are on disk.

## `AdapterNotFound`

codelldb is not on `PATH`. `details.searched` lists where lazydap looked.

Adapter discovery is `PATH`-only today — the config file that would let you point at an
arbitrary binary is specified and not built. Follow [the install
guide](/getting-started/install/), and check the wrapper script is executable:

```bash
codelldb --help
```

## codelldb dies immediately, or `dlopen` fails

```text
called Result::unwrap() on an Err value: "dlopen(.../liblldb.dylib, ...) (no such file)"
```

You installed codelldb as a symlink. It resolves `liblldb` relative to `argv[0]`, so a symlink
sends it one directory too high. Replace it with a wrapper script —
[quirk 1](/reference/codelldb-quirks/#1-a-symlink-on-path-breaks-liblldb-resolution) has the
exact three lines.

## Every codelldb invocation hangs, including `--help`

No output at all, forever, right after a macOS update. macOS has a stale per-inode security
record for that file.

Re-copy the install so every file gets a new inode:

```bash
rm -rf ~/.local/opt/codelldb
cp -R <source>/extension ~/.local/opt/codelldb/extension
timeout 8 codelldb --help
```

Details: [quirk 5](/reference/codelldb-quirks/#5-every-invocation-hangs-after-a-macos-update).

## `eval` says "use of undeclared identifier"

The variable is not in scope where the program is stopped. This is near-universal right after
`--stop-on-entry`, which stops before `main` — the stack says `_dyld_start` and none of your
program's variables exist yet.

Set a breakpoint past the declaration and `continue --wait` before evaluating anything.

If instead you get a *memory read* error, you are on `--context repl`, which runs an LLDB
command rather than evaluating an expression. The default is `watch` and does the right thing
— [quirk 7](/reference/codelldb-quirks/#7-eval-with-context-repl-runs-an-lldb-command).

## `--stop-on-entry` reports `"reason": "exception"`

It is not an error. codelldb implements entry-stop with a `SIGSTOP`, which LLDB files as an
exception. lazydap reports the normalised `"reason": "entry"` and keeps the adapter's word in
`raw_reason`. Branch on `reason`.

## A `--wait` returned `"state": "timeout"`

Nothing settled inside the window, and **the program is still running**. Either it is slow or
it is stuck, and lazydap does not guess.

```bash
lazydap continue --wait --timeout 120     # give it longer
lazydap continue --wait --timeout 0       # wait forever; now it is your problem
lazydap pause --wait                      # stop it and look at where it got to
```

`LAZYDAP_TIMEOUT` changes the 30-second default for every command.

## The TUI lost the daemon

It reconnects on its own, and starts a daemon if there is none — so a `lazydap shutdown` or a
crash resolves itself. If it stays stuck, quit with `q` and start it again, then check
`lazydap logs --level warn` for what the daemon made of it.

## Nothing above matches

Read the daemon's log. It records what the daemon and the adapter said to each other, which is
usually more specific than the error that reached your terminal:

```bash
lazydap logs --level warn
lazydap logs --limit 50
```

Then [open an issue](https://github.com/planetaryescape/lazydap/issues) with that output, your
platform, and `lazydap version --format json`.

## See also

- [codelldb quirks](/reference/codelldb-quirks/) — the same failures, with causes
- [Errors and exit codes](/reference/errors/) — organised by error name
- [The daemon](/guides/daemon/) — logs, paths, instances
