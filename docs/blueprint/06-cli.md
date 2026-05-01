# 06 — CLI

The CLI is the canonical interface. The TUI, agent skill, and any third-party frontend are clients of the same protocol the CLI uses.

This doc enumerates the v0.1 CLI surface and the conventions every subcommand follows.

## Conventions

Every command:

- Has `--help`. Run `lazydap <cmd> --help` to discover.
- Auto-detects output format from TTY: TTY → `table`, otherwise → `json`.
- Accepts `--format table|json|jsonl|csv|ids` to override.
- Mutations accept `--dry-run` and `--yes`.
- Returns an exit code per [`AGENTS.md`](../../AGENTS.md): 0 success, 1 error, 2 usage, 3 daemon, 4 adapter.
- Logs structured events to `{data_dir}/lazydap.log` via `tracing` (no stdout pollution).

## Top-level surface (v0.1)

### Session lifecycle

```
lazydap launch <program> [args...]
        --adapter <kind>          [default: codelldb]
        --cwd <dir>               [default: project root]
        --env KEY=VALUE           [repeatable]
        --stop-on-entry           [default: true for agents, false interactive]
        --no-stop-on-entry
        --launch-config <name>    use named config from .lazydap/state.toml
        --wait                    [default if --format json or non-TTY]
        --timeout <seconds>       [default: 30, 0 = infinite]

lazydap attach <pid>
        [same options as launch where applicable]

lazydap disconnect
        --terminate               kill the debuggee
        --session <id>            [default: only session]

lazydap status
        --watch                   [TTY only — refreshes display]

lazydap launches
        list | run <name> | add | edit | delete
```

### Stepping

All take `--wait` and `--timeout`. Without `--wait`, returns immediately; with `--wait`, blocks until stable state.

```
lazydap continue [--all-threads] [--wait] [--timeout N]
lazydap step                                              # step over (DAP next)
lazydap step-into
lazydap step-out
lazydap pause [--thread <id>]
lazydap until <file:line>                                 # run-to-cursor
```

### Breakpoints

```
lazydap break <file:line>
        --condition "<expr>"
        --hit-condition "<expr>"
        --log "<msg>"             # log point: prints, doesn't pause
        --disabled                # add but don't enable
        --dry-run

lazydap break --list
        --format json|table|ids

lazydap break --remove <file:line>
lazydap break --remove --id <bp-id>
lazydap break --remove --all [--dry-run]
lazydap break --toggle <file:line>
lazydap break --toggle-all
```

### Inspection

```
lazydap stack [--thread <id>] [--levels N] [--format json]
lazydap scopes [--frame <id>] [--format json]
lazydap variables --reference <vref> [--filter named|indexed] [--format json]

lazydap eval "<expression>"
        --frame <id>              [default: top frame]
        --context watch|repl|hover [default: repl]
        --format json

lazydap source --reference <id>   # for adapter-served virtual sources
```

### Watches

```
lazydap watch list
lazydap watch add "<expression>" [--label "<name>"]
lazydap watch remove --id <id>
lazydap watch values [--format json]    # current values, evaluated at last pause
```

### Output capture

```
lazydap output [--since <timestamp>] [--follow] [--format json|jsonl]
```

### Diagnostics

```
lazydap doctor
        --check-adapters
        --check-state
        --format json|table

lazydap logs
        --follow
        --level <debug|info|warn|error>
        --since <duration|timestamp>
        --limit N
        --purge                   # delete log file

lazydap status
lazydap version
lazydap completions <bash|zsh|fish|elvish|powershell>
```

### Daemon

```
lazydap daemon
        --foreground              # don't fork, stay in fg
        --log-level <...>
        --instance <name>         # override auto-detection

lazydap restart                   # kill + restart daemon
lazydap shutdown                  # ask daemon to exit cleanly
```

### Config

```
lazydap config
        show
        edit                      # open in $EDITOR
        path                      # print config file path
        init                      # create .lazydap/state.toml in project
```

### TUI

```
lazydap                           # bare → enters TUI if interactive
lazydap tui                       # explicit
```

## How CLI subcommands map to IPC

Each subcommand is a thin shim:

1. Parse args with `clap`.
2. Probe daemon (`Ping`); spawn if not running.
3. Send the corresponding `Request` over IPC.
4. Receive `Response` (or `Error`).
5. Format and print per `--format`.
6. Exit with appropriate code.

Example: `lazydap break main.c:42 --condition "x > 5"`:

