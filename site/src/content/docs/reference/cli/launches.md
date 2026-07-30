---
title: "lazydap launches"
description: "List the project's named launch configurations, or run one"
---

:::note[Generated page]
From `lazydap launches --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

List the project's named launch configurations, or run one.

They come from `.lazydap/state.toml` and from `.vscode/launch.json`, which lazydap reads and never writes.

```text
lazydap launches [OPTIONS] <COMMAND>
```

## `lazydap launches list`

Show every launch configuration, and whether lazydap can run it

```text
lazydap launches list [OPTIONS]
```

## `lazydap launches run`

Start the configuration with this name

```text
lazydap launches run [OPTIONS] <NAME>
```

### Arguments

| Argument | Description |
| --- | --- |
| `<NAME>` | Its `name` in `launch.json` or `state.toml` |

### Options

| Flag | Description |
| --- | --- |
| `--stop-on-entry` | Stop at the program's entry point, whatever the configuration says |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
