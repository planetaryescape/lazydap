---
title: "lazydap daemon"
description: "Run the daemon. Normally started automatically by the first command that needs it"
---

:::note[Generated page]
From `lazydap daemon --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Run the daemon. Normally started automatically by the first command that needs it

```text
lazydap daemon [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--foreground` | Stay in the terminal and log to stderr, for debugging |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
