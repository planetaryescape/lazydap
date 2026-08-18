---
name: lazydap
description: |
  Debug a program from the shell with lazydap: launch it under a debugger, set
  breakpoints, step, read the stack and variables, and evaluate expressions in
  the running process. Every command is a shell subcommand that prints JSON, so
  it works in any agent that can run a command — no server, no protocol host.
  Use when asked to find why a program crashes, what a variable holds at some
  line, or what actually happens at runtime rather than what the source
  suggests. Debugs C, C++ and Rust binaries via codelldb, Python via debugpy
  and Go via delve. The program's extension picks the adapter — `.py` debugpy,
  `.go` delve, anything else codelldb — and `--adapter` overrides it.
lazydap_min_version: "0.2.6"
---

# lazydap

A scriptable, terminal-first debugger. You drive it exactly as a human would:
run a subcommand, read the JSON, decide what to do next.

## The one thing to get right

**Use `--wait` on every command that moves the program.** Without it, the
command returns the instant the debugger accepts the request, before the
program has gone anywhere, and you will read a stale stack.

With `--wait`, lazydap blocks until the program reaches a stable state and
returns **one JSON object describing everything that happened on the way** —
where it stopped, why, the top frame, and every line the program printed. It
also carries `locals`, the variables in scope where it stopped, and
`user_frame`, the nearest frame in code you wrote when it stopped inside a
library. That single object is usually all you need: reading a local costs no
`scopes` and no `variables` call. Do not go fishing for the pieces separately.

```bash
lazydap continue --wait --format json
```

## The loop

```bash
# 1. Start the program, stopped before it runs.
lazydap launch ./mybinary --stop-on-entry --format json

# 2. Say where you want to stop. Paths are relative to your shell.
lazydap break src/parser.c:142 --format json

# 3. Run to it. Read the blob this returns.
lazydap continue --wait --format json

# 4. Look around. Only meaningful while paused.
lazydap stack --format json
lazydap scopes --format json
lazydap eval "tokens[pos]" --format json

# 5. Finish.
lazydap disconnect --format json
```

Worked examples, including a crash investigation:
[`references/examples.md`](references/examples.md).

## When the repository already knows how to run itself

Before working out a program's arguments yourself, ask:

```bash
lazydap launches list --format json
```

It reads `.vscode/launch.json` and `.lazydap/state.toml` and reports each
configuration with a `runnable` flag — false for the ones lazydap cannot start
(another debugger's adapter, an `attach`, a `${...}` variable nothing here can
expand), with the reason next to it. Start a runnable one by name, and its
program, arguments, working directory and environment come from the file:

```bash
lazydap launches run "Debug binary" --stop-on-entry --format json
```

## Things that will otherwise cost you a turn

- **`--stop-on-entry` stops before `main`.** The stack says `_dyld_start` and
  none of your variables exist yet — `eval` will report *undeclared
  identifier*. That is expected. Set a breakpoint and `continue --wait` before
  inspecting anything.
- **Inspection needs a paused program.** `stack`, `scopes`, `variables` and
  `eval` fail with `SessionNotPaused` while it is running. Pause it first
  (`lazydap pause --wait`) or wait for a breakpoint.
- **A `variables_reference` stops being valid the moment the program moves.**
  The numbers `scopes` hands you are good for that stop only. After any step or
  `continue`, ask `scopes` again and use the new ones; reusing an old one fails
  with `StaleHandle`, which names the stop it came from. The same goes for a
  `frame_id` from `stack`.
- **One session at a time.** Launch again and you get `SessionAlreadyActive`,
  unless the previous program has finished — a finished session is cleared
  automatically.
- **Breakpoints outlive sessions.** They are project state, kept in
  `.lazydap/state.toml`, and applied to every later launch. Set them before
  launching if you like. Remove the ones you added when you are done.
- **Breaking on a line that already has a breakpoint edits it.** The reply says
  `"action": "updated"` (or `"unchanged"`) rather than `"added"`, and the whole
  request wins: `lazydap break x.c:10` with no `--condition` clears a condition
  you set earlier, on the same id.
- **`eval` evaluates an expression** in the program's language. It does *not*
  run debugger commands unless you ask for that with `--context repl`.
- **Read `state`, not the exit code, to know what happened.** Exit `0` means
  the command worked; `"state": "exited"` means the *program* finished.

## Output

`--format json` gives one JSON object. Without a `--format`, lazydap prints a
human table on a terminal and JSON everywhere else — so in a pipeline you get
JSON either way, but say `--format json` and never think about it.

`--format ids` prints bare ids one per line, for piping:

```bash
lazydap break --list --format ids | xargs -I{} lazydap break --remove --id {}
```

Also available: `jsonl` (one object per line), `csv`, `table`.

## Reference

- [`references/commands.md`](references/commands.md) — every command and flag,
  generated from lazydap itself
- [`references/output-schemas.md`](references/output-schemas.md) — the JSON
  each command returns, field by field
- [`references/error-codes.md`](references/error-codes.md) — exit codes, error
  names, and what to do about each
- [`references/examples.md`](references/examples.md) — worked sessions

## What you never need to know

The Debug Adapter Protocol, the daemon, the Unix socket, or which adapter is
running. lazydap starts what it needs on first use and cleans up after itself.
If you find yourself needing any of it, the tool is wrong — say so rather than
working around it.
