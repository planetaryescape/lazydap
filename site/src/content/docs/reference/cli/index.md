---
title: CLI reference
description: Every lazydap command, generated from the binary's own help.
---

:::note[Generated page]
From `lazydap --help` and one `lazydap <command> --help` per command. CI regenerates it and fails if anything drifts, so a flag listed here exists in the binary that produced it.
:::

lazydap is one binary with a subcommand per debugger operation. Run it with no arguments on a terminal and you get [the TUI](/getting-started/tui/); run it anywhere else and you get this help.

```text
lazydap [OPTIONS] [COMMAND]
```

## Commands

| Command | Does |
| --- | --- |
| [`lazydap launch`](/reference/cli/launch/) | Start a program under the debugger |
| [`lazydap launches`](/reference/cli/launches/) | List the project's named launch configurations, or run one |
| [`lazydap status`](/reference/cli/status/) | Show the daemon and its current session |
| [`lazydap disconnect`](/reference/cli/disconnect/) | End the current session |
| [`lazydap shutdown`](/reference/cli/shutdown/) | Stop the daemon and every session it owns |
| [`lazydap continue`](/reference/cli/continue/) <br/>`c` | Resume the program |
| [`lazydap step`](/reference/cli/step/) <br/>`next` | Run the next line, stepping over any call in it |
| [`lazydap step-in`](/reference/cli/step-in/) <br/>`step-into` | Step into the call on this line |
| [`lazydap step-out`](/reference/cli/step-out/) | Run until the current function returns |
| [`lazydap pause`](/reference/cli/pause/) | Interrupt a running program |
| [`lazydap break`](/reference/cli/break/) <br/>`b` | Set, list, remove or toggle breakpoints |
| [`lazydap stack`](/reference/cli/stack/) | Show the call stack of a paused program |
| [`lazydap scopes`](/reference/cli/scopes/) | Show the variable scopes of a frame |
| [`lazydap variables`](/reference/cli/variables/) | Expand a scope or a structured variable |
| [`lazydap eval`](/reference/cli/eval/) | Evaluate an expression in the debuggee |
| [`lazydap threads`](/reference/cli/threads/) | List the debuggee's threads |
| [`lazydap output`](/reference/cli/output/) | Show output the debuggee has produced |
| [`lazydap logs`](/reference/cli/logs/) | Show the daemon's log |
| [`lazydap doctor`](/reference/cli/doctor/) | Check that everything lazydap needs is where it should be |
| [`lazydap version`](/reference/cli/version/) | Print the lazydap and protocol versions |
| [`lazydap completions`](/reference/cli/completions/) | Print a shell completion script |
| [`lazydap tui`](/reference/cli/tui/) | Open the terminal UI. This is also what bare `lazydap` does on a terminal |
| [`lazydap daemon`](/reference/cli/daemon/) | Run the daemon. Normally started automatically by the first command that needs it |

## Options every command takes

| Flag | Description |
| --- | --- |
| `--instance <INSTANCE>` | Which daemon to talk to. Defaults to one per project root, and can also be set with LAZYDAP_INSTANCE |
| `--format <FORMAT>` | Output format. Defaults to `table` on a terminal and `json` when piped. One of `table`, `json`, `jsonl`, `csv`, `ids`. |
| `-h, --help` | Print help (see a summary with '-h') |

`--format` decides how a command answers. `table` is for reading and its layout is not a contract; `json` is [the contract](/reference/json-output/). With no `--format`, lazydap picks `table` on a terminal and `json` everywhere else, so a pipeline gets JSON without asking.

## Commands that move the program

`continue`, `step`, `step-in`, `step-out` and `pause` also take `--wait` and `--timeout`. Without `--wait` they return as soon as the debugger accepts the request, which is what a live UI wants and almost never what a script wants. See [the `--wait` contract](/guides/wait/).

`launch` does **not** take `--wait` — it answers with its own shape once the configuration phase is done. Pass `--stop-on-entry` to hold the program still, then `continue --wait` to move it.

## See also

- [Quickstart](/getting-started/quickstart/) — the commands in order, against a real program
- [JSON output](/reference/json-output/) — field-by-field schemas
- [Errors and exit codes](/reference/errors/) — every error name lazydap emits
