---
title: Debug with an agent
description: Point a coding agent at the lazydap skill and let it inspect a running program.
---

Give an agent the `lazydap.skill` bundle and it drives the debugger with the same commands you
would type. No MCP host, no server, no protocol adapter — if the agent can run a shell
command, it can inspect a running program.

## Install the skill

`lazydap.skill` sits at the root of the repository. It is a zip containing `SKILL.md` plus
generated references for commands, output schemas, error codes and worked sessions. Point your
agent's skill loader at it.

The command reference inside it is generated from lazydap's own argument parser, and CI fails
if the committed bundle drifts from its sources. An agent reading it will not find a flag that
no longer exists.

## The rule that matters

**Use `--wait` on everything that moves the program.** An agent that omits it reads a stack
from before the program moved, concludes something false, and spends the next three turns
acting on it.

```bash
lazydap continue --wait --format json
```

The reply is one object describing the whole trip. It is nearly always all the agent needs,
and going fishing for the pieces separately costs turns and risks a race.

## The loop

```bash
# 1. Start the program, stopped before it runs.
lazydap launch ./mybinary --stop-on-entry --format json

# 2. Say where to stop. Paths are relative to the shell's cwd.
lazydap break src/parser.c:142 --format json

# 3. Run to it, and read the blob.
lazydap continue --wait --format json

# 4. Look around. Only meaningful while paused.
lazydap stack --format json
lazydap scopes --format json
lazydap eval "tokens[pos]" --format json

# 5. Finish.
lazydap disconnect --format json
```

`--format json` is worth writing even though a non-terminal stdout already selects it. It
makes the intent explicit and survives being run somewhere unexpected.

## Things that cost a turn

These are the mistakes that actually happen, in rough order of how often.

**A `variables_reference` expires the moment the program moves.** The numbers `scopes` returns
are good for that stop only. After any step, call `scopes` again. Reusing an old one fails
with `DapProtocolError: Invalid variabes reference` — the typo is codelldb's.

**Inspection needs a paused program.** `stack`, `scopes`, `variables` and `eval` return
`SessionNotPaused` while it is running:

```console
$ lazydap stack --format json
{"details":{"session_id":"74dc1a31-...","state":"running"},"error":"SessionNotPaused","message":"SessionNotPaused: session 74dc1a31-... is running; pause it first (`lazydap pause --wait`) or wait for a breakpoint"}
```

The fix is in the message: `lazydap pause --wait`, or set a breakpoint and continue to it.

**`--stop-on-entry` stops before `main`.** The stack says `_dyld_start` and none of the
program's variables exist yet, so `eval` reports an undeclared identifier. That is expected —
set a breakpoint and `continue --wait` before inspecting anything.

**Read `state`, not the exit code, to learn what happened to the program.** Exit `0` means the
*command* worked. `"state": "exited"` means the *program* finished. A command that
successfully reports a crash still exits `0`.

**One session at a time.** A second `launch` while one is live returns
`SessionAlreadyActive`. A session whose program has finished is cleared automatically, so
this usually only bites when the agent forgot it left something paused.

**Breakpoints outlive the session.** They are project state in `.lazydap/state.toml` and get
applied to later launches. Set them before launching if that is easier. Remove the ones you
added when you are done, or the next run stops in places nobody asked for.

**`eval` evaluates an expression**, in the program's language. It does not run debugger
commands unless you ask with `--context repl`, and with codelldb that mode runs an LLDB
*command*, under which `eval "x"` reads memory rather than the variable
([quirk 7](/reference/codelldb-quirks/#7-eval-with-context-repl-runs-an-lldb-command)).

## What an agent never needs to know

The Debug Adapter Protocol, the daemon, the Unix socket, or which adapter is running. lazydap
starts what it needs on first use. If an agent finds itself needing any of that, the CLI is
wrong and the right response is to say so rather than work around it.

## Piping between commands

`--format ids` exists for this:

```bash
lazydap break --list --format ids | xargs -I{} lazydap break --remove --id {}
```

More patterns in [output formats and piping](/guides/output-formats/).

## Reference for agents

Inside the skill bundle:

- `references/commands.md` — every command and flag, generated
- `references/output-schemas.md` — the JSON each command returns, field by field
- `references/error-codes.md` — exit codes, error names, and what to do about each
- `references/examples.md` — worked sessions, including a crash investigation

The same material is on this site: [CLI reference](/reference/cli/),
[JSON output](/reference/json-output/), [errors and exit codes](/reference/errors/).

For an agent working *on* lazydap rather than with it, `AGENTS.md` in the repository is the
entry point.
