---
title: "lazydap break"
description: "Set, list, remove or toggle breakpoints"
---

:::note[Generated page]
From `lazydap break --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Also spelled `lazydap b`.

Set, list, remove or toggle breakpoints.

Breakpoints are project state: they are remembered in `.lazydap/state.toml` and applied to every session you launch, whether or not one is running when you set them.

```text
lazydap break [OPTIONS] [FILE:LINE]
```

## Arguments

| Argument | Description |
| --- | --- |
| `[FILE:LINE]` | Where to break, as `file:line` |

## Options

| Flag | Description |
| --- | --- |
| `--list` | List every breakpoint in the project |
| `--remove` | Remove the selected breakpoints |
| `--toggle` | Enable or disable the selected breakpoints |
| `--id <ID>` | Select by id. Repeatable, and what `--format ids` output feeds |
| `--all` | Select every breakpoint in the project |
| `--condition <CONDITION>` | Only break when this expression is true |
| `--hit-condition <HIT_CONDITION>` | Only break once the hit count matches, e.g. `>= 10` |
| `--log <MESSAGE>` | Log this message instead of pausing. Braces interpolate: `--log "x = {x}"` |
| `--disabled` | Record it, but leave it switched off |
| `--dry-run` | Report what would change, and change nothing |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
