# lazydap commands

Generated from lazydap's own argument parser by
`cargo run --example gen_skill_commands`. Do not edit by hand — edit
`crates/daemon/src/cli.rs` and rebuild the skill.

Every command accepts the global flags below, and every command that
prints a result accepts `--format`. Exit codes are in
[`error-codes.md`](error-codes.md); the JSON shapes are in
[`output-schemas.md`](output-schemas.md).

## Global flags

| Argument | Required | Default | Description |
|---|---|---|---|
| `--instance <INSTANCE>` | no | - | Which daemon to talk to. Defaults to one per project root, and can also be set with LAZYDAP_INSTANCE |
| `--format <FORMAT>` | no | - | Output format. Defaults to `table` on a terminal and `json` when piped. One of: `table`, `json`, `jsonl`, `csv`, `ids` |

## Commands

### `lazydap launch`

Start a program under the debugger

```
Usage: lazydap launch [OPTIONS] <PROGRAM> [-- <ARGS>...]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<PROGRAM>` | yes | - | The program to debug |
| `--stop-on-entry` | no | `false` | Stop at the program's entry point instead of running to the first breakpoint |
| `--cwd <CWD>` | no | - | Working directory for the debuggee. Defaults to the current one |
| `--env <KEY=VALUE>` | no | - | Environment for the debuggee, as KEY=VALUE. Repeatable |
| `--adapter <ADAPTER>` | no | - | Which debug adapter to use. Defaults to the one the program's file extension implies — debugpy for `.py`, codelldb otherwise |
| `<ARGS>` | no | - | Arguments for the debuggee, after a `--` separator. They are kept separate so a debuggee flag can never be mistaken for a lazydap one |

### `lazydap launches`

List the project's named launch configurations, or run one.

They come from `.lazydap/state.toml` and from `.vscode/launch.json`, which lazydap reads and never writes.

```
Usage: lazydap launches [OPTIONS] <COMMAND>
```


#### `lazydap launches list`

Show every launch configuration, and whether lazydap can run it

```
Usage: lazydap launches list [OPTIONS]
```


#### `lazydap launches run`

Start the configuration with this name

```
Usage: lazydap launches run [OPTIONS] <NAME>
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<NAME>` | yes | - | Its `name` in `launch.json` or `state.toml` |
| `--stop-on-entry` | no | `false` | Stop at the program's entry point, whatever the configuration says |

### `lazydap status`

Show the daemon and its current session

```
Usage: lazydap status [OPTIONS]
```


### `lazydap disconnect`

End the current session

```
Usage: lazydap disconnect [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--session-id <SESSION_ID>` | no | - | Which session to end. Defaults to the active one |
| `--no-terminate` | no | `false` | Leave the debuggee running instead of killing it |
| `--dry-run` | no | `false` | Report what would be ended, and end nothing |

### `lazydap shutdown`

Stop the daemon and every session it owns

```
Usage: lazydap shutdown [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--dry-run` | no | `false` | Report what would be stopped, and stop nothing |

### `lazydap continue`

*Also spelled:* `c`

Resume the program

