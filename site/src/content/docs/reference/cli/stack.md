---
title: "lazydap stack"
description: "Show the call stack of a paused program"
---

:::note[Generated page]
From `lazydap stack --help`. To change it, change the clap definition in [`crates/daemon/src/cli.rs`](https://github.com/planetaryescape/lazydap/blob/main/crates/daemon/src/cli.rs), then run `npm run generate` in `site/` and commit the result. CI fails if this page and the binary disagree.
:::

Show the call stack of a paused program

```text
lazydap stack [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--thread <THREAD>` | Which thread. Defaults to the one that stopped last |
| `--levels <LEVELS>` | How many frames. `0` or unset means all of them |
| `--start <START>` | Skip this many frames from the top |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
