---
title: "lazydap logs"
description: "Show the daemon's log"
---

:::note[Generated page]
From `lazydap logs --help`. To change it, change the clap definition in [`crates/daemon/src/cli/`](https://github.com/planetaryescape/lazydap/tree/main/crates/daemon/src/cli) — the site rebuilds from the binary.
:::

Show the daemon's log

```text
lazydap logs [OPTIONS]
```

## Options

| Flag | Description |
| --- | --- |
| `--limit <LIMIT>` | Show at most this many lines, from the end [default: 200] |
| `--level <LEVEL>` | Only lines at this level or louder |
| `--follow` | Keep printing as the daemon writes more |
| `--purge` | Delete the log file instead of printing it |

Plus the [options every command takes](/reference/cli/#options-every-command-takes).

## See also

- [CLI overview](/reference/cli/) — every command in one list
- [JSON output](/reference/json-output/) — the shape of what comes back
- [Errors and exit codes](/reference/errors/) — what to do when it fails