```
Usage: lazydap continue [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--wait` | no | `false` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | no | - | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--all-threads` | no | `false` | Wait for every thread to stop, not just the first |
| `--thread <THREAD>` | no | - | Which thread to resume. Defaults to the one that stopped last |

### `lazydap step`

*Also spelled:* `next`

Run the next line, stepping over any call in it

```
Usage: lazydap step [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--wait` | no | `false` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | no | - | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--thread <THREAD>` | no | - |  |

### `lazydap step-in`

*Also spelled:* `step-into`

Step into the call on this line

```
Usage: lazydap step-in [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--wait` | no | `false` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | no | - | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--thread <THREAD>` | no | - |  |

### `lazydap step-out`

Run until the current function returns

```
Usage: lazydap step-out [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--wait` | no | `false` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | no | - | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--thread <THREAD>` | no | - |  |

### `lazydap pause`

Interrupt a running program

```
Usage: lazydap pause [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--wait` | no | `false` | Block until the program pauses, exits or is terminated, and return one JSON object describing everything that happened. Always use this from a script or an agent |
| `--timeout <TIMEOUT>` | no | - | How long to wait, in seconds. `0` waits forever. Defaults to 30, or to LAZYDAP_TIMEOUT |
| `--thread <THREAD>` | no | - |  |

### `lazydap break`

*Also spelled:* `b`

Set, list, remove or toggle breakpoints.

Breakpoints are project state: they are remembered in `.lazydap/state.toml` and applied to every session you launch, whether or not one is running when you set them.

```
Usage: lazydap break [OPTIONS] [FILE:LINE]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<FILE:LINE>` | no | - | Where to break, as `file:line` |
| `--list` | no | `false` | List every breakpoint in the project |
| `--remove` | no | `false` | Remove the selected breakpoints |
| `--toggle` | no | `false` | Enable or disable the selected breakpoints |
| `--id <ID>` | no | - | Select by id. Repeatable, and what `--format ids` output feeds |
| `--all` | no | `false` | Select every breakpoint in the project |
| `--condition <CONDITION>` | no | - | Only break when this expression is true |
| `--hit-condition <HIT_CONDITION>` | no | - | Only break once the hit count matches, e.g. `>= 10` |
| `--log <MESSAGE>` | no | - | Log this message instead of pausing. Braces interpolate: `--log "x = {x}"` |
| `--disabled` | no | `false` | Record it, but leave it switched off |
| `--dry-run` | no | `false` | Report what would change, and change nothing |

### `lazydap watch`

*Also spelled:* `w`

Add, list or remove watch expressions.

Watches are project state, exactly as breakpoints are: they are remembered in `.lazydap/state.toml` and outlive the session, the daemon and the machine. What one *evaluates to* is not remembered — ask for that with `lazydap eval`, or watch the TUI's watches pane, which re-evaluates all of them every time the program stops.

```
Usage: lazydap watch [OPTIONS] <COMMAND>
```


#### `lazydap watch add`

Watch an expression at every stop

```
Usage: lazydap watch add [OPTIONS] <EXPRESSION>
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<EXPRESSION>` | yes | - | Handed to the adapter untouched. Quote it if it has spaces |
| `--label <LABEL>` | no | - | Show this instead of the expression, when the expression is long |
| `--dry-run` | no | `false` | Report what would change, and change nothing |

#### `lazydap watch list`

Show every watch in the project

```
Usage: lazydap watch list [OPTIONS]
```


#### `lazydap watch remove`

Stop watching. Name the expression, or select by id

```
Usage: lazydap watch remove [OPTIONS] [EXPRESSION]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<EXPRESSION>` | no | - | The expression, matched whole |
| `--id <ID>` | no | - | Select by id. Repeatable, and what `--format ids` output feeds |
| `--all` | no | `false` | Remove every watch in the project |
| `--dry-run` | no | `false` | Report what would change, and change nothing |

### `lazydap stack`

Show the call stack of a paused program

```
Usage: lazydap stack [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--thread <THREAD>` | no | - | Which thread. Defaults to the one that stopped last |
| `--levels <LEVELS>` | no | - | How many frames. Defaults to all of them |
| `--start <START>` | no | - | Skip this many frames from the top |

### `lazydap scopes`

Show the variable scopes of a frame

```
Usage: lazydap scopes [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--frame <FRAME>` | no | - | Which frame. Defaults to the top one |

### `lazydap variables`

Expand a scope or a structured variable

```
Usage: lazydap variables [OPTIONS] --reference <REFERENCE>
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--reference <REFERENCE>` | yes | - | The `variables_reference` from `scopes` or a parent variable |
| `--filter <FILTER>` | no | `all` | Fetch only named members or only indexed elements |
| `--start <START>` | no | - | Skip this many |
| `--count <COUNT>` | no | - | Take at most this many |

### `lazydap eval`

Evaluate an expression in the debuggee

```
Usage: lazydap eval [OPTIONS] <EXPRESSION>
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<EXPRESSION>` | yes | - | The expression, in the debuggee's own language |
| `--frame <FRAME>` | no | - | Which frame to evaluate in. Defaults to the top one |
| `--context <CONTEXT>` | no | `watch` | How the adapter should read the expression. `watch` and `hover` evaluate it in the program; `repl` runs it as an adapter command, which for codelldb means an LLDB command |

### `lazydap threads`

List the debuggee's threads

```
Usage: lazydap threads [OPTIONS]
```


### `lazydap output`

Show output the debuggee has produced

```
Usage: lazydap output [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--since <SINCE>` | no | - | Only output at or after this Unix-epoch millisecond |

### `lazydap logs`

Show the daemon's log

```
Usage: lazydap logs [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--limit <LIMIT>` | no | `200` | Show at most this many lines, from the end |
| `--level <LEVEL>` | no | - | Only lines at this level or louder |
| `--follow` | no | `false` | Keep printing as the daemon writes more |
| `--purge` | no | `false` | Delete the log file instead of printing it |

### `lazydap doctor`

Check that everything lazydap needs is where it should be

```
Usage: lazydap doctor [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--check-adapters` | no | `false` | Only check the adapters |
| `--check-state` | no | `false` | Only check the project state file |

### `lazydap version`

Print the lazydap and protocol versions

```
Usage: lazydap version [OPTIONS]
```


### `lazydap completions`

Print a shell completion script

```
Usage: lazydap completions [OPTIONS] <SHELL>
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `<SHELL>` | yes | - | Which shell. One of: `bash`, `elvish`, `fish`, `powershell`, `zsh` |

### `lazydap tui`

Open the terminal UI. This is also what bare `lazydap` does on a terminal

```
Usage: lazydap tui [OPTIONS]
```


### `lazydap daemon`

Run the daemon. Normally started automatically by the first command that needs it

```
Usage: lazydap daemon [OPTIONS]
```

| Argument | Required | Default | Description |
|---|---|---|---|
| `--foreground` | no | `false` | Stay in the terminal and log to stderr, for debugging |

