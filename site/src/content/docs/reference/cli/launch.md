---
title: "lazydap launch"
description: "Start a program under the debugger"
---

:::note[Generated page]
From `lazydap launch --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Start a program under the debugger

```text
lazydap launch [OPTIONS] <PROGRAM> [-- <ARGS>...]
```

## Arguments

| Argument | Description |
| --- | --- |
| `<PROGRAM>` | The program to debug |
| `[ARGS]...` | Arguments for the debuggee, after a `--` separator. They are kept separate so a debuggee flag can never be mistaken for a lazydap one |

## Options

| Flag | Description |
| --- | --- |
| `--stop-on-entry` | Stop at the program's entry point instead of running to the first breakpoint |
| `--cwd <CWD>` | Working directory for the debuggee. Defaults to the current one |
| `--env <KEY=VALUE>` | Environment for the debuggee, as KEY=VALUE. Repeatable |
| `--adapter <ADAPTER>` | Which debug adapter to use [default: codelldb] |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