```rust
let req = Request::BreakpointAdd(SourceBreakpoint {
    id: BreakpointId::new(),
    source: PathBuf::from("main.c"),
    line: 42,
    column: None,
    condition: Some("x > 5".into()),
    ...
});
let resp = ipc.request(req).await?;
match resp {
    Response::Breakpoint(bp) => print(&bp, &format),
    other => return Err(unexpected(other)),
}
```

That shim is ~30 lines per subcommand. Most live in `crates/daemon/src/cli/{module}.rs`, one per topic.

## `--wait` design recap

(Full detail in [`10-async-to-sync.md`](10-async-to-sync.md).)

```
Request flow with --wait:

  client                           daemon                          adapter
    │                                │                                │
    │   Continue { wait: Wait, .. }  │                                │
    ├───────────────────────────────►│                                │
    │                                │      DAP continue request      │
    │                                ├───────────────────────────────►│
    │                                │                                │
    │                                │      DAP continue response     │
    │                                │◄───────────────────────────────┤
    │                                │  (acknowledgement only)        │
    │                                │                                │
    │                                │   ◄─── DAP output event ───┐   │
    │                                │   ◄─── DAP output event ───┤   │
    │                                │   ◄─── DAP stopped event ──┘   │
    │                                │                                │
    │   StableState { state: Paused, │                                │
    │     captured_output: [...],    │                                │
    │     ... }                      │                                │
    │◄───────────────────────────────┤                                │
    │                                │                                │
```

The daemon buffers events between request and pause, returns them all in one response.

## Output formats

### `table` (default for TTY)

Human-readable. Aligned columns. Colours when terminal supports them. **Don't parse this.**

```
$ lazydap break --list
ID                    LINE        CONDITION      ENABLED
bp-01ABC...           main.c:42   x > 5          ✓
bp-01XYZ...           foo.c:101                  ✓
```

### `json` (default for non-TTY, or explicit)

Stable schema. Single JSON object or array per command.

```
$ lazydap break --list --format json
{
  "breakpoints": [
    { "id": "bp-01ABC...", "source": "main.c", "line": 42, "condition": "x > 5", "enabled": true },
    { "id": "bp-01XYZ...", "source": "foo.c", "line": 101, "condition": null, "enabled": true }
  ]
}
```

### `jsonl`

One JSON object per line. Used for streaming commands (`output --follow`, `logs --follow`).

```
$ lazydap output --follow --format jsonl
{"category":"Stdout","output":"hello\n","timestamp":"2026-04-30T12:34:56Z"}
{"category":"Stdout","output":"world\n","timestamp":"2026-04-30T12:34:57Z"}
```

### `csv`

For spreadsheet / pipeline-y use cases.

```
$ lazydap break --list --format csv
id,source,line,condition,enabled
bp-01ABC...,main.c,42,x > 5,true
bp-01XYZ...,foo.c,101,,true
```

### `ids`

Bare IDs, one per line. Useful for `xargs`.

```
$ lazydap break --list --format ids
bp-01ABC...
bp-01XYZ...
```

## Mutations: `--dry-run` and `--yes`

Every mutation supports both:

- **`--dry-run`** — shows what *would* happen, doesn't commit. Returns the same response shape with a top-level `dry_run: true` flag and `would_apply: <changes>` field.
- **`--yes`** — skip the confirmation prompt. Required for non-interactive use; agents always pass it.

If a mutation doesn't have `--dry-run`, it's a bug.

Selection logic for mutation must match the real mutation path. So:

```bash
lazydap break --remove --all --dry-run  # shows N breakpoints would be removed
lazydap break --remove --all --yes      # actually removes the same N
```

Same code path. Same logic. Tested as such.

## Help and discovery

```bash
lazydap --help                        # top-level command list
lazydap <subcommand> --help           # specific subcommand
lazydap --version                     # version string
lazydap completions <shell>           # generate completion script
```

`--help` output is generated from clap, with curated examples for non-trivial commands.

## What's NOT a CLI subcommand

- **Pane state, focus, scroll** — TUI-only.
- **Bulk reset / wipe** — too dangerous to expose without an explicit-design pass post-v0.1.
- **Adapter installation** — managed by `lazydap doctor` only as a check, not an install. Mason / VS Code do the install.
- **AI advisor invocation** — external clients (see [`12-ai-future.md`](12-ai-future.md)).

## See also

- [`04-protocol.md`](04-protocol.md) — IPC types each subcommand maps to
- [`09-skill.md`](09-skill.md) — how the agent skill exposes these commands
- [`docs/articles/the-cli-is-the-product.md`](../articles/the-cli-is-the-product.md) — why CLI-first matters here
