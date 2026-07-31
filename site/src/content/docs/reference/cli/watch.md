---
title: "lazydap watch"
description: "Add, list or remove watch expressions"
---

:::note[Generated page]
From `lazydap watch --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Also spelled `lazydap w`.

Add, list or remove watch expressions.

Watches are project state, exactly as breakpoints are: they are remembered in `.lazydap/state.toml` and outlive the session, the daemon and the machine. What one *evaluates to* is not remembered — ask for that with `lazydap eval`, or watch the TUI's watches pane, which re-evaluates all of them every time the program stops.

```text
lazydap watch [OPTIONS] <COMMAND>
```

## `lazydap watch add`

Watch an expression at every stop

```text
lazydap watch add [OPTIONS] <EXPRESSION>
```

### Arguments

| Argument | Description |
| --- | --- |
| `<EXPRESSION>` | Handed to the adapter untouched. Quote it if it has spaces |

### Options

| Flag | Description |
| --- | --- |
| `--label <LABEL>` | Show this instead of the expression, when the expression is long |
| `--dry-run` | Report what would change, and change nothing |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## `lazydap watch list`

Show every watch in the project

```text
lazydap watch list [OPTIONS]
```

## `lazydap watch remove`

Stop watching. Name the expression, or select by id

```text
lazydap watch remove [OPTIONS] [EXPRESSION]
```

### Arguments

| Argument | Description |
| --- | --- |
| `[EXPRESSION]` | The expression, matched whole |

### Options

| Flag | Description |
| --- | --- |
| `--id <ID>` | Select by id. Repeatable, and what `--format ids` output feeds |
| `--all` | Remove every watch in the project |
| `--dry-run` | Report what would change, and change nothing |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
